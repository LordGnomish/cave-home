//! `secrets_encryption` — K3s / Kubernetes **secrets encryption-at-rest**,
//! reimplemented as a pure-logic decision core with a **post-quantum** envelope
//! (ADR-004 phase-1b; ADR-033).
//!
//! # What this is
//!
//! Kubernetes encrypts `Secret` (and any configured resource) **at rest** in the
//! datastore by running every stored value through a chain of *providers* before
//! it is written and after it is read. The upstream shape (k8s
//! `apiserver/pkg/storage/value/encrypt`) is:
//!
//! - an [`config::EncryptionConfiguration`] maps **resources** → an ordered list
//!   of **providers**;
//! - the **first** provider's **first** key is the *write key* (used to encrypt);
//!   every key of every provider is a *read key* (tried, by key id, on decrypt);
//! - each stored blob is tagged with a transformer prefix
//!   (`cave:enc:mlkem768:v1:<key-id>:…`, mirroring k8s `k8s:enc:…`) so the read
//!   path can route a value to the provider + key that wrote it — [`transformer`];
//! - an **identity** provider (no-op) is the disabled-encryption fallback and is
//!   always a valid *read* provider so a cluster can be migrated in or out of
//!   encryption without data loss.
//!
//! K3s layers a **key-rotation lifecycle** on top (`k3s secrets-encrypt
//! prepare|rotate|reencrypt|rotate-keys`): a new key is *prepared* (appended as a
//! read-only key), then *rotated* to the front (made the write key), then all
//! secrets are *re-encrypted* and the stale keys *pruned*. That write-key /
//! read-keys state machine is [`keyring`].
//!
//! # The post-quantum twist (ADR-033)
//!
//! Upstream KMS v2 wraps the per-object data-encryption key (DEK) with a
//! key-encryption key (KEK) held in an external KMS. cave-home is a single
//! self-hosted binary with **no external KMS**, and the Cave Runtime *PQC-ready*
//! mandate forbids shipping classical-only key wrapping. So the KEK here is an
//! **ML-KEM-768** (FIPS 203) key pair and the envelope is:
//!
//! 1. a fresh random **AES-256-GCM** DEK encrypts the plaintext;
//! 2. **ML-KEM-768** encapsulates to the KEK public key, yielding a shared
//!    secret + KEM ciphertext;
//! 3. **HKDF-SHA256** derives a wrapping key from the shared secret, and
//!    **AES-256-GCM** wraps the DEK under it;
//! 4. the stored blob carries KEM-ciphertext ‖ wrapped-DEK ‖ data-ciphertext.
//!
//! Decryption ML-KEM-*decapsulates* with the KEK private key to recover the
//! shared secret, unwraps the DEK, and AES-GCM-decrypts the data. This is
//! genuine NIST-PQC envelope encryption (AES-256-GCM data key + ML-KEM-768 KEK
//! wrap) — see [`envelope`].
//!
//! # Scope (honest)
//!
//! Self-contained and fully tested: the PQC envelope crypto, the write/read
//! keyring + rotation lifecycle, the in-process KMS provider, the prefixed
//! transformer + identity fallback, the encryption-configuration model, and the
//! status / observability data contracts. **Out of scope** (network/runtime,
//! ADR-004 phase-1b): the gRPC KMS-plugin transport, the live apiserver storage
//! wiring, persisting the keyring to the datastore, and the controller that
//! drives a cluster-wide re-encrypt. This crate decides; the runtime executes.
//!
//! This is **infrastructure**, hidden from end users (Charter §6.3, ADR-007).

pub mod envelope;
pub mod keyring;
