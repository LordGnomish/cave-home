//! Authentication (username/password, §3.1.3.5/§3.1.3.6) and topic ACL.
//!
//! Clean-room: Mosquitto's password file uses PBKDF2/bcrypt hashes; this
//! core keeps the verification step pluggable (a stored secret compared
//! against the presented one) and focuses on the topic-based ACL, which
//! is the broker-specific authorization logic.

#[cfg(test)]
mod tests {
    use super::*;

    fn authn() -> Authenticator {
        let mut a = Authenticator::default();
        a.set_anonymous(false);
        a.add_user("admin", b"s3cret");
        a
    }

    #[test]
    fn known_credentials_authenticate() {
        let a = authn();
        assert!(a.authenticate(Some("admin"), Some(b"s3cret")));
        assert!(!a.authenticate(Some("admin"), Some(b"wrong")));
        assert!(!a.authenticate(Some("ghost"), Some(b"x")));
        // Anonymous rejected when disabled.
        assert!(!a.authenticate(None, None));
    }

    #[test]
    fn anonymous_allowed_when_enabled() {
        let mut a = Authenticator::default();
        a.set_anonymous(true);
        assert!(a.authenticate(None, None));
    }

    #[test]
    fn default_deny_with_a_scoped_allow() {
        let mut a = authn();
        a.set_default_allow(false);
        a.allow("admin", AclAction::Publish, "home/#");
        // The allow grants the in-scope topic; the default-deny blocks an
        // out-of-scope topic and an action with no matching allow rule.
        assert!(a.authorize(Some("admin"), AclAction::Publish, "home/loft/temp"));
        assert!(!a.authorize(Some("admin"), AclAction::Publish, "factory/line1"));
        assert!(!a.authorize(Some("admin"), AclAction::Subscribe, "home/loft/temp"));
    }

    #[test]
    fn deny_rule_overrides_when_listed_first() {
        let mut a = authn();
        a.set_default_allow(true);
        a.deny("admin", AclAction::Publish, "home/secret/#");
        assert!(!a.authorize(Some("admin"), AclAction::Publish, "home/secret/code"));
        assert!(a.authorize(Some("admin"), AclAction::Publish, "home/loft/temp"));
    }

    #[test]
    fn wildcard_username_rule_matches_any_user() {
        let mut a = Authenticator::default();
        a.set_anonymous(true);
        a.set_default_allow(false);
        a.allow_any(AclAction::Subscribe, "public/#");
        assert!(a.authorize(None, AclAction::Subscribe, "public/news"));
        assert!(!a.authorize(None, AclAction::Subscribe, "private/news"));
    }
}
