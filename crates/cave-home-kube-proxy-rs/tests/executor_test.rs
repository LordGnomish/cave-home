// SPDX-License-Identifier: Apache-2.0
//! Tests for `IptablesExecutor` trait + `MockExecutor`. The Linux subprocess
//! executor (`LinuxExecutor`) is exercised via a smoke check that's gated
//! `#[cfg(target_os = "linux")]` and only verifies wiring (does NOT actually
//! mutate iptables — that requires root and is a Phase 1b CI concern).

use cave_home_kube_proxy_rs::iptables::executor::{IptablesExecutor, MockExecutor};
use cave_home_kube_proxy_rs::iptables::errors::ProxierError;

#[tokio::test]
async fn mock_executor_records_restore_input() {
    let mock = MockExecutor::new();
    mock.restore("*nat\n:KUBE-SERVICES - [0:0]\nCOMMIT\n").await.expect("ok");
    let inputs = mock.recorded_inputs();
    assert_eq!(inputs.len(), 1);
    assert!(inputs[0].contains("KUBE-SERVICES"));
}

#[tokio::test]
async fn mock_executor_records_multiple_calls() {
    let mock = MockExecutor::new();
    mock.restore("first").await.expect("ok");
    mock.restore("second").await.expect("ok");
    mock.restore("third").await.expect("ok");
    assert_eq!(mock.recorded_inputs().len(), 3);
    assert_eq!(mock.recorded_inputs()[1], "second");
}

#[tokio::test]
async fn mock_executor_can_be_primed_to_fail() {
    let mock = MockExecutor::new();
    mock.set_next_error(ProxierError::IptablesRestoreFailed {
        exit_code: Some(2),
        stderr: "boom".into(),
    });
    let err = mock.restore("anything").await.expect_err("must fail");
    assert!(matches!(err, ProxierError::IptablesRestoreFailed { .. }));
    // Subsequent calls succeed (primed error consumed once).
    mock.restore("ok").await.expect("ok");
}

#[tokio::test]
async fn mock_executor_clears_recorded_inputs() {
    let mock = MockExecutor::new();
    mock.restore("a").await.expect("ok");
    mock.clear();
    assert!(mock.recorded_inputs().is_empty());
}

// Linux subprocess smoke test — verifies the executor exists and constructs.
// We never actually run iptables-restore in CI (would require root + an
// available binary), but constructing the struct must not panic and the
// subprocess invocation path must compile.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn linux_executor_can_be_constructed() {
    use cave_home_kube_proxy_rs::iptables::executor::LinuxExecutor;
    let _exec = LinuxExecutor::new();
    // Successful construction is the assertion.
}

// On non-Linux platforms the trait method must return UnsupportedPlatform.
#[cfg(not(target_os = "linux"))]
#[tokio::test]
async fn non_linux_returns_unsupported_platform_error() {
    use cave_home_kube_proxy_rs::iptables::executor::LinuxExecutor;
    let exec = LinuxExecutor::new();
    let err = exec.restore("noop").await.expect_err("must fail off-Linux");
    assert!(matches!(err, ProxierError::UnsupportedPlatform));
}
