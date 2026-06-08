// SPDX-License-Identifier: Apache-2.0
//! OCI runtime-spec (`config.json`) construction.
//!
//! Behavioural reimplementation of the documented OCI runtime-spec
//! `config.md` schema and containerd's `oci.GenerateSpec` defaults. This is
//! pure construction: given a [`ContainerConfig`], build the runtime
//! [`RuntimeSpec`] model (process, root, mounts, linux namespaces / cgroups /
//! resources). No runc invocation, no syscalls — that is the deferred
//! OCI-runtime exec layer (see `parity.manifest.toml`).
//!
//! Spec sources:
//!   * OCI runtime-spec `config.md` (process, root, mounts, hooks).
//!   * OCI runtime-spec `config-linux.md` (namespaces, cgroupsPath,
//!     resources.memory / resources.cpu, the default masked/readonly paths).
//!   * containerd `oci.GenerateSpec` documented defaults (the default
//!     mount set, the default namespace set, rootfs path `rootfs`).

use crate::oci::version::OCI_VERSION;

/// A Linux namespace kind that a container may enter or create.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NamespaceType {
    /// PID namespace.
    Pid,
    /// Network namespace.
    Network,
    /// Mount namespace.
    Mount,
    /// IPC namespace.
    Ipc,
    /// UTS (hostname) namespace.
    Uts,
    /// Cgroup namespace.
    Cgroup,
}

impl NamespaceType {
    /// The OCI runtime-spec namespace `type` string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pid => "pid",
            Self::Network => "network",
            Self::Mount => "mount",
            Self::Ipc => "ipc",
            Self::Uts => "uts",
            Self::Cgroup => "cgroup",
        }
    }
}

/// A Linux namespace declaration. A `None` path means "create a new
/// namespace"; `Some(path)` means "join the namespace at this path".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxNamespace {
    /// The namespace kind.
    pub kind: NamespaceType,
    /// The path to an existing namespace to join, if sharing one.
    pub path: Option<String>,
}

/// A filesystem mount in the runtime spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    /// Mount target inside the container.
    pub destination: String,
    /// Mount source on the host (or the fs type's pseudo-source).
    pub source: String,
    /// The mount filesystem type (e.g. `bind`, `proc`, `tmpfs`).
    pub mount_type: String,
    /// Mount options (`ro`, `nosuid`, `rbind`, …).
    pub options: Vec<String>,
}

/// The process resource limits, lowered to the OCI `linux.resources` model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Resources {
    /// Memory limit in bytes (`linux.resources.memory.limit`).
    pub memory_limit_bytes: Option<i64>,
    /// CPU shares / weight (`linux.resources.cpu.shares`).
    pub cpu_shares: Option<u64>,
    /// CPU quota in microseconds per period (`linux.resources.cpu.quota`).
    pub cpu_quota_us: Option<i64>,
    /// CPU period in microseconds (`linux.resources.cpu.period`).
    pub cpu_period_us: Option<u64>,
}

/// The container process specification (`process` block).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process {
    /// `process.args` — the command and its arguments.
    pub args: Vec<String>,
    /// `process.env` — `KEY=VALUE` environment entries.
    pub env: Vec<String>,
    /// `process.cwd` — the working directory (defaults to `/`).
    pub cwd: String,
    /// `process.user.uid`.
    pub uid: u32,
    /// `process.user.gid`.
    pub gid: u32,
    /// `process.terminal`.
    pub terminal: bool,
}

/// The root filesystem block (`root`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    /// `root.path` — the rootfs directory, relative to the bundle.
    pub path: String,
    /// `root.readonly`.
    pub readonly: bool,
}

/// The Linux-specific block (`linux`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxSpec {
    /// `linux.namespaces`.
    pub namespaces: Vec<LinuxNamespace>,
    /// `linux.cgroupsPath`.
    pub cgroups_path: String,
    /// `linux.resources`.
    pub resources: Resources,
    /// `linux.maskedPaths` — default kernel paths hidden from the container.
    pub masked_paths: Vec<String>,
    /// `linux.readonlyPaths` — default kernel paths mounted read-only.
    pub readonly_paths: Vec<String>,
}

/// A fully-constructed OCI runtime spec (`config.json` model).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSpec {
    /// `ociVersion`.
    pub oci_version: String,
    /// `hostname`.
    pub hostname: String,
    /// `process`.
    pub process: Process,
    /// `root`.
    pub root: Root,
    /// `mounts`.
    pub mounts: Vec<Mount>,
    /// `linux`.
    pub linux: LinuxSpec,
}

/// Errors raised while generating a runtime spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecError {
    /// The container config had no command/args and the image supplied none.
    NoEntrypoint,
    /// An environment entry was not in `KEY=VALUE` form.
    MalformedEnv(String),
}

impl std::fmt::Display for SpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoEntrypoint => f.write_str("container has no entrypoint/command"),
            Self::MalformedEnv(e) => write!(f, "malformed env entry (want KEY=VALUE): {e}"),
        }
    }
}

impl std::error::Error for SpecError {}

/// The user-supplied container configuration the spec is generated from.
///
/// This is the runtime-agnostic input the CRI layer hands to spec generation:
/// image entrypoint/cmd resolved, env, mounts, working dir, user, resources.
#[derive(Debug, Clone, Default)]
pub struct ContainerConfig {
    /// The container hostname.
    pub hostname: String,
    /// The command (entrypoint + args) to run.
    pub command: Vec<String>,
    /// Environment entries in `KEY=VALUE` form.
    pub env: Vec<String>,
    /// Bind/volume mounts requested by the workload.
    pub mounts: Vec<Mount>,
    /// Working directory; empty means `/`.
    pub working_dir: String,
    /// Effective UID.
    pub uid: u32,
    /// Effective GID.
    pub gid: u32,
    /// Whether the rootfs should be read-only.
    pub readonly_rootfs: bool,
    /// Resource limits.
    pub resources: Resources,
    /// If set, the network namespace path to join (sandbox netns); otherwise a
    /// fresh network namespace is created.
    pub network_ns_path: Option<String>,
}

/// The default mounts containerd's `oci.GenerateSpec` injects for every
/// Linux container.
fn default_mounts() -> Vec<Mount> {
    vec![
        Mount {
            destination: "/proc".to_owned(),
            source: "proc".to_owned(),
            mount_type: "proc".to_owned(),
            options: vec!["nosuid".to_owned(), "noexec".to_owned(), "nodev".to_owned()],
        },
        Mount {
            destination: "/dev".to_owned(),
            source: "tmpfs".to_owned(),
            mount_type: "tmpfs".to_owned(),
            options: vec![
                "nosuid".to_owned(),
                "strictatime".to_owned(),
                "mode=755".to_owned(),
                "size=65536k".to_owned(),
            ],
        },
        Mount {
            destination: "/sys".to_owned(),
            source: "sysfs".to_owned(),
            mount_type: "sysfs".to_owned(),
            options: vec![
                "nosuid".to_owned(),
                "noexec".to_owned(),
                "nodev".to_owned(),
                "ro".to_owned(),
            ],
        },
    ]
}

/// The default namespace set for an isolated container. The network namespace
/// is added separately (it may join the sandbox's netns).
fn default_namespaces(network_ns_path: Option<String>) -> Vec<LinuxNamespace> {
    let mut ns = vec![
        LinuxNamespace { kind: NamespaceType::Pid, path: None },
        LinuxNamespace { kind: NamespaceType::Ipc, path: None },
        LinuxNamespace { kind: NamespaceType::Uts, path: None },
        LinuxNamespace { kind: NamespaceType::Mount, path: None },
    ];
    ns.push(LinuxNamespace { kind: NamespaceType::Network, path: network_ns_path });
    ns
}

/// The default masked kernel paths (`config-linux.md` documented set).
fn default_masked_paths() -> Vec<String> {
    [
        "/proc/kcore",
        "/proc/keys",
        "/proc/latency_stats",
        "/proc/timer_list",
        "/proc/timer_stats",
        "/proc/sched_debug",
        "/sys/firmware",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

/// The default read-only kernel paths.
fn default_readonly_paths() -> Vec<String> {
    [
        "/proc/asound",
        "/proc/bus",
        "/proc/fs",
        "/proc/irq",
        "/proc/sys",
        "/proc/sysrq-trigger",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

/// Generates an OCI runtime spec from a container config.
///
/// `cgroup_parent` plus the container id form the `linux.cgroupsPath`
/// (`<parent>:cri-containerd:<id>` in containerd's slice form, collapsed to a
/// path here). User-supplied mounts are appended after the default set, which
/// matches the documented ordering.
///
/// # Errors
/// Returns [`SpecError::NoEntrypoint`] if `config.command` is empty, or
/// [`SpecError::MalformedEnv`] if any env entry is not `KEY=VALUE`.
pub fn generate_spec(
    container_id: &str,
    cgroup_parent: &str,
    config: &ContainerConfig,
) -> Result<RuntimeSpec, SpecError> {
    if config.command.is_empty() {
        return Err(SpecError::NoEntrypoint);
    }
    for entry in &config.env {
        if !entry.contains('=') || entry.starts_with('=') {
            return Err(SpecError::MalformedEnv(entry.clone()));
        }
    }

    let cwd = if config.working_dir.is_empty() {
        "/".to_owned()
    } else {
        config.working_dir.clone()
    };

    let process = Process {
        args: config.command.clone(),
        env: config.env.clone(),
        cwd,
        uid: config.uid,
        gid: config.gid,
        terminal: false,
    };

    let root = Root { path: "rootfs".to_owned(), readonly: config.readonly_rootfs };

    let mut mounts = default_mounts();
    mounts.extend(config.mounts.iter().cloned());

    let cgroups_path = if cgroup_parent.is_empty() {
        format!("/cri-containerd/{container_id}")
    } else {
        format!("{cgroup_parent}/{container_id}")
    };

    let linux = LinuxSpec {
        namespaces: default_namespaces(config.network_ns_path.clone()),
        cgroups_path,
        resources: config.resources,
        masked_paths: default_masked_paths(),
        readonly_paths: default_readonly_paths(),
    };

    Ok(RuntimeSpec {
        oci_version: OCI_VERSION.to_owned(),
        hostname: config.hostname.clone(),
        process,
        root,
        mounts,
        linux,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config() -> ContainerConfig {
        ContainerConfig {
            hostname: "box".to_owned(),
            command: vec!["/bin/sh".to_owned(), "-c".to_owned(), "echo hi".to_owned()],
            env: vec!["PATH=/usr/bin".to_owned()],
            working_dir: "/app".to_owned(),
            uid: 1000,
            gid: 1000,
            ..ContainerConfig::default()
        }
    }

    #[test]
    fn generates_process_and_root_fields() {
        let spec = generate_spec("c1", "/kubepods/podx", &base_config()).expect("spec");
        assert_eq!(spec.oci_version, OCI_VERSION);
        assert_eq!(spec.hostname, "box");
        assert_eq!(spec.process.args, vec!["/bin/sh", "-c", "echo hi"]);
        assert_eq!(spec.process.env, vec!["PATH=/usr/bin"]);
        assert_eq!(spec.process.cwd, "/app");
        assert_eq!(spec.process.uid, 1000);
        assert_eq!(spec.root.path, "rootfs");
        assert!(!spec.root.readonly);
    }

    #[test]
    fn empty_working_dir_defaults_to_root() {
        let mut cfg = base_config();
        cfg.working_dir = String::new();
        let spec = generate_spec("c1", "", &cfg).expect("spec");
        assert_eq!(spec.process.cwd, "/");
    }

    #[test]
    fn no_command_is_rejected() {
        let mut cfg = base_config();
        cfg.command.clear();
        assert_eq!(generate_spec("c1", "", &cfg), Err(SpecError::NoEntrypoint));
    }

    #[test]
    fn malformed_env_is_rejected() {
        let mut cfg = base_config();
        cfg.env = vec!["NOTANENV".to_owned()];
        assert!(matches!(generate_spec("c1", "", &cfg), Err(SpecError::MalformedEnv(_))));
        cfg.env = vec!["=novalue".to_owned()];
        assert!(matches!(generate_spec("c1", "", &cfg), Err(SpecError::MalformedEnv(_))));
    }

    #[test]
    fn default_mounts_precede_user_mounts() {
        let mut cfg = base_config();
        cfg.mounts = vec![Mount {
            destination: "/data".to_owned(),
            source: "/host/data".to_owned(),
            mount_type: "bind".to_owned(),
            options: vec!["rbind".to_owned(), "rw".to_owned()],
        }];
        let spec = generate_spec("c1", "", &cfg).expect("spec");
        assert_eq!(spec.mounts.len(), 4);
        assert_eq!(spec.mounts[0].destination, "/proc");
        assert_eq!(spec.mounts.last().expect("last").destination, "/data");
    }

    #[test]
    fn default_namespace_set_is_present() {
        let spec = generate_spec("c1", "", &base_config()).expect("spec");
        let kinds: Vec<_> = spec.linux.namespaces.iter().map(|n| n.kind).collect();
        assert!(kinds.contains(&NamespaceType::Pid));
        assert!(kinds.contains(&NamespaceType::Network));
        assert!(kinds.contains(&NamespaceType::Mount));
        // A fresh container creates its own netns when no sandbox path given.
        let net = spec
            .linux
            .namespaces
            .iter()
            .find(|n| n.kind == NamespaceType::Network)
            .expect("netns");
        assert!(net.path.is_none());
    }

    #[test]
    fn shared_network_namespace_path_is_joined() {
        let mut cfg = base_config();
        cfg.network_ns_path = Some("/var/run/netns/cni-abc".to_owned());
        let spec = generate_spec("c1", "", &cfg).expect("spec");
        let net = spec
            .linux
            .namespaces
            .iter()
            .find(|n| n.kind == NamespaceType::Network)
            .expect("netns");
        assert_eq!(net.path.as_deref(), Some("/var/run/netns/cni-abc"));
    }

    #[test]
    fn cgroups_path_uses_parent_and_id() {
        let spec = generate_spec("ctr9", "/kubepods/burstable", &base_config()).expect("spec");
        assert_eq!(spec.linux.cgroups_path, "/kubepods/burstable/ctr9");
        let spec2 = generate_spec("ctr9", "", &base_config()).expect("spec");
        assert_eq!(spec2.linux.cgroups_path, "/cri-containerd/ctr9");
    }

    #[test]
    fn resources_are_lowered() {
        let mut cfg = base_config();
        cfg.resources = Resources {
            memory_limit_bytes: Some(256 * 1024 * 1024),
            cpu_shares: Some(512),
            cpu_quota_us: Some(50_000),
            cpu_period_us: Some(100_000),
        };
        let spec = generate_spec("c1", "", &cfg).expect("spec");
        assert_eq!(spec.linux.resources.memory_limit_bytes, Some(256 * 1024 * 1024));
        assert_eq!(spec.linux.resources.cpu_shares, Some(512));
        assert_eq!(spec.linux.resources.cpu_quota_us, Some(50_000));
    }

    #[test]
    fn readonly_rootfs_is_propagated() {
        let mut cfg = base_config();
        cfg.readonly_rootfs = true;
        let spec = generate_spec("c1", "", &cfg).expect("spec");
        assert!(spec.root.readonly);
    }

    #[test]
    fn default_masked_and_readonly_paths_present() {
        let spec = generate_spec("c1", "", &base_config()).expect("spec");
        assert!(spec.linux.masked_paths.contains(&"/proc/kcore".to_owned()));
        assert!(spec.linux.readonly_paths.contains(&"/proc/sys".to_owned()));
    }
}
