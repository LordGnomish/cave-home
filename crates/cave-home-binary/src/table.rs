// SPDX-License-Identifier: Apache-2.0
//! Server-side `Table` printing — what makes `kubectl get pods` show the
//! familiar `NAME READY STATUS RESTARTS AGE` columns instead of a generic
//! two-column fallback.
//!
//! kubectl asks for a table by sending `Accept: application/json;as=Table;...`.
//! When [`wants_table`] sees that, the transport renders the result set as a
//! `meta.k8s.io/v1` [Table](https://kubernetes.io/docs/reference/using-api/api-concepts/#receiving-resources-as-tables):
//! per-kind column definitions plus one row of pre-computed cells per object
//! (with the full object attached for `-o wide`/`-o yaml`). Behavioural
//! reference: the apiserver's `TableConvertor` for the built-in kinds.
//!
//! Also home to the wall-clock helpers ([`now_rfc3339`]) the transport uses to
//! stamp `metadata.creationTimestamp` on create, so the Age column is real.

// The civil-date conversions are Howard Hinnant's well-known integer algorithms;
// their casts are provably in-range for the timestamps an apiserver handles, and
// the single-letter names (y/m/d/era/doe/…) match the published reference, so we
// keep them rather than obscuring the math.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::many_single_char_names
)]

use std::time::{SystemTime, UNIX_EPOCH};

use cave_home_apiserver_rs::gvk::GroupVersionResource;
use cave_home_apiserver_rs::json::{obj, Value};

/// True if the client's `Accept` header requests the `Table` representation.
#[must_use]
pub fn wants_table(accept: Option<&str>) -> bool {
    accept.is_some_and(|a| a.contains("as=Table"))
}

/// The current wall-clock time as a UTC RFC 3339 timestamp (`...Z`).
#[must_use]
pub fn now_rfc3339() -> String {
    rfc3339_from_epoch(now_epoch())
}

/// Seconds since the Unix epoch (0 if the clock is before 1970, which it is not).
#[must_use]
pub fn now_epoch() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Render a Unix timestamp as `YYYY-MM-DDTHH:MM:SSZ`.
fn rfc3339_from_epoch(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    let (h, mi, s) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Civil date from a count of days since the Unix epoch (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (y + i64::from(m <= 2), m, d)
}

/// Days since the Unix epoch for a civil date (Hinnant's `days_from_civil`).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = i64::from(m);
    let d = i64::from(d);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Parse an RFC 3339 `YYYY-MM-DDTHH:MM:SSZ` timestamp back to a Unix timestamp.
fn epoch_from_rfc3339(s: &str) -> Option<u64> {
    // Tolerant of the exact `...Z` form we emit; rejects anything else.
    let bytes = s.as_bytes();
    if bytes.len() < 20 || bytes[4] != b'-' || bytes[10] != b'T' || !s.ends_with('Z') {
        return None;
    }
    let y: i64 = s.get(0..4)?.parse().ok()?;
    let mo: u32 = s.get(5..7)?.parse().ok()?;
    let d: u32 = s.get(8..10)?.parse().ok()?;
    let h: u64 = s.get(11..13)?.parse().ok()?;
    let mi: u64 = s.get(14..16)?.parse().ok()?;
    let se: u64 = s.get(17..19)?.parse().ok()?;
    let days = days_from_civil(y, mo, d);
    let secs = days * 86_400 + (h * 3600 + mi * 60 + se) as i64;
    u64::try_from(secs).ok()
}

/// kubectl's compact age format: `5s`, `3m`, `2h`, `4d`, `1d3h` for the recent
/// past, falling back to `<unknown>` when there is no creation timestamp.
#[must_use]
pub fn age_string(creation: Option<&str>, now: u64) -> String {
    let Some(then) = creation.and_then(epoch_from_rfc3339) else {
        return "<unknown>".to_string();
    };
    let secs = now.saturating_sub(then);
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        let (h, m) = (secs / 3600, (secs % 3600) / 60);
        if m == 0 { format!("{h}h") } else { format!("{h}h{m}m") }
    } else {
        let (d, h) = (secs / 86_400, (secs % 86_400) / 3600);
        if h == 0 { format!("{d}d") } else { format!("{d}d{h}h") }
    }
}

/// Render a result set as a `meta.k8s.io/v1` Table for the given resource.
#[must_use]
pub fn to_table(gvr: &GroupVersionResource, items: &[Value], now: u64) -> Value {
    let (columns, rows): (Vec<&str>, Vec<Value>) = match (gvr.group.as_str(), gvr.resource.as_str()) {
        ("", "pods") => (
            vec!["Name", "Ready", "Status", "Restarts", "Age"],
            items.iter().map(|p| row(pod_cells(p, now), p)).collect(),
        ),
        ("", "nodes") => (
            vec!["Name", "Status", "Roles", "Age", "Version"],
            items.iter().map(|n| row(node_cells(n, now), n)).collect(),
        ),
        ("", "namespaces") => (
            vec!["Name", "Status", "Age"],
            items.iter().map(|n| row(namespace_cells(n, now), n)).collect(),
        ),
        _ => (
            vec!["Name", "Age"],
            items.iter().map(|o| row(vec![name(o), age(o, now)], o)).collect(),
        ),
    };
    let column_definitions: Vec<Value> = columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            obj([
                ("name", Value::from(*c)),
                ("type", Value::from("string")),
                ("format", Value::from(if i == 0 { "name" } else { "" })),
                ("priority", Value::from(0_i64)),
            ])
        })
        .collect();
    obj([
        ("kind", Value::from("Table")),
        ("apiVersion", Value::from("meta.k8s.io/v1")),
        ("columnDefinitions", Value::Array(column_definitions)),
        ("rows", Value::Array(rows)),
    ])
}

/// One Table row: the pre-computed cells plus the full object.
fn row(cells: Vec<String>, object: &Value) -> Value {
    obj([
        ("cells", Value::Array(cells.into_iter().map(Value::from).collect())),
        ("object", object.clone()),
    ])
}

fn name(o: &Value) -> String {
    o.pointer("metadata.name").and_then(Value::as_str).unwrap_or("").to_string()
}

fn age(o: &Value, now: u64) -> String {
    age_string(o.pointer("metadata.creationTimestamp").and_then(Value::as_str), now)
}

/// `NAME READY STATUS RESTARTS AGE` for a pod.
fn pod_cells(p: &Value, now: u64) -> Vec<String> {
    let total = p.pointer("spec.containers").and_then(Value::as_array).map_or(0, <[_]>::len);
    let statuses = p.pointer("status.containerStatuses").and_then(Value::as_array);
    let ready = statuses.map_or(0, |cs| cs.iter().filter(|c| c.get("ready").and_then(Value::as_bool) == Some(true)).count());
    let restarts: i64 = statuses.map_or(0, |cs| {
        cs.iter().filter_map(|c| c.get("restartCount").and_then(as_i64)).sum()
    });
    let status = p
        .pointer("status.phase")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("Pending")
        .to_string();
    vec![name(p), format!("{ready}/{total}"), status, restarts.to_string(), age(p, now)]
}

/// `NAME STATUS ROLES AGE VERSION` for a node.
fn node_cells(n: &Value, now: u64) -> Vec<String> {
    let ready = n
        .pointer("status.conditions")
        .and_then(Value::as_array)
        .is_some_and(|cs| {
            cs.iter().any(|c| {
                c.get("type").and_then(Value::as_str) == Some("Ready")
                    && c.get("status").and_then(Value::as_str) == Some("True")
            })
        });
    let status = if ready { "Ready" } else { "NotReady" }.to_string();
    let version = n
        .pointer("status.nodeInfo.kubeletVersion")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    vec![name(n), status, "control-plane".to_string(), age(n, now), version]
}

/// `NAME STATUS AGE` for a namespace.
fn namespace_cells(n: &Value, now: u64) -> Vec<String> {
    let status = n.pointer("status.phase").and_then(Value::as_str).unwrap_or("Active").to_string();
    vec![name(n), status, age(n, now)]
}

/// Read a JSON number as an `i64` (cells like restartCount).
const fn as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => Some(*n as i64),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_round_trips_through_epoch() {
        // A known instant: 2021-01-01T00:00:00Z = 1609459200.
        assert_eq!(rfc3339_from_epoch(1_609_459_200), "2021-01-01T00:00:00Z");
        assert_eq!(epoch_from_rfc3339("2021-01-01T00:00:00Z"), Some(1_609_459_200));
        let now = now_epoch();
        assert_eq!(epoch_from_rfc3339(&rfc3339_from_epoch(now)), Some(now));
    }

    #[test]
    fn age_is_compact_and_handles_missing() {
        let now = 1_000_000;
        assert_eq!(age_string(None, now), "<unknown>");
        let t = |secs| rfc3339_from_epoch(now - secs);
        assert_eq!(age_string(Some(&t(5)), now), "5s");
        assert_eq!(age_string(Some(&t(120)), now), "2m");
        assert_eq!(age_string(Some(&t(7200)), now), "2h");
        assert_eq!(age_string(Some(&t(90_000)), now), "1d1h");
    }

    #[test]
    fn wants_table_detects_the_accept_header() {
        assert!(wants_table(Some("application/json;as=Table;v=v1;g=meta.k8s.io,application/json")));
        assert!(!wants_table(Some("application/json")));
        assert!(!wants_table(None));
    }

    fn pod(name: &str, phase: &str, ready: bool) -> Value {
        obj([
            ("apiVersion", Value::from("v1")),
            ("kind", Value::from("Pod")),
            (
                "metadata",
                obj([("name", Value::from(name)), ("creationTimestamp", Value::from(rfc3339_from_epoch(100)))]),
            ),
            ("spec", obj([("containers", Value::Array(vec![obj([("name", Value::from("web"))])]))])),
            (
                "status",
                obj([
                    ("phase", Value::from(phase)),
                    (
                        "containerStatuses",
                        Value::Array(vec![obj([
                            ("ready", Value::from(ready)),
                            ("restartCount", Value::from(2_i64)),
                        ])]),
                    ),
                ]),
            ),
        ])
    }

    #[test]
    fn pod_table_has_kubectl_columns_and_cells() {
        let gvr = GroupVersionResource::new("", "v1", "pods");
        let table = to_table(&gvr, &[pod("nginx", "Running", true)], 130);
        let cols: Vec<&str> = table
            .pointer("columnDefinitions")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|c| c.get("name").and_then(Value::as_str).unwrap())
            .collect();
        assert_eq!(cols, ["Name", "Ready", "Status", "Restarts", "Age"]);
        let cells: Vec<&str> = table
            .pointer("rows")
            .and_then(Value::as_array)
            .unwrap()[0]
            .pointer("cells")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|c| c.as_str().unwrap())
            .collect();
        assert_eq!(cells, ["nginx", "1/1", "Running", "2", "30s"]);
    }

    #[test]
    fn pod_without_status_is_pending_zero_ready() {
        let gvr = GroupVersionResource::new("", "v1", "pods");
        let p = obj([
            ("metadata", obj([("name", Value::from("x"))])),
            ("spec", obj([("containers", Value::Array(vec![obj([("name", Value::from("c"))])]))])),
        ]);
        let table = to_table(&gvr, &[p], 0);
        let cells = table.pointer("rows").and_then(Value::as_array).unwrap()[0]
            .pointer("cells")
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(cells[1].as_str(), Some("0/1"));
        assert_eq!(cells[2].as_str(), Some("Pending"));
    }

    #[test]
    fn unknown_kind_falls_back_to_name_age() {
        let gvr = GroupVersionResource::new("apps", "v1", "deployments");
        let d = obj([("metadata", obj([("name", Value::from("site"))]))]);
        let table = to_table(&gvr, &[d], 0);
        let cols: Vec<&str> = table
            .pointer("columnDefinitions")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|c| c.get("name").and_then(Value::as_str).unwrap())
            .collect();
        assert_eq!(cols, ["Name", "Age"]);
    }
}
