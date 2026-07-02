//! The `EncryptionConfiguration` model: resources → ordered providers.
//!
//! See the module-level docs in [`crate::secrets_encryption`] for the scheme.
//!
//! Mirrors the Kubernetes `apiserver.config.k8s.io` `EncryptionConfiguration`: a
//! list of rules, each mapping a set of resource names to an **ordered** list of
//! providers. The first provider of the first matching rule is the *write*
//! provider; the rest are read fallbacks. cave-home's provider set is just two —
//! the PQC KMS and identity.

use super::transformer::WriteMode;

/// A single encryption provider in a rule's ordered list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// The post-quantum envelope KMS (ML-KEM-768 + AES-256-GCM).
    Mlkem768Kms,
    /// The identity (no-op) provider — passthrough plaintext.
    Identity,
}

/// One rule: which resources it covers and the ordered providers to apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceEncryption {
    /// Resource names this rule governs (e.g. `"secrets"`).
    pub resources: Vec<String>,
    /// Providers in priority order; `providers[0]` is the write provider.
    pub providers: Vec<ProviderKind>,
}

/// The whole encryption configuration: an ordered list of rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptionConfiguration {
    /// Rules in priority order; the first matching a resource wins.
    pub rules: Vec<ResourceEncryption>,
}

/// Why a configuration failed validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionConfigError {
    /// The configuration had no rules at all.
    NoRules,
    /// A rule listed no resources.
    NoResources,
    /// A rule listed no providers.
    NoProviders,
}

impl core::fmt::Display for EncryptionConfigError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let s = match self {
            Self::NoRules => "encryption config: no rules",
            Self::NoResources => "encryption config: a rule has no resources",
            Self::NoProviders => "encryption config: a rule has no providers",
        };
        f.write_str(s)
    }
}

impl std::error::Error for EncryptionConfigError {}

impl EncryptionConfiguration {
    /// Encryption disabled: write identity for `secrets`.
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            rules: vec![ResourceEncryption {
                resources: vec!["secrets".to_owned()],
                providers: vec![ProviderKind::Identity],
            }],
        }
    }

    /// The standard config: encrypt `secrets` with the PQC KMS, with identity as
    /// the read fallback (so a later disable does not strand existing data).
    #[must_use]
    pub fn for_secrets_mlkem768() -> Self {
        Self {
            rules: vec![ResourceEncryption {
                resources: vec!["secrets".to_owned()],
                providers: vec![ProviderKind::Mlkem768Kms, ProviderKind::Identity],
            }],
        }
    }

    /// Validate the configuration's shape.
    ///
    /// # Errors
    /// [`EncryptionConfigError`] if there are no rules, or any rule has no
    /// resources or no providers.
    pub fn validate(&self) -> Result<(), EncryptionConfigError> {
        if self.rules.is_empty() {
            return Err(EncryptionConfigError::NoRules);
        }
        for rule in &self.rules {
            if rule.resources.is_empty() {
                return Err(EncryptionConfigError::NoResources);
            }
            if rule.providers.is_empty() {
                return Err(EncryptionConfigError::NoProviders);
            }
        }
        Ok(())
    }

    /// The write mode for `resource` — the write provider of the first matching
    /// rule. `None` if no rule covers the resource (or the rule is empty).
    #[must_use]
    pub fn write_mode_for(&self, resource: &str) -> Option<WriteMode> {
        let rule = self
            .rules
            .iter()
            .find(|r| r.resources.iter().any(|res| res == resource))?;
        rule.providers.first().map(|p| match p {
            ProviderKind::Mlkem768Kms => WriteMode::Encrypt,
            ProviderKind::Identity => WriteMode::Identity,
        })
    }

    /// Whether new writes of `resource` are encrypted.
    #[must_use]
    pub fn is_encrypted(&self, resource: &str) -> bool {
        self.write_mode_for(resource) == Some(WriteMode::Encrypt)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::secrets_encryption::transformer::WriteMode;

    #[test]
    fn disabled_config_writes_identity_for_secrets() {
        let cfg = EncryptionConfiguration::disabled();
        cfg.validate().unwrap();
        assert_eq!(cfg.write_mode_for("secrets"), Some(WriteMode::Identity));
        assert!(!cfg.is_encrypted("secrets"));
    }

    #[test]
    fn standard_config_encrypts_secrets() {
        let cfg = EncryptionConfiguration::for_secrets_mlkem768();
        cfg.validate().unwrap();
        assert_eq!(cfg.write_mode_for("secrets"), Some(WriteMode::Encrypt));
        assert!(cfg.is_encrypted("secrets"));
    }

    #[test]
    fn resource_not_listed_has_no_rule() {
        let cfg = EncryptionConfiguration::for_secrets_mlkem768();
        assert_eq!(cfg.write_mode_for("configmaps"), None);
        assert!(!cfg.is_encrypted("configmaps"));
    }

    #[test]
    fn first_provider_decides_write_mode() {
        // identity listed first ⇒ write identity, even though mlkem768 can read.
        let cfg = EncryptionConfiguration {
            rules: vec![ResourceEncryption {
                resources: vec!["secrets".to_owned()],
                providers: vec![ProviderKind::Identity, ProviderKind::Mlkem768Kms],
            }],
        };
        cfg.validate().unwrap();
        assert_eq!(cfg.write_mode_for("secrets"), Some(WriteMode::Identity));
    }

    #[test]
    fn first_matching_rule_wins() {
        let cfg = EncryptionConfiguration {
            rules: vec![
                ResourceEncryption {
                    resources: vec!["secrets".to_owned()],
                    providers: vec![ProviderKind::Mlkem768Kms, ProviderKind::Identity],
                },
                ResourceEncryption {
                    resources: vec!["secrets".to_owned()],
                    providers: vec![ProviderKind::Identity],
                },
            ],
        };
        assert_eq!(cfg.write_mode_for("secrets"), Some(WriteMode::Encrypt));
    }

    #[test]
    fn validate_rejects_empty_rules() {
        let cfg = EncryptionConfiguration { rules: vec![] };
        assert!(matches!(cfg.validate(), Err(EncryptionConfigError::NoRules)));
    }

    #[test]
    fn validate_rejects_empty_providers() {
        let cfg = EncryptionConfiguration {
            rules: vec![ResourceEncryption {
                resources: vec!["secrets".to_owned()],
                providers: vec![],
            }],
        };
        assert!(matches!(
            cfg.validate(),
            Err(EncryptionConfigError::NoProviders)
        ));
    }

    #[test]
    fn validate_rejects_empty_resources() {
        let cfg = EncryptionConfiguration {
            rules: vec![ResourceEncryption {
                resources: vec![],
                providers: vec![ProviderKind::Mlkem768Kms],
            }],
        };
        assert!(matches!(
            cfg.validate(),
            Err(EncryptionConfigError::NoResources)
        ));
    }
}
