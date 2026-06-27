//! Lightweight metrics SPI for host applications and platform SDK adapters.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// Stable metric label used by sinks and diagnostics snapshots.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricLabel {
    pub key: String,
    pub value: String,
}

impl MetricLabel {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistogramSnapshot {
    pub count: u64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
}

impl HistogramSnapshot {
    fn record(&mut self, value: f64) {
        if self.count == 0 {
            self.min = value;
            self.max = value;
        } else {
            self.min = self.min.min(value);
            self.max = self.max.max(value);
        }
        self.count = self.count.saturating_add(1);
        self.sum += value;
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsSnapshot {
    pub counters: BTreeMap<String, u64>,
    pub gauges: BTreeMap<String, i64>,
    pub histograms: BTreeMap<String, HistogramSnapshot>,
}

/// Platform or host-provided exporter for SDK metrics.
///
/// The trait is synchronous by design so hot SDK paths can emit metrics without
/// `.await`; sinks should keep implementations non-blocking.
pub trait MetricsSink: Send + Sync {
    fn increment_counter(&self, name: &str, labels: &[MetricLabel], value: u64);
    fn record_gauge(&self, name: &str, labels: &[MetricLabel], value: i64);
    fn record_histogram(&self, name: &str, labels: &[MetricLabel], value: f64);
}

#[derive(Debug, Default)]
pub struct InMemoryMetricsSink {
    snapshot: Mutex<MetricsSnapshot>,
}

impl InMemoryMetricsSink {
    pub fn snapshot(&self) -> MetricsSnapshot {
        self.snapshot
            .lock()
            .map(|snapshot| snapshot.clone())
            .unwrap_or_default()
    }
}

impl MetricsSink for InMemoryMetricsSink {
    fn increment_counter(&self, name: &str, labels: &[MetricLabel], value: u64) {
        let Ok(mut snapshot) = self.snapshot.lock() else {
            return;
        };
        let key = metric_key(name, labels);
        let counter = snapshot.counters.entry(key).or_insert(0);
        *counter = counter.saturating_add(value);
    }

    fn record_gauge(&self, name: &str, labels: &[MetricLabel], value: i64) {
        let Ok(mut snapshot) = self.snapshot.lock() else {
            return;
        };
        snapshot.gauges.insert(metric_key(name, labels), value);
    }

    fn record_histogram(&self, name: &str, labels: &[MetricLabel], value: f64) {
        let Ok(mut snapshot) = self.snapshot.lock() else {
            return;
        };
        snapshot
            .histograms
            .entry(metric_key(name, labels))
            .or_default()
            .record(value);
    }
}

struct MetricsHub {
    snapshot: Arc<InMemoryMetricsSink>,
    external: Option<Arc<dyn MetricsSink>>,
}

/// Cloneable SDK metrics recorder. Disabled recorders are cheap no-ops.
#[derive(Clone, Default)]
pub struct MetricsRecorder {
    hub: Option<Arc<MetricsHub>>,
}

impl MetricsRecorder {
    pub fn disabled() -> Self {
        Self { hub: None }
    }

    pub fn enabled(external: Option<Arc<dyn MetricsSink>>) -> Self {
        Self {
            hub: Some(Arc::new(MetricsHub {
                snapshot: Arc::new(InMemoryMetricsSink::default()),
                external,
            })),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.hub.is_some()
    }

    pub fn counter(&self, name: &str, value: u64) {
        self.counter_with_labels(name, &[], value);
    }

    pub fn counter_with_labels(&self, name: &str, labels: &[MetricLabel], value: u64) {
        let Some(hub) = &self.hub else {
            return;
        };
        hub.snapshot.increment_counter(name, labels, value);
        if let Some(sink) = &hub.external {
            sink.increment_counter(name, labels, value);
        }
    }

    pub fn gauge(&self, name: &str, value: i64) {
        self.gauge_with_labels(name, &[], value);
    }

    pub fn gauge_with_labels(&self, name: &str, labels: &[MetricLabel], value: i64) {
        let Some(hub) = &self.hub else {
            return;
        };
        hub.snapshot.record_gauge(name, labels, value);
        if let Some(sink) = &hub.external {
            sink.record_gauge(name, labels, value);
        }
    }

    pub fn histogram(&self, name: &str, value: f64) {
        self.histogram_with_labels(name, &[], value);
    }

    pub fn histogram_with_labels(&self, name: &str, labels: &[MetricLabel], value: f64) {
        let Some(hub) = &self.hub else {
            return;
        };
        hub.snapshot.record_histogram(name, labels, value);
        if let Some(sink) = &hub.external {
            sink.record_histogram(name, labels, value);
        }
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        self.hub
            .as_ref()
            .map(|hub| hub.snapshot.snapshot())
            .unwrap_or_default()
    }
}

fn metric_key(name: &str, labels: &[MetricLabel]) -> String {
    let mut labels = labels.to_vec();
    labels.sort();
    if labels.is_empty() {
        return name.to_string();
    }
    let suffix = labels
        .iter()
        .map(|label| format!("{}={}", label.key, label.value))
        .collect::<Vec<_>>()
        .join(",");
    format!("{name}{{{suffix}}}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorder_keeps_diagnostics_snapshot_and_forwards_to_sink() {
        let external = Arc::new(InMemoryMetricsSink::default());
        let recorder = MetricsRecorder::enabled(Some(external.clone()));

        recorder.counter_with_labels(
            "send_total",
            &[
                MetricLabel::new("result", "ok"),
                MetricLabel::new("path", "queue"),
            ],
            2,
        );
        recorder.gauge("queue_depth", 3);
        recorder.histogram("send_latency_ms", 12.0);

        let snapshot = recorder.snapshot();
        assert_eq!(snapshot.counters["send_total{path=queue,result=ok}"], 2);
        assert_eq!(snapshot.gauges["queue_depth"], 3);
        assert_eq!(snapshot.histograms["send_latency_ms"].count, 1);

        assert_eq!(
            external.snapshot().counters["send_total{path=queue,result=ok}"],
            2
        );
    }
}
