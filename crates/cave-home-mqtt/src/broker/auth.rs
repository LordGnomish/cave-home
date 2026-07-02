//! Authentication (username/password, §3.1.3.5/§3.1.3.6) and topic ACL.
//!
//! Clean-room: Mosquitto's password file uses PBKDF2/bcrypt hashes; this
//! core keeps the verification step pluggable (a stored secret compared
//! against the presented one) and focuses on the topic-based ACL, which
//! is the broker-specific authorization logic.

use crate::broker::topic::topic_matches;
use std::collections::HashMap;

/// The operation an ACL rule governs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AclAction {
    Publish,
    Subscribe,
}

/// A single ACL rule. `username == None` matches any authenticated (or
/// anonymous) client; `action == None` matches both publish and
/// subscribe. `topic` is a Topic Filter matched against the operation's
/// topic. Rules are evaluated in order; the first match wins.
#[derive(Clone, Debug, PartialEq, Eq)]
struct AclRule {
    username: Option<String>,
    action: Option<AclAction>,
    topic: String,
    allow: bool,
}

/// Username/password verification plus an ordered topic ACL.
#[derive(Debug, Default)]
pub struct Authenticator {
    credentials: HashMap<String, Vec<u8>>,
    allow_anonymous: bool,
    default_allow: bool,
    rules: Vec<AclRule>,
}

impl Authenticator {
    pub fn set_anonymous(&mut self, allow: bool) {
        self.allow_anonymous = allow;
    }

    /// Default verdict when no ACL rule matches (Mosquitto's default is
    /// deny; set `true` for an allow-by-default broker).
    pub fn set_default_allow(&mut self, allow: bool) {
        self.default_allow = allow;
    }

    pub fn add_user(&mut self, username: &str, password: &[u8]) {
        self.credentials.insert(username.to_owned(), password.to_vec());
    }

    pub fn allow(&mut self, username: &str, action: AclAction, topic: &str) {
        self.push_rule(Some(username), Some(action), topic, true);
    }

    pub fn deny(&mut self, username: &str, action: AclAction, topic: &str) {
        self.push_rule(Some(username), Some(action), topic, false);
    }

    pub fn allow_any(&mut self, action: AclAction, topic: &str) {
        self.push_rule(None, Some(action), topic, true);
    }

    fn push_rule(
        &mut self,
        username: Option<&str>,
        action: Option<AclAction>,
        topic: &str,
        allow: bool,
    ) {
        self.rules.push(AclRule {
            username: username.map(ToOwned::to_owned),
            action,
            topic: topic.to_owned(),
            allow,
        });
    }

    /// §3.1.3.5/§3.1.3.6 — verify the presented credentials. An absent
    /// username is anonymous and permitted only when anonymous access is
    /// enabled.
    pub fn authenticate(&self, username: Option<&str>, password: Option<&[u8]>) -> bool {
        match username {
            None => self.allow_anonymous,
            Some(user) => match self.credentials.get(user) {
                Some(stored) => password == Some(stored.as_slice()),
                None => false,
            },
        }
    }

    /// Decide whether `username` may perform `action` on `topic`. The
    /// first rule whose username, action and topic filter all match
    /// determines the verdict; otherwise the default applies.
    pub fn authorize(&self, username: Option<&str>, action: AclAction, topic: &str) -> bool {
        for rule in &self.rules {
            let user_ok = match &rule.username {
                None => true,
                Some(u) => Some(u.as_str()) == username,
            };
            let action_ok = rule.action.is_none_or(|a| a == action);
            if user_ok && action_ok && topic_matches(&rule.topic, topic) {
                return rule.allow;
            }
        }
        self.default_allow
    }
}

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
