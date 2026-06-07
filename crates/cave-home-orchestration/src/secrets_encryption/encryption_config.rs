//! The `EncryptionConfiguration` model: resources → ordered providers.
//!
//! See the module-level docs in [`crate::secrets_encryption`] for the scheme.

// ── RED (TDD) ────────────────────────────────────────────────────────────────
// Failing tests first; implementation lands in the paired `feat` commit.

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
