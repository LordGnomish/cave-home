// SPDX-License-Identifier: Apache-2.0
//! The local node's self-registration object.
//!
//! In K3s the kubelet registers its own `Node` object with the apiserver on
//! start-up (`pkg/agent` → `kubelet` `--register-node`). Our `kubelet-rs`
//! decision core does not yet produce that object (node-status is deferred), so
//! the unified binary builds the `Node` here and seeds it into the in-process
//! [`Registry`](cave_home_apiserver_rs::registry::Registry) at boot. That is
//! what makes `cavehomectl get nodes` return the running host.
//!
//! This module only *builds* the object (a pure `Value` tree); the seeding and
//! the periodic Ready heartbeat live in [`crate::server`].

use cave_home_apiserver_rs::json::{self, Value};

/// Identity + reachable address of the host this binary runs on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalNode {
    /// The node name (registered as `metadata.name`).
    pub name: String,
    /// The node's primary internal IP address.
    pub internal_ip: String,
}

impl LocalNode {
    /// Construct from a name and internal IP.
    #[must_use]
    pub fn new(name: impl Into<String>, internal_ip: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            internal_ip: internal_ip.into(),
        }
    }

    /// Build the core/v1 `Node` object to register with the apiserver.
    ///
    /// The apiserver assigns `uid`/`resourceVersion`/`generation` on create;
    /// we supply identity (`metadata.name`), reachable `status.addresses`, and a
    /// `Ready=True` condition so schedulers and `kubectl get nodes` see a
    /// healthy node.
    #[must_use]
    pub fn to_object(&self) -> Value {
        let addresses = Value::Array(vec![
            json::obj([
                ("type", Value::from("InternalIP")),
                ("address", Value::from(self.internal_ip.as_str())),
            ]),
            json::obj([
                ("type", Value::from("Hostname")),
                ("address", Value::from(self.name.as_str())),
            ]),
        ]);
        let conditions = Value::Array(vec![json::obj([
            ("type", Value::from("Ready")),
            ("status", Value::from("True")),
            ("reason", Value::from("KubeletReady")),
            ("message", Value::from("cave-home node is ready")),
        ])]);
        json::obj([
            ("apiVersion", Value::from("v1")),
            ("kind", Value::from("Node")),
            (
                "metadata",
                json::obj([("name", Value::from(self.name.as_str()))]),
            ),
            ("spec", Value::object()),
            (
                "status",
                json::obj([("addresses", addresses), ("conditions", conditions)]),
            ),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cave_home_apiserver_rs::gvk::GroupVersionResource;
    use cave_home_apiserver_rs::registry::{ListOptions, Registry};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn node() -> Value {
        LocalNode::new("hub-01", "10.0.0.5").to_object()
    }

    #[test]
    fn has_kind_and_api_version() {
        let n = node();
        assert_eq!(n.pointer("kind").and_then(Value::as_str), Some("Node"));
        assert_eq!(n.pointer("apiVersion").and_then(Value::as_str), Some("v1"));
    }

    #[test]
    fn metadata_name_is_the_node_name() {
        let n = node();
        assert_eq!(
            n.pointer("metadata.name").and_then(Value::as_str),
            Some("hub-01")
        );
    }

    #[test]
    fn reports_ready_condition() -> TestResult {
        let n = node();
        let conditions = n
            .pointer("status.conditions")
            .and_then(Value::as_array)
            .ok_or("status.conditions is not an array")?;
        let ready = conditions
            .iter()
            .find(|c| c.pointer("type").and_then(Value::as_str) == Some("Ready"))
            .ok_or("no Ready condition")?;
        assert_eq!(
            ready.pointer("status").and_then(Value::as_str),
            Some("True")
        );
        Ok(())
    }

    #[test]
    fn advertises_internal_ip_and_hostname() -> TestResult {
        let n = node();
        let addrs = n
            .pointer("status.addresses")
            .and_then(Value::as_array)
            .ok_or("status.addresses is not an array")?;
        let internal = addrs
            .iter()
            .find(|a| a.pointer("type").and_then(Value::as_str) == Some("InternalIP"))
            .ok_or("no InternalIP address")?;
        assert_eq!(
            internal.pointer("address").and_then(Value::as_str),
            Some("10.0.0.5")
        );
        assert!(
            addrs
                .iter()
                .any(|a| a.pointer("type").and_then(Value::as_str) == Some("Hostname"))
        );
        Ok(())
    }

    #[test]
    fn registers_into_the_apiserver_registry() -> TestResult {
        // The object must survive an actual apiserver create+list round-trip.
        let nodes = GroupVersionResource::new("", "v1", "nodes");
        let mut reg = Registry::new();
        reg.create(&nodes, node())?;
        let list = reg.list(&nodes, &ListOptions::default())?;
        assert_eq!(list.items.len(), 1);
        assert_eq!(
            list.items[0]
                .pointer("metadata.name")
                .and_then(Value::as_str),
            Some("hub-01")
        );
        Ok(())
    }
}
