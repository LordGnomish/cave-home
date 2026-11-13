// SPDX-License-Identifier: Apache-2.0
//! End-to-end proxier tests using `MockEventSource` + `MockExecutor`.
//! Mirrors upstream `pkg/proxy/iptables/proxier_test.go` style: feed events,
//! advance time, assert iptables-restore was called with the expected rules.

use std::sync::Arc;
use std::time::Duration;

use cave_home_kube_proxy_rs::api::{
    Endpoint, EndpointConditions, EndpointPort, EndpointSlice, NamespacedName, Protocol, Service,
    ServicePort, ServiceType, WatchEvent,
};
use cave_home_kube_proxy_rs::cache::source::MockEventSource;
use cave_home_kube_proxy_rs::iptables::executor::MockExecutor;
use cave_home_kube_proxy_rs::proxier::proxier::{Proxier, ProxierConfig};
use cave_home_kube_proxy_rs::proxier::reconciler::BoundedFrequencyConfig;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn svc1() -> Service {
    Service {
        metadata: NamespacedName::new("ns1", "svc1"),
        cluster_ip: "10.20.30.41".into(),
        ports: vec![ServicePort {
            name: "p80".into(),
            port: 80,
            protocol: Protocol::Tcp,
        }],
        type_: ServiceType::ClusterIP,
    }
}

fn slice1() -> EndpointSlice {
    EndpointSlice {
        metadata: NamespacedName::new("ns1", "svc1-abc"),
        service_name: "svc1".into(),
        ports: vec![EndpointPort {
            name: "p80".into(),
            port: 80,
            protocol: Protocol::Tcp,
        }],
        endpoints: vec![Endpoint {
            addresses: vec!["10.180.0.1".into()],
            conditions: EndpointConditions {
                ready: Some(true),
                serving: None,
                terminating: None,
            },
        }],
    }
}

fn cfg() -> ProxierConfig {
    ProxierConfig {
        cluster_cidr: Some("10.0.0.0/24".into()),
        frequency: BoundedFrequencyConfig {
            min_interval: Duration::from_millis(0),
            debounce: Duration::from_millis(0),
            resync_period: Duration::from_secs(3600),
        },
    }
}

#[tokio::test]
async fn one_shot_sync_writes_expected_rules_to_executor() -> TestResult {
    let src = MockEventSource::new();
    src.push(WatchEvent::ServiceAdded(svc1()));
    src.push(WatchEvent::EndpointSliceAdded(slice1()));
    let exec = Arc::new(MockExecutor::new());
    let proxier = Proxier::new(cfg(), &src, exec.clone());

    proxier.sync_once().await?;
    let inputs = exec.recorded_inputs();
    assert!(!inputs.is_empty(), "executor should have been called");
    let rules = &inputs[0];
    assert!(rules.contains("KUBE-SVC-XPGD46QRK7WJZT7O"), "got:\n{rules}");
    assert!(rules.contains("KUBE-SEP-SXIVWICOYRO3J4NJ"), "got:\n{rules}");
    assert!(
        rules.contains("--to-destination 10.180.0.1:80"),
        "got:\n{rules}"
    );
    Ok(())
}

#[tokio::test]
async fn sync_with_no_services_emits_skeleton_only() -> TestResult {
    let src = MockEventSource::new();
    let exec = Arc::new(MockExecutor::new());
    let proxier = Proxier::new(cfg(), &src, exec.clone());

    proxier.sync_once().await?;
    let rules = &exec.recorded_inputs()[0];
    assert!(rules.contains(":KUBE-SERVICES"));
    assert!(rules.contains("KUBE-MARK-MASQ"));
    // No KUBE-SVC- chain decl when no services.
    assert!(!rules.contains(":KUBE-SVC-"));
    Ok(())
}

#[tokio::test]
async fn sync_propagates_executor_errors() -> TestResult {
    let src = MockEventSource::new();
    let exec = Arc::new(MockExecutor::new());
    exec.set_next_error(
        cave_home_kube_proxy_rs::iptables::errors::ProxierError::IptablesRestoreFailed {
            exit_code: Some(2),
            stderr: "rule mismatch".into(),
        },
    );
    let proxier = Proxier::new(cfg(), &src, exec.clone());

    let err = proxier.sync_once().await.err().ok_or("must fail")?;
    assert!(matches!(
        err,
        cave_home_kube_proxy_rs::iptables::errors::ProxierError::IptablesRestoreFailed { .. }
    ));
    Ok(())
}

#[tokio::test]
async fn sync_after_service_delete_removes_service_chain() -> TestResult {
    let src = MockEventSource::new();
    src.push(WatchEvent::ServiceAdded(svc1()));
    src.push(WatchEvent::EndpointSliceAdded(slice1()));
    let exec = Arc::new(MockExecutor::new());
    let proxier = Proxier::new(cfg(), &src, exec.clone());

    proxier.sync_once().await?;
    src.push(WatchEvent::ServiceDeleted(svc1()));
    proxier.sync_once().await?;

    let inputs = exec.recorded_inputs();
    assert_ne!(inputs[0], "");
    assert!(
        !inputs[1].contains("KUBE-SVC-XPGD46QRK7WJZT7O"),
        "second sync (after delete) should not declare svc1 chain:\n{}",
        inputs[1]
    );
    Ok(())
}

#[tokio::test]
async fn sync_is_deterministic_between_runs() -> TestResult {
    let src1 = MockEventSource::new();
    src1.push(WatchEvent::ServiceAdded(svc1()));
    src1.push(WatchEvent::EndpointSliceAdded(slice1()));
    let exec1 = Arc::new(MockExecutor::new());
    Proxier::new(cfg(), &src1, exec1.clone())
        .sync_once()
        .await?;

    let src2 = MockEventSource::new();
    src2.push(WatchEvent::ServiceAdded(svc1()));
    src2.push(WatchEvent::EndpointSliceAdded(slice1()));
    let exec2 = Arc::new(MockExecutor::new());
    Proxier::new(cfg(), &src2, exec2.clone())
        .sync_once()
        .await?;

    assert_eq!(exec1.recorded_inputs()[0], exec2.recorded_inputs()[0]);
    Ok(())
}

#[tokio::test]
async fn sync_excludes_services_with_clusterip_none() -> TestResult {
    let src = MockEventSource::new();
    let mut headless = svc1();
    headless.metadata.name = "headless".into();
    headless.cluster_ip = "None".into();
    src.push(WatchEvent::ServiceAdded(headless));
    src.push(WatchEvent::ServiceAdded(svc1()));
    src.push(WatchEvent::EndpointSliceAdded(slice1()));
    let exec = Arc::new(MockExecutor::new());
    let proxier = Proxier::new(cfg(), &src, exec.clone());

    proxier.sync_once().await?;
    let rules = &exec.recorded_inputs()[0];
    assert!(rules.contains("KUBE-SVC-XPGD46QRK7WJZT7O"));
    assert!(!rules.contains("ns1/headless"));
    Ok(())
}

#[tokio::test]
async fn reconciler_run_loop_processes_initial_events_and_calls_executor() -> TestResult {
    let src = MockEventSource::new();
    src.push(WatchEvent::ServiceAdded(svc1()));
    src.push(WatchEvent::EndpointSliceAdded(slice1()));
    src.close();
    let exec = Arc::new(MockExecutor::new());
    let proxier = Proxier::new(cfg(), &src, exec.clone());

    // Run the loop with a short deadline — once the source is closed and
    // the deadline expires, the loop returns Ok(()).
    let h = tokio::spawn({
        let p = proxier.clone();
        async move { p.run_until(Duration::from_millis(150)).await }
    });
    h.await??;

    let inputs = exec.recorded_inputs();
    assert!(!inputs.is_empty(), "expected at least one sync");
    assert!(
        inputs
            .last()
            .ok_or("expected at least one sync")?
            .contains("KUBE-SVC-XPGD46QRK7WJZT7O")
    );
    Ok(())
}

#[tokio::test]
async fn reconciler_periodic_resync_fires_even_without_events() -> TestResult {
    let src = MockEventSource::new();
    src.push(WatchEvent::ServiceAdded(svc1()));
    src.push(WatchEvent::EndpointSliceAdded(slice1()));
    src.close();
    let exec = Arc::new(MockExecutor::new());

    let mut c = cfg();
    c.frequency.resync_period = Duration::from_millis(40);
    let proxier = Proxier::new(c, &src, exec.clone());

    proxier.run_until(Duration::from_millis(200)).await?;
    // We expect ≥ 2 syncs over 200ms with a 40ms resync interval —
    // initial event-driven sync + at least one periodic resync.
    assert!(
        exec.recorded_inputs().len() >= 2,
        "expected ≥2 syncs, got {}",
        exec.recorded_inputs().len()
    );
    Ok(())
}
