// SPDX-License-Identifier: Apache-2.0
//! Layered node configuration: defaults < config file < environment < flags.
//!
//! The cave-home node config is modelled as typed structs (no YAML crate is
//! required — a parsed file is represented as a [`ConfigLayer`] of optional
//! fields). The four layers are merged with **strict, documented precedence**,
//! then validated into a [`Config`]. The merge is pure and deterministic, so
//! the precedence is unit-tested directly.
//!
//! Precedence, lowest to highest:
//!
//! 1. [`ConfigLayer::defaults`] — the built-in defaults.
//! 2. the config-file layer.
//! 3. the environment layer.
//! 4. the command-line-flag layer.
//!
//! A field set in a higher layer overrides the same field in every lower layer.
//! A field left `None` in a layer is transparent — it does not clear a value a
//! lower layer set.

use crate::Component;
use std::collections::BTreeSet;
use std::fmt;

/// The role a node plays in the cluster (Charter §5 deployment topology).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeRole {
    /// Primary hub: broker, radios, automation engine, Portal.
    Hub,
    /// Active-passive failover companion to the hub.
    Secondary,
    /// ML / GPU off-load node (e.g. camera inference).
    Ml,
}

impl NodeRole {
    /// Parse a role from its lowercase config token.
    #[must_use]
    pub fn from_str_ci(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "hub" => Some(Self::Hub),
            "secondary" => Some(Self::Secondary),
            "ml" => Some(Self::Ml),
            _ => None,
        }
    }

    /// The lowercase canonical token for this role.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hub => "hub",
            Self::Secondary => "secondary",
            Self::Ml => "ml",
        }
    }

    /// The pillars a fresh node of this role enables by default. The hub runs
    /// the whole household stack; secondary mirrors the automation core for
    /// failover; an ML node only off-loads camera inference.
    #[must_use]
    pub fn default_components(self) -> BTreeSet<Component> {
        match self {
            Self::Hub => Component::ALL.into_iter().collect(),
            Self::Secondary => [
                Component::Orchestration,
                Component::Broker,
                Component::Core,
            ]
            .into_iter()
            .collect(),
            Self::Ml => [Component::Orchestration, Component::Cameras]
                .into_iter()
                .collect(),
        }
    }
}

/// Severity of a log threshold, low to high.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    /// Parse a log level from its lowercase token.
    #[must_use]
    pub fn from_str_ci(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "error" => Some(Self::Error),
            "warn" | "warning" => Some(Self::Warn),
            "info" => Some(Self::Info),
            "debug" => Some(Self::Debug),
            "trace" => Some(Self::Trace),
            _ => None,
        }
    }

    /// The lowercase canonical token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}

/// One layer of configuration: every field is optional so the merge can tell
/// "set here" from "inherit from below".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigLayer {
    /// Where this layer came from (for diagnostics / `config show`).
    pub origin: Layer,
    pub node_name: Option<String>,
    pub role: Option<NodeRole>,
    /// Explicit component set. `None` means "inherit"; `Some(set)` replaces.
    pub components: Option<BTreeSet<Component>>,
    pub data_dir: Option<String>,
    pub bind_addr: Option<String>,
    pub bind_port: Option<u16>,
    pub log_level: Option<LogLevel>,
}

/// Which of the four precedence layers a [`ConfigLayer`] represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layer {
    #[default]
    Defaults,
    File,
    Env,
    Flags,
}

impl Layer {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Defaults => "defaults",
            Self::File => "config file",
            Self::Env => "environment",
            Self::Flags => "command line",
        }
    }
}

impl ConfigLayer {
    /// The built-in defaults layer. These hold when no file/env/flag overrides
    /// them. Role defaults to [`NodeRole::Hub`] (single-box is the common case);
    /// components are left `None` so they fall out of the role unless overridden.
    #[must_use]
    pub fn defaults() -> Self {
        Self {
            origin: Layer::Defaults,
            node_name: Some("cave-home".to_string()),
            role: Some(NodeRole::Hub),
            components: None,
            data_dir: Some("/var/lib/cave-home".to_string()),
            bind_addr: Some("0.0.0.0".to_string()),
            bind_port: Some(8123),
            log_level: Some(LogLevel::Info),
        }
    }

    /// An empty layer of the given origin (used to build file/env/flag layers).
    #[must_use]
    pub fn empty(origin: Layer) -> Self {
        Self {
            origin,
            ..Self::default()
        }
    }

    /// Overlay `higher` onto `self`: any field set in `higher` wins; an unset
    /// field in `higher` leaves `self` untouched. The result's `origin` is the
    /// higher layer's origin (it represents the merged-so-far top).
    #[must_use]
    fn overlay(mut self, higher: &Self) -> Self {
        if higher.node_name.is_some() {
            self.node_name.clone_from(&higher.node_name);
        }
        if higher.role.is_some() {
            self.role = higher.role;
        }
        if higher.components.is_some() {
            self.components.clone_from(&higher.components);
        }
        if higher.data_dir.is_some() {
            self.data_dir.clone_from(&higher.data_dir);
        }
        if higher.bind_addr.is_some() {
            self.bind_addr.clone_from(&higher.bind_addr);
        }
        if higher.bind_port.is_some() {
            self.bind_port = higher.bind_port;
        }
        if higher.log_level.is_some() {
            self.log_level = higher.log_level;
        }
        self.origin = higher.origin;
        self
    }
}

/// A fully-merged, validated node configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub node_name: String,
    pub role: NodeRole,
    pub components: BTreeSet<Component>,
    pub data_dir: String,
    pub bind_addr: String,
    pub bind_port: u16,
    pub log_level: LogLevel,
}

/// Why a configuration is invalid. No panics — every bad input is one of these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// A required field had no value in any layer (should not happen given the
    /// defaults layer, but modelled so merges that drop defaults are caught).
    Missing(&'static str),
    /// The node name was empty or had disallowed characters.
    BadNodeName(String),
    /// The data dir was empty or not absolute.
    BadDataDir(String),
    /// The bind address was empty.
    BadBindAddr(String),
    /// Port 0 is not a valid listen port.
    BadPort,
    /// An ML node that does not enable the camera pillar has nothing to do.
    MlNodeWithoutCameras,
    /// No components are enabled — the node would do nothing.
    NoComponents,
    /// A pillar was enabled without the orchestration layer that must host it
    /// (Charter §5: everything runs in-process under the orchestration layer).
    MissingOrchestration,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(field) => write!(f, "configuration is missing a value for `{field}`"),
            Self::BadNodeName(n) => write!(
                f,
                "the home's name `{n}` is not allowed — use letters, numbers and dashes"
            ),
            Self::BadDataDir(d) => {
                write!(f, "the data folder `{d}` must be a full path starting with /")
            }
            Self::BadBindAddr(a) => write!(f, "the network address `{a}` is empty"),
            Self::BadPort => write!(f, "the network port must be between 1 and 65535"),
            Self::MlNodeWithoutCameras => write!(
                f,
                "a camera-helper node must have the Cameras feature turned on, or it has nothing to do"
            ),
            Self::NoComponents => {
                write!(f, "no home features are turned on — this node would do nothing")
            }
            Self::MissingOrchestration => write!(
                f,
                "the home foundation must be turned on for the other features to run"
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    /// Merge the four layers (lowest precedence first) and validate the result.
    ///
    /// `layers` are applied left-to-right with later layers winning. The
    /// canonical call passes `[defaults, file, env, flags]`.
    ///
    /// # Errors
    /// Returns a [`ConfigError`] if the merged config fails validation.
    pub fn from_layers(layers: &[ConfigLayer]) -> Result<Self, ConfigError> {
        let mut acc = ConfigLayer::default();
        for layer in layers {
            acc = acc.overlay(layer);
        }
        Self::resolve(acc)
    }

    /// The conventional pipeline: built-in defaults, then file, then env, then
    /// flags.
    ///
    /// # Errors
    /// Returns a [`ConfigError`] if the merged config fails validation.
    pub fn resolve_standard(
        file: &ConfigLayer,
        env: &ConfigLayer,
        flags: &ConfigLayer,
    ) -> Result<Self, ConfigError> {
        Self::from_layers(&[ConfigLayer::defaults(), file.clone(), env.clone(), flags.clone()])
    }

    fn resolve(merged: ConfigLayer) -> Result<Self, ConfigError> {
        let node_name = merged.node_name.ok_or(ConfigError::Missing("node_name"))?;
        let role = merged.role.ok_or(ConfigError::Missing("role"))?;
        let data_dir = merged.data_dir.ok_or(ConfigError::Missing("data_dir"))?;
        let bind_addr = merged.bind_addr.ok_or(ConfigError::Missing("bind_addr"))?;
        let bind_port = merged.bind_port.ok_or(ConfigError::Missing("bind_port"))?;
        let log_level = merged.log_level.ok_or(ConfigError::Missing("log_level"))?;

        // Components fall out of the role unless explicitly overridden.
        let components = merged
            .components
            .unwrap_or_else(|| role.default_components());

        let cfg = Self {
            node_name,
            role,
            components,
            data_dir,
            bind_addr,
            bind_port,
            log_level,
        };
        cfg.validate()?;
        Ok(cfg)
    }

    /// Validate a resolved config. Pure; never panics.
    ///
    /// # Errors
    /// Returns the first [`ConfigError`] found.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let name = self.node_name.trim();
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(ConfigError::BadNodeName(self.node_name.clone()));
        }
        if self.data_dir.trim().is_empty() || !self.data_dir.starts_with('/') {
            return Err(ConfigError::BadDataDir(self.data_dir.clone()));
        }
        if self.bind_addr.trim().is_empty() {
            return Err(ConfigError::BadBindAddr(self.bind_addr.clone()));
        }
        if self.bind_port == 0 {
            return Err(ConfigError::BadPort);
        }
        if self.components.is_empty() {
            return Err(ConfigError::NoComponents);
        }
        // Charter §5: pillars run in-process under the orchestration layer, so
        // it must be present whenever any other pillar is enabled.
        let only_orchestration = self.components.len() == 1
            && self.components.contains(&Component::Orchestration);
        if !self.components.contains(&Component::Orchestration) && !only_orchestration {
            return Err(ConfigError::MissingOrchestration);
        }
        if self.role == NodeRole::Ml && !self.components.contains(&Component::Cameras) {
            return Err(ConfigError::MlNodeWithoutCameras);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags_layer() -> ConfigLayer {
        ConfigLayer::empty(Layer::Flags)
    }

    #[test]
    fn defaults_alone_resolve_to_a_valid_hub() {
        let cfg = Config::from_layers(&[ConfigLayer::defaults()]).expect("defaults valid");
        assert_eq!(cfg.node_name, "cave-home");
        assert_eq!(cfg.role, NodeRole::Hub);
        assert_eq!(cfg.bind_port, 8123);
        assert_eq!(cfg.log_level, LogLevel::Info);
        // Hub enables every pillar.
        assert_eq!(cfg.components, Component::ALL.into_iter().collect());
    }

    #[test]
    fn file_overrides_defaults() {
        let mut file = ConfigLayer::empty(Layer::File);
        file.node_name = Some("livingroom".to_string());
        file.bind_port = Some(9000);
        let cfg =
            Config::resolve_standard(&file, &ConfigLayer::empty(Layer::Env), &flags_layer())
                .expect("valid");
        assert_eq!(cfg.node_name, "livingroom");
        assert_eq!(cfg.bind_port, 9000);
        // Untouched fields fall back to defaults.
        assert_eq!(cfg.log_level, LogLevel::Info);
    }

    #[test]
    fn env_overrides_file() {
        let mut file = ConfigLayer::empty(Layer::File);
        file.bind_port = Some(9000);
        file.log_level = Some(LogLevel::Warn);
        let mut env = ConfigLayer::empty(Layer::Env);
        env.bind_port = Some(9100);
        let cfg = Config::resolve_standard(&file, &env, &flags_layer()).expect("valid");
        assert_eq!(cfg.bind_port, 9100, "env beats file");
        assert_eq!(cfg.log_level, LogLevel::Warn, "file value kept where env silent");
    }

    #[test]
    fn flags_override_everything() {
        let mut file = ConfigLayer::empty(Layer::File);
        file.bind_port = Some(9000);
        let mut env = ConfigLayer::empty(Layer::Env);
        env.bind_port = Some(9100);
        let mut flags = flags_layer();
        flags.bind_port = Some(9200);
        let cfg = Config::resolve_standard(&file, &env, &flags).expect("valid");
        assert_eq!(cfg.bind_port, 9200, "flags are the highest layer");
    }

    #[test]
    fn full_four_layer_precedence_chain() {
        // Each layer sets a distinct field plus a contested one; assert the
        // contested field tracks the highest layer that set it.
        let mut file = ConfigLayer::empty(Layer::File);
        file.node_name = Some("from-file".to_string());
        file.data_dir = Some("/data/file".to_string());
        file.bind_port = Some(1);

        let mut env = ConfigLayer::empty(Layer::Env);
        env.bind_addr = Some("10.0.0.1".to_string());
        env.bind_port = Some(2);

        let mut flags = flags_layer();
        flags.log_level = Some(LogLevel::Debug);
        flags.bind_port = Some(3);

        let cfg = Config::resolve_standard(&file, &env, &flags).expect("valid");
        assert_eq!(cfg.node_name, "from-file"); // only file set it
        assert_eq!(cfg.data_dir, "/data/file"); // only file set it
        assert_eq!(cfg.bind_addr, "10.0.0.1"); // only env set it
        assert_eq!(cfg.log_level, LogLevel::Debug); // only flags set it
        assert_eq!(cfg.bind_port, 3); // contested -> flags wins
    }

    #[test]
    fn unset_higher_layer_field_does_not_clear_lower() {
        let mut file = ConfigLayer::empty(Layer::File);
        file.node_name = Some("keepme".to_string());
        // env + flags silent on node_name.
        let cfg = Config::resolve_standard(&file, &ConfigLayer::empty(Layer::Env), &flags_layer())
            .expect("valid");
        assert_eq!(cfg.node_name, "keepme");
    }

    #[test]
    fn role_can_be_overridden_by_a_layer() {
        let mut flags = flags_layer();
        flags.role = Some(NodeRole::Secondary);
        let cfg = Config::resolve_standard(
            &ConfigLayer::empty(Layer::File),
            &ConfigLayer::empty(Layer::Env),
            &flags,
        )
        .expect("valid");
        assert_eq!(cfg.role, NodeRole::Secondary);
        // Secondary's default component set is a strict subset.
        assert!(cfg.components.contains(&Component::Core));
        assert!(!cfg.components.contains(&Component::Cameras));
    }

    #[test]
    fn explicit_components_override_role_defaults() {
        let mut flags = flags_layer();
        flags.role = Some(NodeRole::Hub);
        flags.components = Some(
            [Component::Orchestration, Component::Core]
                .into_iter()
                .collect(),
        );
        let cfg = Config::resolve_standard(
            &ConfigLayer::empty(Layer::File),
            &ConfigLayer::empty(Layer::Env),
            &flags,
        )
        .expect("valid");
        assert_eq!(
            cfg.components,
            [Component::Orchestration, Component::Core]
                .into_iter()
                .collect()
        );
    }

    #[test]
    fn ml_role_defaults_include_cameras() {
        let mut flags = flags_layer();
        flags.role = Some(NodeRole::Ml);
        let cfg = Config::resolve_standard(
            &ConfigLayer::empty(Layer::File),
            &ConfigLayer::empty(Layer::Env),
            &flags,
        )
        .expect("ml valid");
        assert!(cfg.components.contains(&Component::Cameras));
    }

    #[test]
    fn bad_node_name_rejected() {
        let mut flags = flags_layer();
        flags.node_name = Some("living room!".to_string());
        let err = Config::resolve_standard(
            &ConfigLayer::empty(Layer::File),
            &ConfigLayer::empty(Layer::Env),
            &flags,
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::BadNodeName(_)));
    }

    #[test]
    fn empty_node_name_rejected() {
        let mut flags = flags_layer();
        flags.node_name = Some("   ".to_string());
        let err = Config::resolve_standard(
            &ConfigLayer::empty(Layer::File),
            &ConfigLayer::empty(Layer::Env),
            &flags,
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::BadNodeName(_)));
    }

    #[test]
    fn relative_data_dir_rejected() {
        let mut flags = flags_layer();
        flags.data_dir = Some("relative/path".to_string());
        let err = Config::resolve_standard(
            &ConfigLayer::empty(Layer::File),
            &ConfigLayer::empty(Layer::Env),
            &flags,
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::BadDataDir(_)));
    }

    #[test]
    fn zero_port_rejected() {
        let mut flags = flags_layer();
        flags.bind_port = Some(0);
        let err = Config::resolve_standard(
            &ConfigLayer::empty(Layer::File),
            &ConfigLayer::empty(Layer::Env),
            &flags,
        )
        .unwrap_err();
        assert_eq!(err, ConfigError::BadPort);
    }

    #[test]
    fn empty_components_rejected() {
        let mut flags = flags_layer();
        flags.components = Some(BTreeSet::new());
        let err = Config::resolve_standard(
            &ConfigLayer::empty(Layer::File),
            &ConfigLayer::empty(Layer::Env),
            &flags,
        )
        .unwrap_err();
        assert_eq!(err, ConfigError::NoComponents);
    }

    #[test]
    fn pillars_without_orchestration_rejected() {
        let mut flags = flags_layer();
        flags.components = Some([Component::Core, Component::Portal].into_iter().collect());
        let err = Config::resolve_standard(
            &ConfigLayer::empty(Layer::File),
            &ConfigLayer::empty(Layer::Env),
            &flags,
        )
        .unwrap_err();
        assert_eq!(err, ConfigError::MissingOrchestration);
    }

    #[test]
    fn ml_role_without_cameras_rejected() {
        let mut flags = flags_layer();
        flags.role = Some(NodeRole::Ml);
        flags.components = Some([Component::Orchestration].into_iter().collect());
        let err = Config::resolve_standard(
            &ConfigLayer::empty(Layer::File),
            &ConfigLayer::empty(Layer::Env),
            &flags,
        )
        .unwrap_err();
        assert_eq!(err, ConfigError::MlNodeWithoutCameras);
    }

    #[test]
    fn role_and_loglevel_parsing_is_case_insensitive() {
        assert_eq!(NodeRole::from_str_ci("HUB"), Some(NodeRole::Hub));
        assert_eq!(NodeRole::from_str_ci(" Ml "), Some(NodeRole::Ml));
        assert_eq!(NodeRole::from_str_ci("worker"), None);
        assert_eq!(LogLevel::from_str_ci("WARNING"), Some(LogLevel::Warn));
        assert_eq!(LogLevel::from_str_ci("trace"), Some(LogLevel::Trace));
        assert_eq!(LogLevel::from_str_ci("loud"), None);
    }

    #[test]
    fn error_messages_are_grandma_friendly() {
        // No implementation jargon in the user-facing error text (Charter §6.3).
        let msg = ConfigError::MissingOrchestration.to_string();
        let lower = msg.to_ascii_lowercase();
        for jargon in ["orchestration", "k3s", "pod", "kubelet"] {
            assert!(!lower.contains(jargon), "jargon `{jargon}` leaked: {msg}");
        }
    }
}
