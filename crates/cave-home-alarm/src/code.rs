//! User-code model for the alarm panel.
//!
//! An alarm keypad accepts a numeric user code to arm and disarm. This module
//! models the *value object* (a validated, bounded code) and the *verification
//! contract* — comparing a presented code against a stored credential — without
//! ever holding a plaintext code to compare against.
//!
//! # Security stance (Phase 1 vs Phase 1b)
//!
//! cave-home does **not** store plaintext user codes. The stored credential
//! here is an opaque digest ([`CodeDigest`]) plus a comparison that runs in
//! time independent of how many leading bytes happen to match, so a keypad
//! attacker cannot learn a code one digit at a time from timing.
//!
//! The digest in *this* crate is a deliberately simple, dependency-free folding
//! function — it is **not** a cryptographic hash and must not be relied on for
//! at-rest secrecy. Real password-grade hashing (Argon2id / scrypt, per-panel
//! salt, a vetted constant-time crate) is a Phase-1b adapter concern,
//! enumerated in `parity.manifest.toml` against ADR-018. This crate owns the
//! *contract* (validate, digest, constant-time compare, lock-out semantics);
//! the adapter swaps in the real primitive behind the same shape.

/// Minimum user-code length we accept. Four digits is the residential keypad
/// floor (and the common factory default length).
pub const MIN_CODE_LEN: usize = 4;

/// Maximum user-code length we accept. Generous upper bound to fit any vendor
/// keypad while still rejecting unbounded input.
pub const MAX_CODE_LEN: usize = 12;

/// Why a [`UserCode`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeError {
    /// The code was empty.
    Empty,
    /// The code was shorter than [`MIN_CODE_LEN`].
    TooShort,
    /// The code was longer than [`MAX_CODE_LEN`].
    TooLong,
    /// The code contained a character that is not a keypad digit `0`–`9`.
    NotDigits,
}

impl core::fmt::Display for CodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => f.write_str("code is empty"),
            Self::TooShort => f.write_str("code is too short"),
            Self::TooLong => f.write_str("code is too long"),
            Self::NotDigits => f.write_str("code contains a non-digit character"),
        }
    }
}

impl std::error::Error for CodeError {}

/// A validated keypad user code.
///
/// Construction enforces the length and digit-only invariants up front, so the
/// rest of the system never has to defend against an empty or malformed code.
/// The code is held only long enough to digest it for storage or to verify a
/// presented attempt; it is never exposed back out as a string.
#[derive(Clone, PartialEq, Eq)]
pub struct UserCode {
    digits: Vec<u8>,
}

// Hand-written so a user code never lands in a log line via `{:?}`.
impl core::fmt::Debug for UserCode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UserCode")
            .field("len", &self.digits.len())
            .finish_non_exhaustive()
    }
}

impl UserCode {
    /// Validate and construct a user code from a string of keypad digits.
    ///
    /// # Errors
    /// Returns [`CodeError`] if the code is empty, outside
    /// [`MIN_CODE_LEN`]..=[`MAX_CODE_LEN`], or contains a non-digit character.
    pub fn parse(raw: &str) -> Result<Self, CodeError> {
        if raw.is_empty() {
            return Err(CodeError::Empty);
        }
        if raw.len() < MIN_CODE_LEN {
            return Err(CodeError::TooShort);
        }
        if raw.len() > MAX_CODE_LEN {
            return Err(CodeError::TooLong);
        }
        let mut digits = Vec::with_capacity(raw.len());
        for b in raw.bytes() {
            if !b.is_ascii_digit() {
                return Err(CodeError::NotDigits);
            }
            digits.push(b);
        }
        Ok(Self { digits })
    }

    /// The number of digits in the code. Length is not secret.
    #[must_use]
    pub fn len(&self) -> usize {
        self.digits.len()
    }

    /// Whether the code has no digits. Always `false` for a constructed
    /// [`UserCode`] (kept for API completeness / clippy's `is_empty` lint).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.digits.is_empty()
    }

    /// Reduce this code to an opaque, storable digest.
    ///
    /// See the module docs: this is a dependency-free fold, **not** a
    /// cryptographic hash. The adapter swaps in real hashing in Phase 1b.
    #[must_use]
    pub fn digest(&self) -> CodeDigest {
        CodeDigest::of(&self.digits)
    }
}

/// An opaque, storable representation of a user code. Comparable in constant
/// time but not reversible to the original digits by this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeDigest([u8; 8]);

impl CodeDigest {
    /// Fold a byte slice into a fixed-width digest. Order-sensitive and
    /// length-sensitive so distinct codes collide only by accident, not by
    /// construction — but, again, this is not a security-grade hash.
    fn of(bytes: &[u8]) -> Self {
        // FNV-1a-style 64-bit fold, split into 8 bytes. Deterministic and
        // std-only; the Phase-1b adapter replaces this with a salted KDF.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in bytes {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        // Mix the length in so "12" and "1212..." cannot trivially align.
        hash ^= bytes.len() as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        Self(hash.to_le_bytes())
    }

    /// Compare two digests in time independent of where they first differ.
    ///
    /// Both digests are the same fixed width, so this loops a constant number
    /// of times and accumulates differences without an early return — a keypad
    /// attacker learns nothing from how long a rejection took.
    #[must_use]
    pub fn ct_eq(&self, other: &Self) -> bool {
        let mut diff: u8 = 0;
        for i in 0..self.0.len() {
            diff |= self.0[i] ^ other.0[i];
        }
        diff == 0
    }
}

/// The outcome of presenting a user code to the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeVerdict {
    /// The presented code matched the stored credential.
    Accepted,
    /// The presented code did not match.
    Rejected,
    /// The panel keypad is in a temporary lock-out after too many wrong
    /// attempts; no code is accepted until the lock-out clears.
    LockedOut,
}

/// Stored keypad credential plus failed-attempt accounting.
///
/// This is the safety logic around a user code: it never holds the plaintext
/// (only the digest), it counts consecutive failures, and it refuses further
/// attempts once a threshold is crossed — the classic keypad lock-out that
/// stops a brute-force walk of every code.
#[derive(Debug, Clone)]
pub struct CodeCredential {
    expected: CodeDigest,
    failures: u32,
    max_failures: u32,
}

impl CodeCredential {
    /// Number of consecutive wrong codes allowed before lock-out, by default.
    pub const DEFAULT_MAX_FAILURES: u32 = 5;

    /// Enroll a credential from a validated user code, using the default
    /// lock-out threshold.
    #[must_use]
    pub fn enroll(code: &UserCode) -> Self {
        Self::enroll_with_limit(code, Self::DEFAULT_MAX_FAILURES)
    }

    /// Enroll a credential with an explicit consecutive-failure threshold.
    #[must_use]
    pub fn enroll_with_limit(code: &UserCode, max_failures: u32) -> Self {
        Self {
            expected: code.digest(),
            failures: 0,
            max_failures,
        }
    }

    /// Whether the credential is currently locked out.
    #[must_use]
    pub const fn is_locked_out(&self) -> bool {
        self.failures >= self.max_failures
    }

    /// Consecutive failed attempts since the last acceptance / reset.
    #[must_use]
    pub const fn failure_count(&self) -> u32 {
        self.failures
    }

    /// Verify a presented user code.
    ///
    /// On a correct code the failure counter resets. On a wrong code it
    /// increments and, once the threshold is reached, every further attempt —
    /// correct or not — is refused with [`CodeVerdict::LockedOut`] until
    /// [`CodeCredential::reset`] is called.
    pub fn verify(&mut self, presented: &UserCode) -> CodeVerdict {
        if self.is_locked_out() {
            return CodeVerdict::LockedOut;
        }
        if presented.digest().ct_eq(&self.expected) {
            self.failures = 0;
            CodeVerdict::Accepted
        } else {
            self.failures = self.failures.saturating_add(1);
            if self.is_locked_out() {
                CodeVerdict::LockedOut
            } else {
                CodeVerdict::Rejected
            }
        }
    }

    /// Clear a lock-out and reset the failure counter (e.g. after an
    /// administrator override or a cool-down timer elapses).
    pub fn reset(&mut self) {
        self.failures = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_short() {
        assert_eq!(UserCode::parse(""), Err(CodeError::Empty));
        assert_eq!(UserCode::parse("12"), Err(CodeError::TooShort));
        assert_eq!(UserCode::parse("123"), Err(CodeError::TooShort));
    }

    #[test]
    fn accepts_minimum_length() {
        let c = UserCode::parse("1234").expect("4 digits is valid");
        assert_eq!(c.len(), 4);
        assert!(!c.is_empty());
    }

    #[test]
    fn rejects_too_long() {
        let long = "1234567890123"; // 13 digits
        assert_eq!(UserCode::parse(long), Err(CodeError::TooLong));
    }

    #[test]
    fn accepts_maximum_length() {
        let max = "123456789012"; // 12 digits
        assert!(UserCode::parse(max).is_ok());
    }

    #[test]
    fn rejects_non_digits() {
        assert_eq!(UserCode::parse("12a4"), Err(CodeError::NotDigits));
        assert_eq!(UserCode::parse("12 4"), Err(CodeError::NotDigits));
        assert_eq!(UserCode::parse("12#4"), Err(CodeError::NotDigits));
    }

    #[test]
    fn debug_never_reveals_digits() {
        let c = UserCode::parse("4242").expect("valid");
        let rendered = format!("{c:?}");
        assert!(!rendered.contains("4242"), "code leaked into Debug: {rendered}");
        assert!(rendered.contains("len"));
    }

    #[test]
    fn matching_codes_digest_equal() {
        let a = UserCode::parse("9182").expect("valid").digest();
        let b = UserCode::parse("9182").expect("valid").digest();
        assert!(a.ct_eq(&b));
    }

    #[test]
    fn different_codes_digest_differ() {
        let a = UserCode::parse("9182").expect("valid").digest();
        let b = UserCode::parse("9183").expect("valid").digest();
        assert!(!a.ct_eq(&b));
    }

    #[test]
    fn order_sensitive_digest() {
        let a = UserCode::parse("1234").expect("valid").digest();
        let b = UserCode::parse("4321").expect("valid").digest();
        assert!(!a.ct_eq(&b), "digit order must matter");
    }

    #[test]
    fn correct_code_accepted() {
        let stored = UserCode::parse("2468").expect("valid");
        let mut cred = CodeCredential::enroll(&stored);
        let attempt = UserCode::parse("2468").expect("valid");
        assert_eq!(cred.verify(&attempt), CodeVerdict::Accepted);
    }

    #[test]
    fn wrong_code_rejected() {
        let stored = UserCode::parse("2468").expect("valid");
        let mut cred = CodeCredential::enroll(&stored);
        let attempt = UserCode::parse("0000").expect("valid");
        assert_eq!(cred.verify(&attempt), CodeVerdict::Rejected);
        assert_eq!(cred.failure_count(), 1);
    }

    #[test]
    fn correct_code_resets_failures() {
        let stored = UserCode::parse("2468").expect("valid");
        let mut cred = CodeCredential::enroll(&stored);
        let wrong = UserCode::parse("0000").expect("valid");
        let right = UserCode::parse("2468").expect("valid");
        assert_eq!(cred.verify(&wrong), CodeVerdict::Rejected);
        assert_eq!(cred.verify(&wrong), CodeVerdict::Rejected);
        assert_eq!(cred.failure_count(), 2);
        assert_eq!(cred.verify(&right), CodeVerdict::Accepted);
        assert_eq!(cred.failure_count(), 0);
    }

    #[test]
    fn locks_out_after_threshold() {
        let stored = UserCode::parse("2468").expect("valid");
        let mut cred = CodeCredential::enroll_with_limit(&stored, 3);
        let wrong = UserCode::parse("0000").expect("valid");
        assert_eq!(cred.verify(&wrong), CodeVerdict::Rejected);
        assert_eq!(cred.verify(&wrong), CodeVerdict::Rejected);
        // Third failure crosses the threshold.
        assert_eq!(cred.verify(&wrong), CodeVerdict::LockedOut);
        assert!(cred.is_locked_out());
    }

    #[test]
    fn locked_out_refuses_even_correct_code() {
        let stored = UserCode::parse("2468").expect("valid");
        let mut cred = CodeCredential::enroll_with_limit(&stored, 2);
        let wrong = UserCode::parse("0000").expect("valid");
        let right = UserCode::parse("2468").expect("valid");
        cred.verify(&wrong);
        cred.verify(&wrong); // now locked out
        assert_eq!(
            cred.verify(&right),
            CodeVerdict::LockedOut,
            "a brute-force lock-out must not be bypassed by guessing right after"
        );
    }

    #[test]
    fn reset_clears_lockout() {
        let stored = UserCode::parse("2468").expect("valid");
        let mut cred = CodeCredential::enroll_with_limit(&stored, 1);
        let wrong = UserCode::parse("0000").expect("valid");
        let right = UserCode::parse("2468").expect("valid");
        cred.verify(&wrong); // locked out
        assert!(cred.is_locked_out());
        cred.reset();
        assert!(!cred.is_locked_out());
        assert_eq!(cred.verify(&right), CodeVerdict::Accepted);
    }
}
