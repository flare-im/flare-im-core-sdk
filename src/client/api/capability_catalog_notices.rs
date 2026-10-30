//! The tenant's plugin catalog-changed notices (`flare.plugin.catalog` /
//! `changed`). A notice invalidates the catalog at once; hosts hear about it
//! (`PluginEvent::CatalogChanged`) after a random 0-3 s delay, so a tenant's
//! clients do not all read the catalog in the same instant, and the notices
//! that arrive meanwhile are merged into that one announcement.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rand::Rng;

use super::CapabilityApi;
use crate::kernel::PluginCatalogNoticePort;
use crate::kernel::event::{EventBus, PluginEvent, SdkEvent};
use crate::shared::util::{delay, now_millis, spawn_background};

/// The longest random delay before a catalog change is announced, and the
/// shortest gap between two announcements.
pub(super) const CATALOG_NOTICE_WINDOW_MS: u64 = 3_000;

/// When to announce: one announcement per burst of notices, at least
/// [`CATALOG_NOTICE_WINDOW_MS`] after the previous one.
#[derive(Debug, Default)]
pub(super) struct CatalogNoticeWindow {
    pending: Option<String>,
    last_announced_at_ms: Option<u64>,
}

impl CatalogNoticeWindow {
    /// Records a notice of `version` at `now_ms`. Returns the delay of the
    /// announcement it schedules (the random `jitter_ms`, capped at the
    /// window, and never sooner than a window after the last announcement),
    /// or `None` when one is already scheduled: that one announces `version`.
    pub(super) fn notice(&mut self, version: String, now_ms: u64, jitter_ms: u64) -> Option<u64> {
        if self.pending.replace(version).is_some() {
            return None;
        }
        let gap_ms = self.last_announced_at_ms.map_or(0, |last| {
            (last + CATALOG_NOTICE_WINDOW_MS).saturating_sub(now_ms)
        });
        Some(jitter_ms.min(CATALOG_NOTICE_WINDOW_MS).max(gap_ms))
    }

    /// The version to announce at `now_ms`; `None` when nothing is pending.
    pub(super) fn take(&mut self, now_ms: u64) -> Option<String> {
        let version = self.pending.take()?;
        self.last_announced_at_ms = Some(now_ms);
        Some(version)
    }
}

fn random_jitter_ms() -> u64 {
    rand::thread_rng().gen_range(0..=CATALOG_NOTICE_WINDOW_MS)
}

/// [`PluginCatalogNoticePort`] of a client: invalidates its plugin catalog and
/// announces the change on its event bus.
#[derive(Clone)]
pub(crate) struct PluginCatalogNotices {
    capability: CapabilityApi,
    bus: EventBus,
    window: Arc<Mutex<CatalogNoticeWindow>>,
    jitter_ms: fn() -> u64,
}

impl PluginCatalogNotices {
    pub(crate) fn new(capability: CapabilityApi, bus: EventBus) -> Self {
        Self::with_jitter(capability, bus, random_jitter_ms)
    }

    fn with_jitter(capability: CapabilityApi, bus: EventBus, jitter_ms: fn() -> u64) -> Self {
        Self {
            capability,
            bus,
            window: Arc::new(Mutex::new(CatalogNoticeWindow::default())),
            jitter_ms,
        }
    }

    fn window(&self) -> std::sync::MutexGuard<'_, CatalogNoticeWindow> {
        self.window.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Re-announces the recent plugin cards against the fresh catalog, then
    /// tells hosts to read their plugin entries again.
    async fn announce(&self) {
        let Some(version) = self.window().take(now_millis()) else {
            return;
        };
        if let Err(error) = self.capability.invalidate_plugin_catalog().await {
            tracing::debug!(target: "flare_sdk.plugin", %error, "plugin catalog invalidation failed");
        }
        self.bus
            .publish(SdkEvent::Plugin(PluginEvent::CatalogChanged { version }));
    }
}

impl PluginCatalogNoticePort for PluginCatalogNotices {
    fn catalog_changed(&self, version: String) {
        // Reads in flight now answer for the old catalog: never share them.
        self.capability.invalidate_catalog_reads();
        let jitter_ms = (self.jitter_ms)();
        let Some(delay_ms) = self.window().notice(version, now_millis(), jitter_ms) else {
            return;
        };
        let this = self.clone();
        spawn_background(async move {
            delay(Duration::from_millis(delay_ms)).await;
            this.announce().await;
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::RwLock;

    use super::*;
    use crate::infrastructure::transport::http::http_client::HttpRequestContext;
    use crate::kernel::event::{PluginEventType, SdkEventType};

    #[test]
    fn a_lone_notice_waits_its_jitter_capped_at_the_window() {
        for (jitter, expected) in [(0, 0), (1_234, 1_234), (3_000, 3_000), (9_999, 3_000)] {
            let mut window = CatalogNoticeWindow::default();
            assert_eq!(window.notice("1".into(), 10_000, jitter), Some(expected));
        }
    }

    #[test]
    fn random_jitter_stays_inside_the_window() {
        for _ in 0..1_000 {
            assert!(random_jitter_ms() <= CATALOG_NOTICE_WINDOW_MS);
        }
    }

    #[test]
    fn notices_before_the_announcement_merge_into_it_with_the_latest_version() {
        let mut window = CatalogNoticeWindow::default();
        assert_eq!(window.notice("1".into(), 0, 2_000), Some(2_000));
        assert_eq!(window.notice("2".into(), 500, 0), None);
        assert_eq!(window.notice("3".into(), 1_900, 0), None);
        assert_eq!(window.take(2_000), Some("3".into()));
        assert_eq!(window.take(2_001), None);
    }

    #[test]
    fn announcements_are_a_window_apart() {
        let mut window = CatalogNoticeWindow::default();
        assert_eq!(window.notice("1".into(), 0, 100), Some(100));
        assert_eq!(window.take(100), Some("1".into()));
        // 0.4 s after the announcement: wait out the rest of the window.
        assert_eq!(window.notice("2".into(), 500, 0), Some(2_600));
        assert_eq!(window.take(3_100), Some("2".into()));
        // Long after: only the jitter.
        assert_eq!(window.notice("3".into(), 60_000, 700), Some(700));
    }

    fn capability_api() -> CapabilityApi {
        CapabilityApi::new(
            "http://127.0.0.1:1",
            Arc::new(RwLock::new("u1".to_string())),
            "201",
            Arc::new(HttpRequestContext::new()),
        )
    }

    #[tokio::test(start_paused = true)]
    async fn a_notice_invalidates_the_catalog_then_announces_once_after_the_jitter() {
        let api = capability_api();
        let bus = EventBus::new();
        let announced = Arc::new(Mutex::new(Vec::<String>::new()));
        let seen = announced.clone();
        let _subscription = bus.on_event_type(
            SdkEventType::Plugin(PluginEventType::CatalogChanged),
            move |event| {
                if let SdkEvent::Plugin(PluginEvent::CatalogChanged { version }) = event.as_ref() {
                    seen.lock().unwrap().push(version.clone());
                }
            },
        );
        let notices = PluginCatalogNotices::with_jitter(api.clone(), bus.clone(), || 1_500);
        let generation = api.catalog_flight.generation();

        notices.catalog_changed("7".into());
        assert!(
            api.catalog_flight.generation() > generation,
            "the catalog is invalidated before the announcement"
        );
        notices.catalog_changed("8".into());

        tokio::time::sleep(Duration::from_millis(1_400)).await;
        assert!(
            announced.lock().unwrap().is_empty(),
            "nothing before the jitter"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
        // The announcement first tries to invalidate the catalog over real HTTP, and the bus
        // hands it to its subscribers on a blocking thread; neither moves with the paused
        // clock. Give them real time, bounded, instead of a fixed number of yields, which a
        // loaded runner does not always cover.
        for _ in 0..500 {
            if !announced.lock().unwrap().is_empty() {
                break;
            }
            tokio::task::spawn_blocking(|| std::thread::sleep(Duration::from_millis(2)))
                .await
                .unwrap();
        }
        assert_eq!(*announced.lock().unwrap(), vec!["8".to_string()]);
    }
}
