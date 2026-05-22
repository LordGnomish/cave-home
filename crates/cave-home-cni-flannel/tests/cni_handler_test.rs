// SPDX-License-Identifier: Apache-2.0
//! CNI handler dispatch tests — port of `cni-plugin/flannel_test.go`.

use cave_home_cni_flannel::cni::{
    CniInvocation, CniRequest, CniResponse, dispatch,
};
use cave_home_cni_flannel::cni::handler::{CniCommand, CniError};
use cave_home_cni_flannel::cni::types::NetConf;

fn invocation(cmd: CniCommand) -> CniInvocation {
    CniInvocation {
        command: cmd,
        container_id: "ctr-1".into(),
        netns: "/var/run/netns/test".into(),
        ifname: "eth0".into(),
        args: String::new(),
        path: "/opt/cni/bin".into(),
    }
}

fn netconf() -> NetConf {
    NetConf {
        cni_version: "1.0.0".into(),
        name: "flannel".into(),
        plugin_type: "flannel".into(),
        subnet_file: "/run/flannel/subnet.env".into(),
        data_dir: "/var/lib/cni/flannel".into(),
        ipam: None,
        delegate: Some(serde_json::json!({"type":"bridge","name":"cbr0","isGateway":true})),
        ip_masq: None,
        mtu: None,
        runtime_config: None,
    }
}

const SUBNET_ENV: &str = "FLANNEL_NETWORK=10.244.0.0/16\n\
FLANNEL_SUBNET=10.244.5.1/24\n\
FLANNEL_MTU=1450\n\
FLANNEL_IPMASQ=true\n";

#[test]
fn version_returns_supported_versions() {
    let req = CniRequest {
        invocation: invocation(CniCommand::Version),
        conf: netconf(),
        subnet_env: None,
    };
    let resp = dispatch(&req).unwrap();
    let CniResponse::Version(v) = resp else { panic!("wrong variant") };
    assert!(v.supported_versions.contains(&"1.0.0".to_string()));
    assert!(v.supported_versions.contains(&"0.4.0".to_string()));
}

#[test]
fn add_with_subnet_env_emits_ip_and_route() {
    let req = CniRequest {
        invocation: invocation(CniCommand::Add),
        conf: netconf(),
        subnet_env: Some(SUBNET_ENV.into()),
    };
    let resp = dispatch(&req).unwrap();
    let CniResponse::Result(r) = resp else { panic!("wrong variant") };
    assert_eq!(r.ips.len(), 1);
    assert_eq!(r.routes.len(), 1);
    assert_eq!(r.routes[0].dst.to_string(), "10.244.0.0/16");
}

#[test]
fn add_without_subnet_env_errors() {
    let req = CniRequest {
        invocation: invocation(CniCommand::Add),
        conf: netconf(),
        subnet_env: None,
    };
    let err = dispatch(&req).unwrap_err();
    assert!(matches!(err, CniError::MissingSubnetEnv(_)));
}

#[test]
fn check_behaves_like_add() {
    let req = CniRequest {
        invocation: invocation(CniCommand::Check),
        conf: netconf(),
        subnet_env: Some(SUBNET_ENV.into()),
    };
    let resp = dispatch(&req).unwrap();
    assert!(matches!(resp, CniResponse::Result(_)));
}

#[test]
fn del_returns_empty() {
    let req = CniRequest {
        invocation: invocation(CniCommand::Del),
        conf: netconf(),
        subnet_env: None,
    };
    let resp = dispatch(&req).unwrap();
    assert!(matches!(resp, CniResponse::Empty {}));
}

#[test]
fn cni_command_parse_known_strings() {
    assert_eq!(CniCommand::parse("ADD").unwrap(), CniCommand::Add);
    assert_eq!(CniCommand::parse("DEL").unwrap(), CniCommand::Del);
    assert_eq!(CniCommand::parse("CHECK").unwrap(), CniCommand::Check);
    assert_eq!(CniCommand::parse("VERSION").unwrap(), CniCommand::Version);
}

#[test]
fn cni_command_parse_rejects_unknown() {
    let err = CniCommand::parse("FROBNICATE").unwrap_err();
    assert!(matches!(err, CniError::UnsupportedCommand(_)));
}

#[test]
fn add_uses_first_usable_as_gateway() {
    let req = CniRequest {
        invocation: invocation(CniCommand::Add),
        conf: netconf(),
        subnet_env: Some(SUBNET_ENV.into()),
    };
    let CniResponse::Result(r) = dispatch(&req).unwrap() else { panic!() };
    let gw = r.ips[0].gateway.expect("gateway");
    // Subnet 10.244.5.1/24 → network 10.244.5.0 → first usable 10.244.5.1.
    assert_eq!(gw.to_string(), "10.244.5.1");
}

#[test]
fn version_response_serialises_to_expected_json() {
    let req = CniRequest {
        invocation: invocation(CniCommand::Version),
        conf: netconf(),
        subnet_env: None,
    };
    let resp = dispatch(&req).unwrap();
    let s = serde_json::to_string(&resp).unwrap();
    assert!(s.contains("supportedVersions"));
    assert!(s.contains("cniVersion"));
}

#[test]
fn add_response_serialises_to_cni_result_shape() {
    let req = CniRequest {
        invocation: invocation(CniCommand::Add),
        conf: netconf(),
        subnet_env: Some(SUBNET_ENV.into()),
    };
    let resp = dispatch(&req).unwrap();
    let s = serde_json::to_string(&resp).unwrap();
    assert!(s.contains("\"ips\""));
    assert!(s.contains("\"routes\""));
    assert!(s.contains("cniVersion"));
}
