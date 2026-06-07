// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Observability — the Prometheus metrics for the Tesla energy adapter.
//!
//! [`Metrics`] is a tiny in-process registry: the household's live power
//! gauges, plus per-endpoint request-duration and error counters. It renders to
//! the Prometheus text exposition format directly (no client library), matching
//! the convention used elsewhere in cave-home.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PowerFlowData;

    fn flow() -> PowerFlowData {
        PowerFlowData {
            pv_watts: 3200.0,
            battery_watts: -800.0,
            load_watts: 1500.0,
            grid_watts: -900.0, // exporting 900 W
            soc_percent: 82.0,
        }
    }

    #[test]
    fn power_flow_sets_the_gauges() {
        let m = Metrics::new();
        m.record_power_flow(&flow());
        let out = m.render();
        assert!(out.contains("tesla_pv_power_watts 3200"));
        assert!(out.contains("tesla_battery_soc_percent 82"));
        assert!(out.contains("tesla_grid_export_watts 900"));
        assert!(out.contains("tesla_grid_import_watts 0"));
    }

    #[test]
    fn gauges_have_help_and_type_headers() {
        let m = Metrics::new();
        let out = m.render();
        for name in [
            "tesla_pv_power_watts",
            "tesla_battery_soc_percent",
            "tesla_grid_import_watts",
            "tesla_grid_export_watts",
        ] {
            assert!(out.contains(&format!("# HELP {name} ")), "missing HELP {name}");
            assert!(out.contains(&format!("# TYPE {name} gauge")), "missing TYPE {name}");
        }
    }

    #[test]
    fn request_duration_emits_count_and_sum_per_endpoint() {
        let m = Metrics::new();
        m.record_request("live_status", 0.25);
        m.record_request("live_status", 0.75);
        let out = m.render();
        assert!(out.contains("# TYPE tesla_api_request_duration_seconds summary"));
        assert!(out.contains(r#"tesla_api_request_duration_seconds_count{endpoint="live_status"} 2"#));
        assert!(out.contains(r#"tesla_api_request_duration_seconds_sum{endpoint="live_status"} 1"#));
    }

    #[test]
    fn errors_counter_is_labelled_by_endpoint_and_status() {
        let m = Metrics::new();
        m.record_error("live_status", 500);
        m.record_error("live_status", 500);
        m.record_error("backup", 429);
        let out = m.render();
        assert!(out.contains("# TYPE tesla_api_errors_total counter"));
        assert!(out
            .contains(r#"tesla_api_errors_total{endpoint="live_status",status="500"} 2"#));
        assert!(out.contains(r#"tesla_api_errors_total{endpoint="backup",status="429"} 1"#));
    }

    #[test]
    fn exposition_ends_with_newline() {
        let m = Metrics::new();
        assert!(m.render().ends_with('\n'));
    }
}
