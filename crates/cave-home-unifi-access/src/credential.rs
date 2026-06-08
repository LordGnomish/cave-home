// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//
//! Access credential model (ADR-009).
//!
//! `UniFi` Access admits a person by one of several credential kinds — a keypad
//! PIN, an NFC card, a phone (mobile/Bluetooth), or a wave-to-unlock gesture.
//! This module models the *validated value object* for each, never leaks the
//! secret part via [`std::fmt::Debug`], and gives a constant-time-ish compare so
//! a reader attacker cannot learn a PIN one digit at a time from timing.
//!
//! # Security stance (Phase 1 vs Phase 1b)
//!
//! Like `cave-home-lock`, cave-home does **not** store plaintext PINs. The
//! stored form is an opaque [`CredentialDigest`] plus a compare that runs in
//! time independent of where two digests first differ. The digest here is a
//! deliberately simple, dependency-free fold — it is **not** a cryptographic
//! hash and must not be relied on for at-rest secrecy. Real password-grade
//! hashing (Argon2id / scrypt + per-reader salt + a constant-time crate) is a
//! Phase-1b adapter concern, enumerated in `parity.manifest.toml` against
//! ADR-009. This crate owns the *contract*; the adapter swaps in the real
//! primitive behind the same shape.

/// Minimum keypad PIN length we accept. Four digits is the residential floor.
pub const MIN_PIN_LEN: usize = 4;

/// Maximum keypad PIN length we accept. Generous bound that still rejects
/// unbounded input.
pub const MAX_PIN_LEN: usize = 8;

/// Minimum hex characters in an NFC card id (a 4-byte UID = 8 hex chars).
pub const MIN_CARD_HEX: usize = 8;

/// Maximum hex characters in an NFC card id (a 7-byte UID = 14 hex chars).
pub const MAX_CARD_HEX: usize = 14;

/// Which kind of credential a person presented.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CredentialKind {
    /// A keypad PIN.
    Pin,
    /// An NFC card / fob.
    NfcCard,
    /// A phone (mobile app / Bluetooth).
    Mobile,
    /// A wave-to-unlock hand gesture at the reader.
    WaveToUnlock,
}

/// Why a [`Credential`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialError {
    /// The PIN was shorter than [`MIN_PIN_LEN`].
    PinTooShort,
    /// The PIN was longer than [`MAX_PIN_LEN`].
    PinTooLong,
    /// The PIN contained a non-digit character.
    PinNotDigits,
    /// The card id was outside [`MIN_CARD_HEX`]..=[`MAX_CARD_HEX`] characters.
    CardWrongLength,
    /// The card id contained a non-hex character.
    CardNotHex,
    /// A mobile token / wave subject was empty.
    EmptyIdentifier,
}

impl core::fmt::Display for CredentialError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::PinTooShort => f.write_str("PIN is too short"),
            Self::PinTooLong => f.write_str("PIN is too long"),
            Self::PinNotDigits => f.write_str("PIN contains a non-digit character"),
            Self::CardWrongLength => f.write_str("card id is the wrong length"),
            Self::CardNotHex => f.write_str("card id contains a non-hex character"),
            Self::EmptyIdentifier => f.write_str("credential identifier is empty"),
        }
    }
}

impl std::error::Error for CredentialError {}

/// A validated credential a person can present at a door.
///
/// Construction enforces the per-kind invariants up front so the rest of the
/// system never has to defend against a malformed PIN or card id. The secret
/// material is held only long enough to digest it; it is never exposed back out
/// and never lands in a log line via `Debug`.
#[derive(Clone, PartialEq, Eq)]
pub struct Credential {
    kind: CredentialKind,
    /// The validated secret/identifier bytes. Private; only [`Credential::digest`]
    /// ever reads it, and `Debug` never renders it.
    material: Vec<u8>,
}

// Hand-written so credential material never lands in a log line via `{:?}`.
impl core::fmt::Debug for Credential {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Credential")
            .field("kind", &self.kind)
            .field("len", &self.material.len())
            .finish_non_exhaustive()
    }
}

impl Credential {
    /// Validate and construct a keypad-PIN credential.
    ///
    /// # Errors
    /// [`CredentialError`] if the PIN is outside
    /// [`MIN_PIN_LEN`]..=[`MAX_PIN_LEN`] or contains a non-digit.
    pub fn pin(raw: &str) -> Result<Self, CredentialError> {
        if raw.len() < MIN_PIN_LEN {
            return Err(CredentialError::PinTooShort);
        }
        if raw.len() > MAX_PIN_LEN {
            return Err(CredentialError::PinTooLong);
        }
        for b in raw.bytes() {
            if !b.is_ascii_digit() {
                return Err(CredentialError::PinNotDigits);
            }
        }
        Ok(Self {
            kind: CredentialKind::Pin,
            material: raw.as_bytes().to_vec(),
        })
    }

    /// Validate and construct an NFC-card credential from its hex UID.
    ///
    /// # Errors
    /// [`CredentialError`] if the id is the wrong length or not hex.
    pub fn nfc_card(uid_hex: &str) -> Result<Self, CredentialError> {
        let len = uid_hex.len();
        if !(MIN_CARD_HEX..=MAX_CARD_HEX).contains(&len) {
            return Err(CredentialError::CardWrongLength);
        }
        for b in uid_hex.bytes() {
            if !b.is_ascii_hexdigit() {
                return Err(CredentialError::CardNotHex);
            }
        }
        // Normalise to lower-case so "AB" and "ab" are the same card.
        Ok(Self {
            kind: CredentialKind::NfcCard,
            material: uid_hex.to_ascii_lowercase().into_bytes(),
        })
    }

    /// Validate and construct a mobile (phone/Bluetooth) credential from its
    /// opaque token.
    ///
    /// # Errors
    /// [`CredentialError::EmptyIdentifier`] if the token is empty.
    pub fn mobile(token: &str) -> Result<Self, CredentialError> {
        if token.is_empty() {
            return Err(CredentialError::EmptyIdentifier);
        }
        Ok(Self {
            kind: CredentialKind::Mobile,
            material: token.as_bytes().to_vec(),
        })
    }

    /// Validate and construct a wave-to-unlock credential bound to a subject
    /// identifier (the enrolled person the reader recognises).
    ///
    /// # Errors
    /// [`CredentialError::EmptyIdentifier`] if the subject is empty.
    pub fn wave_to_unlock(subject: &str) -> Result<Self, CredentialError> {
        if subject.is_empty() {
            return Err(CredentialError::EmptyIdentifier);
        }
        Ok(Self {
            kind: CredentialKind::WaveToUnlock,
            material: subject.as_bytes().to_vec(),
        })
    }

    /// The credential kind.
    #[must_use]
    pub fn kind(&self) -> CredentialKind {
        self.kind
    }

    /// Reduce this credential to an opaque, storable digest.
    ///
    /// See the module docs: this is a dependency-free fold, **not** a
    /// cryptographic hash. The kind is mixed in so an NFC id can never collide
    /// with a PIN of the same bytes. The adapter swaps in real hashing in
    /// Phase 1b.
    #[must_use]
    pub fn digest(&self) -> CredentialDigest {
        CredentialDigest::of(self.kind, &self.material)
    }
}

/// An opaque, storable representation of a credential. Comparable in constant
/// time but not reversible to the original material by this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CredentialDigest([u8; 8]);

impl CredentialDigest {
    /// Fold a kind tag + byte slice into a fixed-width digest. Order- and
    /// length-sensitive so distinct credentials collide only by accident.
    fn of(kind: CredentialKind, bytes: &[u8]) -> Self {
        // FNV-1a-style 64-bit fold, split into 8 bytes. Deterministic and
        // std-only; the Phase-1b adapter replaces this with a salted KDF.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mix = |h: &mut u64, b: u8| {
            *h ^= u64::from(b);
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
        };
        // Domain-separate by kind so identical bytes under different kinds
        // digest differently.
        mix(&mut hash, kind as u8 + 1);
        for &b in bytes {
            mix(&mut hash, b);
        }
        // Mix the length in so a prefix cannot trivially align with a longer
        // secret.
        hash ^= bytes.len() as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        Self(hash.to_le_bytes())
    }

    /// Compare two digests in time independent of where they first differ.
    ///
    /// Both digests are the same fixed width, so this loops a constant number
    /// of times and accumulates differences without an early return — a reader
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

/// An enrolled credential stored against a person, with brute-force accounting.
///
/// Holds only the digest, counts consecutive failures, and refuses further
/// attempts once a threshold is crossed — the keypad lock-out that stops a
/// brute-force walk of every PIN.
#[derive(Debug, Clone)]
pub struct EnrolledCredential {
    kind: CredentialKind,
    expected: CredentialDigest,
    failures: u32,
    max_failures: u32,
}

/// The outcome of presenting a credential against an enrolled one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialVerdict {
    /// The presented credential matched.
    Accepted,
    /// The presented credential did not match.
    Rejected,
    /// Too many wrong attempts; locked out until reset.
    LockedOut,
}

impl EnrolledCredential {
    /// Consecutive wrong presentations allowed before lock-out, by default.
    pub const DEFAULT_MAX_FAILURES: u32 = 5;

    /// Enroll a credential using the default lock-out threshold.
    #[must_use]
    pub fn enroll(credential: &Credential) -> Self {
        Self::enroll_with_limit(credential, Self::DEFAULT_MAX_FAILURES)
    }

    /// Enroll a credential with an explicit consecutive-failure threshold.
    #[must_use]
    pub fn enroll_with_limit(credential: &Credential, max_failures: u32) -> Self {
        Self {
            kind: credential.kind(),
            expected: credential.digest(),
            failures: 0,
            max_failures,
        }
    }

    /// The credential kind this enrolment expects.
    #[must_use]
    pub fn kind(&self) -> CredentialKind {
        self.kind
    }

    /// Whether the enrolment is currently locked out.
    #[must_use]
    pub const fn is_locked_out(&self) -> bool {
        self.failures >= self.max_failures
    }

    /// Consecutive failed attempts since the last acceptance / reset.
    #[must_use]
    pub const fn failure_count(&self) -> u32 {
        self.failures
    }

    /// Verify a presented credential.
    ///
    /// A credential of a different kind is always [`CredentialVerdict::Rejected`]
    /// (a card can never satisfy a PIN enrolment). On a correct match the
    /// failure counter resets; on a wrong attempt it increments and, once the
    /// threshold is reached, every further attempt — correct or not — is refused
    /// with [`CredentialVerdict::LockedOut`] until [`EnrolledCredential::reset`].
    pub fn verify(&mut self, presented: &Credential) -> CredentialVerdict {
        if self.is_locked_out() {
            return CredentialVerdict::LockedOut;
        }
        let matches =
            presented.kind() == self.kind && presented.digest().ct_eq(&self.expected);
        if matches {
            self.failures = 0;
            CredentialVerdict::Accepted
        } else {
            self.failures = self.failures.saturating_add(1);
            if self.is_locked_out() {
                CredentialVerdict::LockedOut
            } else {
                CredentialVerdict::Rejected
            }
        }
    }

    /// Clear a lock-out and reset the failure counter (administrator override
    /// or cool-down elapsed).
    pub fn reset(&mut self) {
        self.failures = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_rejects_short_long_and_non_digit() {
        assert_eq!(Credential::pin("123"), Err(CredentialError::PinTooShort));
        assert_eq!(Credential::pin("123456789"), Err(CredentialError::PinTooLong));
        assert_eq!(Credential::pin("12a4"), Err(CredentialError::PinNotDigits));
    }

    #[test]
    fn pin_accepts_boundaries() {
        assert!(Credential::pin("1234").is_ok());
        assert!(Credential::pin("12345678").is_ok());
    }

    #[test]
    fn card_validation() {
        assert!(Credential::nfc_card("0a1b2c3d").is_ok()); // 8 hex
        assert!(Credential::nfc_card("0a1b2c3d4e5f6a").is_ok()); // 14 hex
        assert_eq!(Credential::nfc_card("0a1b"), Err(CredentialError::CardWrongLength));
        assert_eq!(
            Credential::nfc_card("0a1b2c3d4e5f6a7b"),
            Err(CredentialError::CardWrongLength)
        );
        assert_eq!(Credential::nfc_card("0a1b2c3g"), Err(CredentialError::CardNotHex));
    }

    #[test]
    fn card_is_case_insensitive() {
        let upper = Credential::nfc_card("0A1B2C3D").expect("valid");
        let lower = Credential::nfc_card("0a1b2c3d").expect("valid");
        assert!(upper.digest().ct_eq(&lower.digest()));
    }

    #[test]
    fn mobile_and_wave_reject_empty() {
        assert_eq!(Credential::mobile(""), Err(CredentialError::EmptyIdentifier));
        assert_eq!(
            Credential::wave_to_unlock(""),
            Err(CredentialError::EmptyIdentifier)
        );
        assert!(Credential::mobile("phone-token-xyz").is_ok());
        assert!(Credential::wave_to_unlock("person-7").is_ok());
    }

    #[test]
    fn debug_never_reveals_material() {
        let c = Credential::pin("4242").expect("valid");
        let rendered = format!("{c:?}");
        assert!(!rendered.contains("4242"), "PIN leaked into Debug: {rendered}");
        assert!(rendered.contains("Pin"));
        assert!(rendered.contains("len"));
    }

    #[test]
    fn matching_pins_digest_equal() {
        let a = Credential::pin("9182").expect("valid").digest();
        let b = Credential::pin("9182").expect("valid").digest();
        assert!(a.ct_eq(&b));
    }

    #[test]
    fn different_pins_digest_differ() {
        let a = Credential::pin("9182").expect("valid").digest();
        let b = Credential::pin("9183").expect("valid").digest();
        assert!(!a.ct_eq(&b));
    }

    #[test]
    fn pin_digest_is_order_sensitive() {
        let a = Credential::pin("1234").expect("valid").digest();
        let b = Credential::pin("4321").expect("valid").digest();
        assert!(!a.ct_eq(&b), "digit order must matter");
    }

    #[test]
    fn kind_is_domain_separated_in_digest() {
        // A PIN and a card that happen to share bytes must not collide.
        let pin = Credential::pin("12345678").expect("valid");
        let card = Credential::nfc_card("12345678").expect("valid");
        assert!(!pin.digest().ct_eq(&card.digest()));
    }

    #[test]
    fn correct_credential_accepted() {
        let stored = Credential::pin("2468").expect("valid");
        let mut enrolled = EnrolledCredential::enroll(&stored);
        let attempt = Credential::pin("2468").expect("valid");
        assert_eq!(enrolled.verify(&attempt), CredentialVerdict::Accepted);
    }

    #[test]
    fn wrong_credential_rejected_and_counts() {
        let stored = Credential::pin("2468").expect("valid");
        let mut enrolled = EnrolledCredential::enroll(&stored);
        let attempt = Credential::pin("0000").expect("valid");
        assert_eq!(enrolled.verify(&attempt), CredentialVerdict::Rejected);
        assert_eq!(enrolled.failure_count(), 1);
    }

    #[test]
    fn mismatched_kind_is_rejected() {
        let stored = Credential::pin("12345678").expect("valid");
        let mut enrolled = EnrolledCredential::enroll(&stored);
        let card = Credential::nfc_card("12345678").expect("valid");
        assert_eq!(
            enrolled.verify(&card),
            CredentialVerdict::Rejected,
            "a card must never satisfy a PIN enrolment"
        );
    }

    #[test]
    fn correct_credential_resets_failures() {
        let stored = Credential::pin("2468").expect("valid");
        let mut enrolled = EnrolledCredential::enroll(&stored);
        let wrong = Credential::pin("0000").expect("valid");
        let right = Credential::pin("2468").expect("valid");
        assert_eq!(enrolled.verify(&wrong), CredentialVerdict::Rejected);
        assert_eq!(enrolled.failure_count(), 1);
        assert_eq!(enrolled.verify(&right), CredentialVerdict::Accepted);
        assert_eq!(enrolled.failure_count(), 0);
    }

    #[test]
    fn locks_out_after_threshold() {
        let stored = Credential::pin("2468").expect("valid");
        let mut enrolled = EnrolledCredential::enroll_with_limit(&stored, 3);
        let wrong = Credential::pin("0000").expect("valid");
        assert_eq!(enrolled.verify(&wrong), CredentialVerdict::Rejected);
        assert_eq!(enrolled.verify(&wrong), CredentialVerdict::Rejected);
        assert_eq!(enrolled.verify(&wrong), CredentialVerdict::LockedOut);
        assert!(enrolled.is_locked_out());
    }

    #[test]
    fn locked_out_refuses_even_correct_credential() {
        let stored = Credential::pin("2468").expect("valid");
        let mut enrolled = EnrolledCredential::enroll_with_limit(&stored, 2);
        let wrong = Credential::pin("0000").expect("valid");
        let right = Credential::pin("2468").expect("valid");
        enrolled.verify(&wrong);
        enrolled.verify(&wrong); // now locked out
        assert_eq!(
            enrolled.verify(&right),
            CredentialVerdict::LockedOut,
            "a brute-force lock-out must not be bypassed by guessing right after"
        );
    }

    #[test]
    fn reset_clears_lockout() {
        let stored = Credential::pin("2468").expect("valid");
        let mut enrolled = EnrolledCredential::enroll_with_limit(&stored, 1);
        let wrong = Credential::pin("0000").expect("valid");
        let right = Credential::pin("2468").expect("valid");
        enrolled.verify(&wrong); // locked out
        assert!(enrolled.is_locked_out());
        enrolled.reset();
        assert!(!enrolled.is_locked_out());
        assert_eq!(enrolled.verify(&right), CredentialVerdict::Accepted);
    }
}
