use crate::kernel::event::ReadinessStage;

use super::task::{SyncReason, SyncRunContext, SyncTrigger};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupSyncWaitReport {
    pub run: SyncRunContext,
    pub started_at_ms: u64,
    pub local_ready_wait_ms: Option<u64>,
    pub foreground_fresh_wait_ms: Option<u64>,
    pub converged_wait_ms: Option<u64>,
    pub hot_calibration_wait_ms: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct StartupSyncTiming {
    run: SyncRunContext,
    started_at_ms: u64,
    local_ready_at_ms: Option<u64>,
    foreground_fresh_at_ms: Option<u64>,
    converged_at_ms: Option<u64>,
}

impl StartupSyncTiming {
    pub fn new(run: SyncRunContext, started_at_ms: u64) -> Self {
        Self {
            run,
            started_at_ms,
            local_ready_at_ms: None,
            foreground_fresh_at_ms: None,
            converged_at_ms: None,
        }
    }

    pub fn record_readiness(&mut self, run: &SyncRunContext, stage: ReadinessStage, at_ms: u64) {
        if run.run_id != self.run.run_id {
            return;
        }
        match stage {
            ReadinessStage::LocalReady => {
                self.local_ready_at_ms = Some(min_existing(self.local_ready_at_ms, at_ms));
            }
            ReadinessStage::ForegroundFresh => {
                self.foreground_fresh_at_ms =
                    Some(min_existing(self.foreground_fresh_at_ms, at_ms));
            }
            ReadinessStage::Converged => {
                self.converged_at_ms = Some(min_existing(self.converged_at_ms, at_ms));
            }
            ReadinessStage::Degraded => {}
        }
    }

    pub fn report(&self) -> Option<StartupSyncWaitReport> {
        if self.local_ready_at_ms.is_none()
            && self.foreground_fresh_at_ms.is_none()
            && self.converged_at_ms.is_none()
        {
            return None;
        }

        let local_ready_wait_ms = self.wait_from_start(self.local_ready_at_ms);
        let converged_wait_ms = self.wait_from_start(self.converged_at_ms);
        let hot_calibration_wait_ms = if self.is_warm_start_calibration() {
            self.local_ready_at_ms
                .zip(self.converged_at_ms)
                .map(|(local, converged)| converged.saturating_sub(local))
        } else {
            None
        };

        Some(StartupSyncWaitReport {
            run: self.run.clone(),
            started_at_ms: self.started_at_ms,
            local_ready_wait_ms,
            foreground_fresh_wait_ms: self.wait_from_start(self.foreground_fresh_at_ms),
            converged_wait_ms,
            hot_calibration_wait_ms,
        })
    }

    fn wait_from_start(&self, at_ms: Option<u64>) -> Option<u64> {
        at_ms.map(|at| at.saturating_sub(self.started_at_ms))
    }

    fn is_warm_start_calibration(&self) -> bool {
        self.run.trigger == SyncTrigger::WarmStartupCalibration
            || self.run.reason == SyncReason::WarmStartupCalibration
    }
}

fn min_existing(existing: Option<u64>, next: u64) -> u64 {
    existing.map_or(next, |value| value.min(next))
}
