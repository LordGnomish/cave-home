//! Node-role configuration: the K3s server-vs-agent decision and its inputs.
//!
//! K3s boots a node in one of two roles. A **server** runs the control plane
//! (apiserver, scheduler, controller-manager) on top of a datastore (kine over
//! `SQLite` by default, or an external endpoint), and also runs the node-side
//! agent components. An **agent** runs only the node-side components (kubelet,
//! kube-proxy, CNI) and joins an existing server over a URL with a token.
//!
//! This module models the *configuration* those two roles take and validates
//! it. It deliberately does **not** implement any crypto: the token model is
//! validated for *shape* only (non-empty, no whitespace, plausible length).
//! Real token derivation / TLS bootstrap is ADR-004 phase-1b (see manifest).

use core::fmt;

/// Where the control plane keeps cluster state.
///
/// K3s defaults to an embedded kine instance backed by `SQLite`; a server may
/// instead point at an external datastore endpoint (kine-fronted `Postgres` /
/// `MySQL`, or an external etcd). cave-home's single-binary default is the
/// embedded path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Datastore {
    /// Embedded kine over a local `SQLite` file (the single-binary default).
    EmbeddedSqlite,
    /// An external datastore endpoint, e.g. `postgres://…` or `https://…:2379`.
    /// The string is validated for shape (non-empty, has a `://` scheme), not
    /// reachability.
    External(String),
}

impl Datastore {
    /// Whether this datastore is the embedded single-binary path.
    #[must_use]
    pub const fn is_embedded(&self) -> bool {
        matches!(self, Self::EmbeddedSqlite)
    }
}

/// A cluster or node token, validated for *shape* only.
///
/// K3s uses a cluster token to authorise a node joining a server, and (after
/// bootstrap) per-node tokens. This type rejects obviously-malformed tokens
/// (empty, whitespace-bearing, too short) without performing any cryptography:
/// derivation, hashing and verification are ADR-004 phase-1b.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token(String);

/// The minimum plausible token length. K3s tokens are far longer; this is a
/// floor that rejects trivially-empty / typo'd values, not a security check.
pub const MIN_TOKEN_LEN: usize = 8;

impl Token {
    /// Validate and wrap a token string.
    ///
    /// # Errors
    /// Returns [`ConfigError::EmptyToken`] when empty, [`ConfigError::TokenTooShort`]
    /// when below [`MIN_TOKEN_LEN`], or [`ConfigError::TokenHasWhitespace`] when
    /// it contains any whitespace (which never appears in a real K3s token).
    pub fn new(raw: &str) -> Result<Self, ConfigError> {
        if raw.is_empty() {
            return Err(ConfigError::EmptyToken);
        }
        if raw.chars().any(char::is_whitespace) {
            return Err(ConfigError::TokenHasWhitespace);
        }
        if raw.len() < MIN_TOKEN_LEN {
            return Err(ConfigError::TokenTooShort {
                got: raw.len(),
                min: MIN_TOKEN_LEN,
            });
        }
        Ok(Self(raw.to_owned()))
    }

    /// The validated token text.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Optional batteries-included add-ons K3s bundles and can be toggled.
///
/// K3s enables flannel (CNI), servicelb (the klipper-lb load balancer), and
/// traefik (ingress) by default; each can be disabled. cave-home models the
/// flags so the bring-up planner knows which optional components to schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Addons {
    /// Flannel CNI. Disabling it means an external CNI is supplied.
    pub flannel: bool,
    /// The klipper service load balancer (servicelb).
    pub servicelb: bool,
    /// The Traefik ingress controller.
    pub traefik: bool,
    /// Explicit disable list (mirrors K3s `--disable`), e.g. `"traefik"`.
    /// Entries here override the corresponding `true` flag above.
    pub disabled: Vec<String>,
}

impl Default for Addons {
    /// K3s defaults: flannel, servicelb and traefik all on, nothing disabled.
    fn default() -> Self {
        Self {
            flannel: true,
            servicelb: true,
            traefik: true,
            disabled: Vec::new(),
        }
    }
}

impl Addons {
    /// Whether the named add-on is effectively enabled, honouring both the flag
    /// and the disable list (the list wins).
    #[must_use]
    pub fn is_enabled(&self, name: &str) -> bool {
        if self.disabled.iter().any(|d| d == name) {
            return false;
        }
        match name {
            "flannel" => self.flannel,
            "servicelb" => self.servicelb,
            "traefik" => self.traefik,
            _ => false,
        }
    }
}

/// How a server starts a cluster: it initialises a brand-new one, or it joins
/// an existing server (HA control-plane members beyond the first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClusterStart {
    /// `--cluster-init`: this server creates the cluster (the first server).
    Init,
    /// This server joins an existing cluster at the given server URL.
    Join { server_url: String },
}

/// Configuration for a node booted in the K3s **server** role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    /// Stable node identity.
    pub node_name: String,
    /// Where cluster state lives.
    pub datastore: Datastore,
    /// The cluster token (shape-validated).
    pub token: Token,
    /// Whether this server initialises the cluster or joins an existing one.
    pub start: ClusterStart,
    /// Bundled add-on toggles.
    pub addons: Addons,
}

/// Configuration for a node booted in the K3s **agent** role (a worker).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentConfig {
    /// Stable node identity.
    pub node_name: String,
    /// The server URL this agent joins.
    pub server_url: String,
    /// The cluster token (shape-validated).
    pub token: Token,
}

/// A node's resolved role + its configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeConfig {
    /// A control-plane server.
    Server(ServerConfig),
    /// A worker agent.
    Agent(AgentConfig),
}

impl NodeConfig {
    /// Whether this node carries the control plane.
    #[must_use]
    pub const fn is_server(&self) -> bool {
        matches!(self, Self::Server(_))
    }

    /// The node's stable name, regardless of role.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn node_name(&self) -> &str {
        match self {
            Self::Server(s) => &s.node_name,
            Self::Agent(a) => &a.node_name,
        }
    }

    /// Validate the whole configuration, returning the first problem found.
    ///
    /// Checks performed (no panics, never crypto):
    /// - node name is non-empty and whitespace-free;
    /// - a server's datastore is shape-valid (external endpoints need a scheme);
    /// - a server's add-on flags do not conflict (a flag cannot be both `true`
    ///   and named in the disable list);
    /// - a joining server / any agent has a shape-valid server URL.
    ///
    /// # Errors
    /// Returns the first [`ConfigError`] encountered.
    pub fn validate(&self) -> Result<(), ConfigError> {
        match self {
            Self::Server(s) => s.validate(),
            Self::Agent(a) => a.validate(),
        }
    }
}

impl ServerConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        validate_node_name(&self.node_name)?;
        validate_datastore(&self.datastore)?;
        validate_addons(&self.addons)?;
        if let ClusterStart::Join { server_url } = &self.start {
            validate_url(server_url)?;
        }
        Ok(())
    }
}

impl AgentConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        validate_node_name(&self.node_name)?;
        validate_url(&self.server_url)?;
        Ok(())
    }
}

fn validate_node_name(name: &str) -> Result<(), ConfigError> {
    if name.is_empty() {
        return Err(ConfigError::EmptyNodeName);
    }
    if name.chars().any(char::is_whitespace) {
        return Err(ConfigError::NodeNameHasWhitespace);
    }
    Ok(())
}

fn validate_datastore(ds: &Datastore) -> Result<(), ConfigError> {
    match ds {
        Datastore::EmbeddedSqlite => Ok(()),
        Datastore::External(endpoint) => {
            if endpoint.is_empty() {
                return Err(ConfigError::EmptyDatastore);
            }
            if !endpoint.contains("://") {
                return Err(ConfigError::DatastoreNoScheme);
            }
            Ok(())
        }
    }
}

fn validate_addons(a: &Addons) -> Result<(), ConfigError> {
    // A flag set `true` while also named in the disable list is a contradiction
    // the caller should resolve before bring-up rather than silently ignore.
    for (flag, name) in [
        (a.flannel, "flannel"),
        (a.servicelb, "servicelb"),
        (a.traefik, "traefik"),
    ] {
        if flag && a.disabled.iter().any(|d| d == name) {
            return Err(ConfigError::ConflictingAddon {
                name: name.to_owned(),
            });
        }
    }
    Ok(())
}

fn validate_url(url: &str) -> Result<(), ConfigError> {
    if url.is_empty() {
        return Err(ConfigError::EmptyServerUrl);
    }
    if !url.contains("://") {
        return Err(ConfigError::ServerUrlNoScheme);
    }
    Ok(())
}

/// Everything that can be wrong with a [`NodeConfig`], reported instead of
/// panicking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// The node name was empty.
    EmptyNodeName,
    /// The node name contained whitespace.
    NodeNameHasWhitespace,
    /// The token was empty.
    EmptyToken,
    /// The token contained whitespace.
    TokenHasWhitespace,
    /// The token was below the minimum plausible length.
    TokenTooShort { got: usize, min: usize },
    /// An external datastore endpoint was empty.
    EmptyDatastore,
    /// An external datastore endpoint lacked a `scheme://`.
    DatastoreNoScheme,
    /// An add-on flag was `true` while also named in the disable list.
    ConflictingAddon { name: String },
    /// A server URL was required but empty.
    EmptyServerUrl,
    /// A server URL lacked a `scheme://`.
    ServerUrlNoScheme,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyNodeName => f.write_str("node name is empty"),
            Self::NodeNameHasWhitespace => f.write_str("node name contains whitespace"),
            Self::EmptyToken => f.write_str("token is empty"),
            Self::TokenHasWhitespace => f.write_str("token contains whitespace"),
            Self::TokenTooShort { got, min } => {
                write!(f, "token too short: {got} < {min} bytes")
            }
            Self::EmptyDatastore => f.write_str("external datastore endpoint is empty"),
            Self::DatastoreNoScheme => {
                f.write_str("external datastore endpoint has no scheme://")
            }
            Self::ConflictingAddon { name } => {
                write!(f, "add-on '{name}' is both enabled and in the disable list")
            }
            Self::EmptyServerUrl => f.write_str("server URL is empty"),
            Self::ServerUrlNoScheme => f.write_str("server URL has no scheme://"),
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn good_token() -> Token {
        match Token::new("K10abcdef::server:secrethere") {
            Ok(t) => t,
            Err(_) => Token("fallback-token".to_owned()),
        }
    }

    #[test]
    fn token_rejects_empty_short_and_whitespace() {
        assert_eq!(Token::new(""), Err(ConfigError::EmptyToken));
        assert_eq!(
            Token::new("short"),
            Err(ConfigError::TokenTooShort { got: 5, min: 8 })
        );
        assert_eq!(
            Token::new("has space here"),
            Err(ConfigError::TokenHasWhitespace)
        );
    }

    #[test]
    fn token_accepts_plausible_value() {
        let t = Token::new("K10::node::abcdef").expect("valid token");
        assert_eq!(t.as_str(), "K10::node::abcdef");
    }

    #[test]
    fn embedded_datastore_is_default_single_binary_path() {
        assert!(Datastore::EmbeddedSqlite.is_embedded());
        assert!(!Datastore::External("postgres://x".to_owned()).is_embedded());
    }

    #[test]
    fn addons_default_is_all_on_nothing_disabled() {
        let a = Addons::default();
        assert!(a.flannel && a.servicelb && a.traefik);
        assert!(a.disabled.is_empty());
        assert!(a.is_enabled("flannel"));
        assert!(!a.is_enabled("unknown"));
    }

    #[test]
    fn disable_list_overrides_flag_in_is_enabled() {
        let a = Addons {
            traefik: true,
            disabled: vec!["traefik".to_owned()],
            ..Addons::default()
        };
        assert!(!a.is_enabled("traefik"));
    }

    #[test]
    fn server_init_config_validates() {
        let cfg = NodeConfig::Server(ServerConfig {
            node_name: "hub-1".to_owned(),
            datastore: Datastore::EmbeddedSqlite,
            token: good_token(),
            start: ClusterStart::Init,
            addons: Addons::default(),
        });
        assert!(cfg.validate().is_ok());
        assert!(cfg.is_server());
        assert_eq!(cfg.node_name(), "hub-1");
    }

    #[test]
    fn agent_config_validates_and_is_not_server() {
        let cfg = NodeConfig::Agent(AgentConfig {
            node_name: "worker-1".to_owned(),
            server_url: "https://hub-1:6443".to_owned(),
            token: good_token(),
        });
        assert!(cfg.validate().is_ok());
        assert!(!cfg.is_server());
    }

    #[test]
    fn server_external_datastore_needs_scheme() {
        let cfg = NodeConfig::Server(ServerConfig {
            node_name: "hub-1".to_owned(),
            datastore: Datastore::External("just-a-host".to_owned()),
            token: good_token(),
            start: ClusterStart::Init,
            addons: Addons::default(),
        });
        assert_eq!(cfg.validate(), Err(ConfigError::DatastoreNoScheme));
    }

    #[test]
    fn server_empty_external_datastore_rejected() {
        let cfg = NodeConfig::Server(ServerConfig {
            node_name: "hub-1".to_owned(),
            datastore: Datastore::External(String::new()),
            token: good_token(),
            start: ClusterStart::Init,
            addons: Addons::default(),
        });
        assert_eq!(cfg.validate(), Err(ConfigError::EmptyDatastore));
    }

    #[test]
    fn conflicting_addon_flag_and_disable_list_rejected() {
        let cfg = NodeConfig::Server(ServerConfig {
            node_name: "hub-1".to_owned(),
            datastore: Datastore::EmbeddedSqlite,
            token: good_token(),
            start: ClusterStart::Init,
            addons: Addons {
                traefik: true,
                disabled: vec!["traefik".to_owned()],
                ..Addons::default()
            },
        });
        assert_eq!(
            cfg.validate(),
            Err(ConfigError::ConflictingAddon {
                name: "traefik".to_owned()
            })
        );
    }

    #[test]
    fn joining_server_needs_valid_url() {
        let cfg = NodeConfig::Server(ServerConfig {
            node_name: "hub-2".to_owned(),
            datastore: Datastore::EmbeddedSqlite,
            token: good_token(),
            start: ClusterStart::Join {
                server_url: "no-scheme".to_owned(),
            },
            addons: Addons::default(),
        });
        assert_eq!(cfg.validate(), Err(ConfigError::ServerUrlNoScheme));
    }

    #[test]
    fn agent_empty_url_rejected() {
        let cfg = NodeConfig::Agent(AgentConfig {
            node_name: "worker-1".to_owned(),
            server_url: String::new(),
            token: good_token(),
        });
        assert_eq!(cfg.validate(), Err(ConfigError::EmptyServerUrl));
    }

    #[test]
    fn empty_node_name_rejected() {
        let cfg = NodeConfig::Agent(AgentConfig {
            node_name: String::new(),
            server_url: "https://hub-1:6443".to_owned(),
            token: good_token(),
        });
        assert_eq!(cfg.validate(), Err(ConfigError::EmptyNodeName));
    }

    #[test]
    fn whitespace_node_name_rejected() {
        let cfg = NodeConfig::Agent(AgentConfig {
            node_name: "bad name".to_owned(),
            server_url: "https://hub-1:6443".to_owned(),
            token: good_token(),
        });
        assert_eq!(cfg.validate(), Err(ConfigError::NodeNameHasWhitespace));
    }

    #[test]
    fn config_error_displays_without_panicking() {
        // Exercise every Display arm.
        let errs = [
            ConfigError::EmptyNodeName,
            ConfigError::NodeNameHasWhitespace,
            ConfigError::EmptyToken,
            ConfigError::TokenHasWhitespace,
            ConfigError::TokenTooShort { got: 1, min: 8 },
            ConfigError::EmptyDatastore,
            ConfigError::DatastoreNoScheme,
            ConfigError::ConflictingAddon {
                name: "x".to_owned(),
            },
            ConfigError::EmptyServerUrl,
            ConfigError::ServerUrlNoScheme,
        ];
        for e in errs {
            assert!(!format!("{e}").is_empty());
        }
    }
}
