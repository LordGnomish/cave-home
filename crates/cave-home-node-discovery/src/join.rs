//! Join-token model + pairing handshake **state machine** (Charter §6.3 /
//! ADR-005).
//!
//! When a homeowner adds a node, the user-facing path is QR / token sharing /
//! IP picker (ADR-005 Portal "Add node" wizard, rendered over the
//! `cavehome join --token=… --hub=…` CLI primitive). This module models two
//! things, both pure logic:
//!
//! 1. [`JoinToken`] — the enrolment token's *shape and validation* (length,
//!    charset, expiry over a caller clock). The token here is a plain bearer
//!    string; the real cryptographic mutual-TLS / PSK exchange is deferred to
//!    Phase 1b (see the parity manifest). Validation rejects malformed or
//!    expired tokens up front.
//! 2. [`Pairing`] — the handshake **state** a joining node walks through:
//!    [`PairingState::Discovered`] → [`PairingState::TokenOffered`] →
//!    [`PairingState::Joined`], with an explicit
//!    [`PairingState::Rejected`] terminal. No bytes hit the wire here; this is
//!    the protocol's control state only.

/// A cave-home join token, as printed/scanned during enrolment.
///
/// Format (Phase 1): an opaque base32-style bearer string,
/// [`JoinToken::LEN`] characters of the [`JoinToken`] charset, paired with the
/// tick at which it was issued and how long it stays valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinToken {
    value: String,
    issued_at: u64,
    lifetime: u64,
}

/// Why a token string was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenError {
    /// The token was not exactly [`JoinToken::LEN`] characters.
    WrongLength { got: usize },
    /// The token contained a character outside the allowed charset.
    BadCharacter(char),
    /// The token's lifetime was zero (it would never be valid).
    ZeroLifetime,
}

impl core::fmt::Display for TokenError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::WrongLength { got } => {
                write!(f, "join token must be {} characters, got {got}", JoinToken::LEN)
            }
            Self::BadCharacter(c) => write!(f, "join token has invalid character {c:?}"),
            Self::ZeroLifetime => f.write_str("join token lifetime is zero"),
        }
    }
}

impl std::error::Error for TokenError {}

impl JoinToken {
    /// The fixed token length (characters).
    pub const LEN: usize = 12;

    /// The allowed token charset: upper-case Crockford-style base32 (no
    /// ambiguous `I`, `L`, `O`, `U`) plus digits 2-9. Chosen so a token reads
    /// cleanly off a screen / QR for the family persona.
    pub const CHARSET: &'static str = "ABCDEFGHJKMNPQRSTVWXYZ23456789";

    /// Validate and wrap a token string.
    ///
    /// # Errors
    /// - [`TokenError::WrongLength`] if not exactly [`JoinToken::LEN`] chars.
    /// - [`TokenError::BadCharacter`] for any out-of-charset character.
    /// - [`TokenError::ZeroLifetime`] if `lifetime` is zero.
    pub fn parse(value: &str, issued_at: u64, lifetime: u64) -> Result<Self, TokenError> {
        let count = value.chars().count();
        if count != Self::LEN {
            return Err(TokenError::WrongLength { got: count });
        }
        if lifetime == 0 {
            return Err(TokenError::ZeroLifetime);
        }
        for c in value.chars() {
            if !Self::CHARSET.contains(c) {
                return Err(TokenError::BadCharacter(c));
            }
        }
        Ok(Self {
            value: value.to_owned(),
            issued_at,
            lifetime,
        })
    }

    /// The token string.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Whether the token is still within its validity window at `now`.
    ///
    /// Valid for `issued_at ..= issued_at + lifetime`; pure over the supplied
    /// clock.
    #[must_use]
    pub const fn is_valid_at(&self, now: u64) -> bool {
        now >= self.issued_at && now <= self.issued_at.saturating_add(self.lifetime)
    }

    /// Constant-time-ish equality against an offered token string. (A genuine
    /// constant-time compare belongs with the deferred real crypto; this is the
    /// modelled bearer check.)
    #[must_use]
    pub fn matches(&self, offered: &str) -> bool {
        self.value == offered
    }
}

/// The state of a node-join handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingState {
    /// The joining node has been discovered on the LAN but no token offered.
    Discovered,
    /// A token has been offered and is awaiting verification against the hub.
    TokenOffered,
    /// The token verified; the node is accepted into the cluster.
    Joined,
    /// The handshake failed (bad / expired token, or version-incompatible).
    Rejected(RejectReason),
}

/// Why a pairing handshake was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectReason {
    /// The offered token did not match the hub's token.
    TokenMismatch,
    /// The token was past its validity window.
    TokenExpired,
    /// The joining node was on an incompatible version (Charter §8).
    VersionIncompatible,
}

/// A pairing handshake driven step by step. Holds the hub's expected token and
/// the current [`PairingState`]; transitions are explicit method calls.
#[derive(Debug, Clone)]
pub struct Pairing {
    expected: JoinToken,
    state: PairingState,
}

impl Pairing {
    /// Begin a pairing for a discovered node, with the hub's expected token.
    #[must_use]
    pub fn begin(expected: JoinToken) -> Self {
        Self {
            expected,
            state: PairingState::Discovered,
        }
    }

    /// The current state.
    #[must_use]
    pub fn state(&self) -> &PairingState {
        &self.state
    }

    /// Whether the handshake has reached a terminal state.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self.state, PairingState::Joined | PairingState::Rejected(_))
    }

    /// Record that the joining node offered a token. Only valid from
    /// [`PairingState::Discovered`]; a no-op from any other state (the state
    /// machine never moves backwards).
    pub fn offer_token(&mut self) {
        if self.state == PairingState::Discovered {
            self.state = PairingState::TokenOffered;
        }
    }

    /// Verify the offered token (value + validity at `now`) and advance to
    /// [`PairingState::Joined`] or [`PairingState::Rejected`]. Only acts from
    /// [`PairingState::TokenOffered`].
    ///
    /// Returns the resulting state for convenience.
    pub fn verify(&mut self, offered: &str, now: u64) -> &PairingState {
        if self.state != PairingState::TokenOffered {
            return &self.state;
        }
        self.state = if !self.expected.matches(offered) {
            PairingState::Rejected(RejectReason::TokenMismatch)
        } else if !self.expected.is_valid_at(now) {
            PairingState::Rejected(RejectReason::TokenExpired)
        } else {
            PairingState::Joined
        };
        &self.state
    }

    /// Reject the pairing outright (e.g. the discovery layer found the node on
    /// an incompatible version, [`crate::compat`]). Terminal from any
    /// non-terminal state.
    pub fn reject(&mut self, reason: RejectReason) {
        if !self.is_terminal() {
            self.state = PairingState::Rejected(reason);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good_token() -> JoinToken {
        // 12 chars, all from the charset.
        JoinToken::parse("ABCDEFGHJKMN", 0, 300).expect("valid token")
    }

    #[test]
    fn parses_valid_token() {
        let t = good_token();
        assert_eq!(t.value(), "ABCDEFGHJKMN");
    }

    #[test]
    fn rejects_wrong_length() {
        assert_eq!(
            JoinToken::parse("ABC", 0, 300),
            Err(TokenError::WrongLength { got: 3 })
        );
    }

    #[test]
    fn rejects_bad_character() {
        // 'I' is intentionally excluded from the charset.
        assert_eq!(
            JoinToken::parse("ABCDEFGHJKMI", 0, 300),
            Err(TokenError::BadCharacter('I'))
        );
        // lower-case is out of charset too.
        assert_eq!(
            JoinToken::parse("abcdefghjkmn", 0, 300),
            Err(TokenError::BadCharacter('a'))
        );
    }

    #[test]
    fn rejects_zero_lifetime() {
        assert_eq!(
            JoinToken::parse("ABCDEFGHJKMN", 0, 0),
            Err(TokenError::ZeroLifetime)
        );
    }

    #[test]
    fn validity_window_uses_clock() {
        let t = JoinToken::parse("ABCDEFGHJKMN", 100, 200).expect("valid");
        assert!(!t.is_valid_at(99));
        assert!(t.is_valid_at(100));
        assert!(t.is_valid_at(300));
        assert!(!t.is_valid_at(301));
    }

    #[test]
    fn happy_path_reaches_joined() {
        let mut p = Pairing::begin(good_token());
        assert_eq!(p.state(), &PairingState::Discovered);
        p.offer_token();
        assert_eq!(p.state(), &PairingState::TokenOffered);
        p.verify("ABCDEFGHJKMN", 10);
        assert_eq!(p.state(), &PairingState::Joined);
        assert!(p.is_terminal());
    }

    #[test]
    fn token_mismatch_is_rejected() {
        let mut p = Pairing::begin(good_token());
        p.offer_token();
        p.verify("WRONGTOKEN23", 10);
        assert_eq!(
            p.state(),
            &PairingState::Rejected(RejectReason::TokenMismatch)
        );
    }

    #[test]
    fn expired_token_is_rejected() {
        let mut p = Pairing::begin(JoinToken::parse("ABCDEFGHJKMN", 0, 50).expect("valid"));
        p.offer_token();
        p.verify("ABCDEFGHJKMN", 100); // past issued_at+lifetime=50
        assert_eq!(
            p.state(),
            &PairingState::Rejected(RejectReason::TokenExpired)
        );
    }

    #[test]
    fn verify_is_noop_before_token_offered() {
        let mut p = Pairing::begin(good_token());
        // Skipping offer_token: verify must not advance from Discovered.
        p.verify("ABCDEFGHJKMN", 10);
        assert_eq!(p.state(), &PairingState::Discovered);
    }

    #[test]
    fn version_incompatible_rejects_from_any_stage() {
        let mut p = Pairing::begin(good_token());
        p.reject(RejectReason::VersionIncompatible);
        assert_eq!(
            p.state(),
            &PairingState::Rejected(RejectReason::VersionIncompatible)
        );
        assert!(p.is_terminal());
    }

    #[test]
    fn reject_does_not_override_terminal_joined() {
        let mut p = Pairing::begin(good_token());
        p.offer_token();
        p.verify("ABCDEFGHJKMN", 10);
        p.reject(RejectReason::TokenMismatch);
        assert_eq!(p.state(), &PairingState::Joined, "joined is terminal");
    }

    #[test]
    fn token_matches_compares_value() {
        let t = good_token();
        assert!(t.matches("ABCDEFGHJKMN"));
        assert!(!t.matches("ABCDEFGHJKMM"));
    }
}
