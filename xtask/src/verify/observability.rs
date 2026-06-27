use anyhow::Result;
use std::{fs, path::Path};

use crate::{core_root, emit_errors, fail};

pub(crate) fn verify_observability_gate(root: &Path) -> Result<()> {
    let mut errors = Vec::new();
    let sdk_root = core_root(root);

    require_contains_all(
        &mut errors,
        &root.join("docs/client-observability.md"),
        "client observability doc",
        &[
            "client.diagnostics.getRuntimeHealth",
            "MetricsSink",
            "send→ack",
            "event.raw_subscriber_dropped_total",
        ],
    );

    require_contains_all(
        &mut errors,
        &sdk_root.join("src/spi/metrics.rs"),
        "metrics SPI",
        &[
            "pub trait MetricsSink",
            "pub struct MetricsRecorder",
            "pub struct MetricsSnapshot",
            "increment_counter",
            "record_histogram",
        ],
    );

    require_contains_all(
        &mut errors,
        &sdk_root.join("src/client/im_client.rs"),
        "runtime health API",
        &[
            "pub async fn runtime_health_snapshot",
            "metrics_enabled",
            "raw_subscriber_dropped_total",
            "metrics: g.metrics.snapshot()",
        ],
    );

    require_contains_all(
        &mut errors,
        &root.join("sdk-spec/modules/diagnostics.json"),
        "diagnostics module contract",
        &[
            "\"operation\": \"diagnostics.runtime_health\"",
            "\"response\": \"RuntimeHealthResponse\"",
            "\"transport\": \"contract-invoke-json\"",
        ],
    );
    require_contains_all(
        &mut errors,
        &root.join("sdk-spec/models/diagnostics.json"),
        "diagnostics model contract",
        &[
            "\"name\": \"metricsEnabled\"",
            "\"name\": \"rawSubscriberDroppedTotal\"",
            "\"name\": \"metricsJson\"",
        ],
    );

    require_contains_all(
        &mut errors,
        &sdk_root.join("tests/runtime_observability.rs"),
        "runtime observability regression test",
        &[
            "runtime_health_reports_event_bus_backpressure_metrics",
            "enable_metrics(true)",
            "metrics_sink_arc",
            "event.raw_subscriber_dropped_total",
        ],
    );

    emit_errors("observability gate", errors)
}

fn require_contains_all(errors: &mut Vec<String>, path: &Path, label: &str, needles: &[&str]) {
    let Ok(text) = fs::read_to_string(path) else {
        fail(
            errors,
            format!("{label} missing or unreadable: {}", path.display()),
        );
        return;
    };

    for needle in needles {
        if !text.contains(needle) {
            fail(
                errors,
                format!("{label} missing `{needle}` in {}", path.display()),
            );
        }
    }
}
