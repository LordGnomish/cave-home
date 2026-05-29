// SPDX-License-Identifier: Apache-2.0
//! Helm-job argument construction.
//!
//! helm-controller does not link the Helm SDK; it runs `helm` inside a
//! Kubernetes `Job` (the `klipper-helm` image), passing the operation and
//! flags as container args. This module is the **pure** construction of those
//! args from a [`HelmChartSpec`] and an [`Operation`]. The actual Job object,
//! its scheduling, and the in-cluster exec are deferred (Phase 1b).
//!
//! Spec sources (public, Apache-2.0-compatible documentation):
//! * Helm CLI reference — `helm install|upgrade|uninstall` flags
//!   (`--namespace`, `--version`, `--repo`, `--set`, `--create-namespace`).
//! * k3s-io/helm-controller public docs on the `klipper-helm` job contract.

use crate::chart::{HelmChartSpec, VersionPolicy};
use crate::values::Value;

/// The helm operation a job performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Install,
    Upgrade,
    Uninstall,
}

impl Operation {
    /// The helm sub-command verb.
    #[must_use]
    pub const fn verb(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Upgrade => "upgrade",
            Self::Uninstall => "uninstall",
        }
    }
}

/// Flatten a value tree into helm `--set` `key.path=value` leaves, sorted.
///
/// Mirrors helm's dotted-path `--set` syntax. Objects recurse; arrays use the
/// `key[i]` index form; scalars become leaves. Used only for set-style values;
/// inline `valuesContent` is passed as a file by the real job (Phase 1b) — here
/// we surface the explicit `set` map deterministically.
fn flatten_set(prefix: &str, value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let next = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten_set(&next, v, out);
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                let next = format!("{prefix}[{i}]");
                flatten_set(&next, v, out);
            }
        }
        Value::Null => out.push(format!("{prefix}=null")),
        Value::Bool(b) => out.push(format!("{prefix}={b}")),
        Value::Number(n) => out.push(format!("{prefix}={n}")),
        Value::String(s) => out.push(format!("{prefix}={s}")),
    }
}

/// Build the ordered helm container args for the given operation on this spec.
///
/// Deterministic: the same spec+operation always yields the same arg vector.
#[must_use]
pub fn build_args(spec: &HelmChartSpec, op: Operation) -> Vec<String> {
    let mut args: Vec<String> = vec![op.verb().to_string()];
    // Release name = chart name (helm-controller convention for its jobs).
    args.push(spec.chart.clone());

    match op {
        Operation::Uninstall => {
            args.push("--namespace".into());
            args.push(spec.target_namespace.clone());
            return args;
        }
        Operation::Install | Operation::Upgrade => {
            // Chart reference comes after the release name for install/upgrade.
            args.push(spec.chart.clone());
            args.push("--namespace".into());
            args.push(spec.target_namespace.clone());
            args.push("--create-namespace".into());

            if op == Operation::Upgrade {
                // Allow upgrade to create if missing — helm-controller's job
                // uses `upgrade --install` idempotently; we mark it explicitly.
                args.push("--install".into());
            }

            if let Some(repo) = &spec.repo {
                if !repo.trim().is_empty() {
                    args.push("--repo".into());
                    args.push(repo.clone());
                }
            }

            match &spec.version {
                VersionPolicy::Pinned(v) => {
                    args.push("--version".into());
                    args.push(v.clone());
                }
                // Charter §7 always-latest: omit --version so helm resolves the
                // newest available chart.
                VersionPolicy::Latest => {}
            }

            // Explicit --set overrides, sorted for determinism.
            if let Some(set) = spec.set_as_value() {
                let mut leaves = Vec::new();
                flatten_set("", &set, &mut leaves);
                leaves.sort();
                for leaf in leaves {
                    args.push("--set".into());
                    args.push(leaf);
                }
            }
        }
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::HelmChartSpec;
    use std::collections::BTreeMap;

    fn spec() -> HelmChartSpec {
        HelmChartSpec {
            chart: "traefik".into(),
            repo: Some("https://helm.traefik.io/traefik".into()),
            version: VersionPolicy::Pinned("1.2.3".into()),
            target_namespace: "kube-system".into(),
            values_content: None,
            set: BTreeMap::new(),
            bootstrap: false,
            job_image: "rancher/klipper-helm:v0.8.0".into(),
        }
    }

    #[test]
    fn install_contains_namespace_and_create() {
        let a = build_args(&spec(), Operation::Install);
        assert_eq!(a[0], "install");
        assert!(a.windows(2).any(|w| w == ["--namespace", "kube-system"]));
        assert!(a.contains(&"--create-namespace".to_string()));
    }

    #[test]
    fn pinned_version_emits_version_flag() {
        let a = build_args(&spec(), Operation::Install);
        assert!(a.windows(2).any(|w| w == ["--version", "1.2.3"]));
    }

    #[test]
    fn latest_omits_version_flag() {
        let mut s = spec();
        s.version = VersionPolicy::Latest;
        let a = build_args(&s, Operation::Install);
        assert!(!a.contains(&"--version".to_string()));
    }

    #[test]
    fn upgrade_is_idempotent_install() {
        let a = build_args(&spec(), Operation::Upgrade);
        assert_eq!(a[0], "upgrade");
        assert!(a.contains(&"--install".to_string()));
    }

    #[test]
    fn uninstall_is_minimal() {
        let a = build_args(&spec(), Operation::Uninstall);
        assert_eq!(a[0], "uninstall");
        assert!(a.windows(2).any(|w| w == ["--namespace", "kube-system"]));
        assert!(!a.contains(&"--repo".to_string()));
        assert!(!a.contains(&"--version".to_string()));
    }

    #[test]
    fn repo_flag_present_when_set() {
        let a = build_args(&spec(), Operation::Install);
        assert!(a
            .windows(2)
            .any(|w| w == ["--repo", "https://helm.traefik.io/traefik"]));
    }

    #[test]
    fn empty_repo_omits_repo_flag() {
        let mut s = spec();
        s.repo = None;
        let a = build_args(&s, Operation::Install);
        assert!(!a.contains(&"--repo".to_string()));
    }

    #[test]
    fn set_values_are_flattened_sorted_and_typed() {
        let mut s = spec();
        let mut set = BTreeMap::new();
        set.insert(
            "service".into(),
            Value::object().with("type", Value::String("LoadBalancer".into())),
        );
        set.insert("replicas".into(), Value::Number("3".into()));
        set.insert("enabled".into(), Value::Bool(true));
        s.set = set;
        let a = build_args(&s, Operation::Install);
        // Collect the --set leaf values.
        let leaves: Vec<&String> = a
            .iter()
            .enumerate()
            .filter(|(i, _)| *i > 0 && a[i - 1] == "--set")
            .map(|(_, v)| v)
            .collect();
        assert!(leaves.contains(&&"replicas=3".to_string()));
        assert!(leaves.contains(&&"enabled=true".to_string()));
        assert!(leaves.contains(&&"service.type=LoadBalancer".to_string()));
        // Sorted: 'enabled' < 'replicas' < 'service.type'.
        assert_eq!(leaves[0], "enabled=true");
    }

    #[test]
    fn arg_build_is_deterministic() {
        assert_eq!(
            build_args(&spec(), Operation::Install),
            build_args(&spec(), Operation::Install)
        );
    }

    #[test]
    fn array_set_uses_index_form() {
        let mut s = spec();
        let mut set = BTreeMap::new();
        set.insert(
            "args".into(),
            Value::Array(vec![Value::String("a".into()), Value::String("b".into())]),
        );
        s.set = set;
        let a = build_args(&s, Operation::Install);
        assert!(a.contains(&"args[0]=a".to_string()));
        assert!(a.contains(&"args[1]=b".to_string()));
    }
}
