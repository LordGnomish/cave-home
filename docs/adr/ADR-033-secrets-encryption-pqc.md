# ADR-033 — Secrets encryption-at-rest with a post-quantum envelope

## Status

**Accepted** — 2026-06-07. Implemented in `cave-home-orchestration`
(`src/secrets_encryption`) under the Cave Runtime PQC-ready mandate. The
decision core ships; the live datastore/apiserver wiring is ADR-004
phase-1b (see `parity.manifest.toml` `[[unmapped]]`).

## Context

K3s / Kubernetes encrypt `Secret` (and any configured resource) **at
rest** in the datastore by running every stored value through a chain of
*providers* before write and after read
(`apiserver/pkg/storage/value/encrypt`). The strong mode is **envelope
KMS v2**: a per-object data-encryption key (DEK) encrypts the value, and
a key-encryption key (KEK) — held in an external KMS — wraps the DEK.
K3s adds a key-rotation lifecycle on top (`k3s secrets-encrypt
prepare|rotate|reencrypt|prune|rotate-keys`).

cave-home needs the same at-rest protection for the Secrets its embedded
control plane stores, but under three cave-home constraints:

1. **No external KMS.** cave-home is a self-hosted single binary
   (Charter §5). There is no cloud KMS to hold the KEK.
2. **PQC-ready mandate (no-backcompat).** The Cave Runtime PQC-ready
   rule carries to cave-home: new key-wrapping must be quantum-resistant.
   Shipping a classical-only KEK (RSA/ECDH) wrap that we would have to
   rip out later violates the no-backcompat stance (Charter §8).
3. **cave-runtime ↔ cave-home isolation.** cave-home may not depend on
   `cave-runtime`'s `cave-core/crypto`; it carries its own crypto deps.

## Decision

Implement the encryption-at-rest **decision core** in
`cave-home-orchestration::secrets_encryption`, with the KEK realised as a
**post-quantum key-encapsulation mechanism** instead of an external KMS.

The envelope, per object:

1. a fresh random **AES-256-GCM** DEK encrypts the plaintext;
2. **ML-KEM-768** (FIPS 203) *encapsulates* to the KEK public key,
   yielding a shared secret + KEM ciphertext;
3. **HKDF-SHA256** derives a wrapping key from the shared secret, and
   **AES-256-GCM** wraps the DEK under it;
4. the stored blob is `magic ‖ kem_ct ‖ wrap_nonce ‖ wrapped_dek ‖
   data_nonce ‖ data_ct`, self-describing and fixed-layout.

Decryption ML-KEM-*decapsulates* with the KEK private key to recover the
shared secret, unwraps the DEK, and AES-GCM-decrypts the data. Both AEAD
layers authenticate the key id as AAD, so a blob only opens under the key
id that wrote it.

On top of the envelope: a write-key/read-keys **keyring** with the K3s
rotation lifecycle as an explicit phase machine (Steady → Prepared →
Rotated → Steady); an object-safe **KMS provider** trait
(`Encrypt`/`Decrypt`/`Status`, the upstream `EnvelopeService` seam) with
an in-process implementation; a prefixed stored-value **transformer**
(`cave:enc:mlkem768:v1:<key-id>:…`, mirroring `k8s:enc:…`) with an
identity read fallback; the **EncryptionConfiguration** model; and the
**status** + **observability** data contracts.

### PQC algorithm choice rationale

- **ML-KEM-768 (FIPS 203)** is the NIST-standardised module-lattice KEM
  at **Category 3** (≈AES-192) security — the balanced parameter set, and
  the one most deployments (incl. TLS hybrid drafts) default to. ML-KEM-512
  (Cat 1) is weaker than our AES-256 data layer warrants; ML-KEM-1024
  (Cat 5) costs larger keys/ciphertexts (1568/1568 B vs 1184/1088 B) for
  protection beyond what the rest of the system targets. 768 is the
  right balance for hub-stored Secrets.
- **KEM-based envelope, not a PQC signature or classical KDH.** Key
  *wrapping* is an encapsulation problem; a KEM is the direct fit. ML-DSA
  (FIPS 204) is for *signatures* (a different concern — provenance, not
  wrapping) and is not used here. A classical ECDH/RSA wrap is rejected by
  the PQC-ready mandate.
- **AES-256-GCM data + HKDF-SHA256.** The data layer and the DEK-wrap use
  AES-256-GCM (FIPS-grade AEAD, hardware-accelerated on the Linux 7.1
  target); the 256-bit DEK pairs with ML-KEM-768's ≥192-bit KEM so the
  wrap is not the weak link. HKDF-SHA256 domain-separates the wrapping key
  from the raw shared secret.
- **Pure-Rust, audited deps, no `cave-core`.** `ml-kem` (RustCrypto, FIPS
  203), `aes-gcm`, `hkdf`, `sha2`, `zeroize` — all `#![forbid(unsafe)]`-
  compatible, no OpenSSL/C, satisfying the cave-runtime ↔ cave-home
  isolation rule. Private key material is held as the compact 64-byte
  ML-KEM seed and zeroized on drop.

### 4-track completion

- **Backend** — `cave-home-orchestration::secrets_encryption`: envelope,
  keyring + rotation, provider, transformer, config, status, metrics.
  127 unit tests, strict TDD (test→fail→impl→pass, paired commits),
  clippy `--lib` clean, `honest_ratio = 1.00`.
- **cavectl** — `cave-home-cli::secrets_encryption`: `orchestration
  secrets encryption status` + `rotate-keys`, rendering the real
  `EncryptionStatus` view-model (not a stub). Hidden-infra surface
  (ADR-007): not wired into the end-user command tree.
- **Portal** — the *data contract* for "Security > Encryption" (key
  versions, last rotation, algorithm, rotation phase) is delivered as
  `status::EncryptionStatus`. The page rendering binds in the Portal crate
  when the orchestration runtime is live (phase-1b), consistent with the
  sibling hidden-infra k3s crates whose Portal pages are intentionally
  deferred.
- **Observability** — `metrics::EncryptionMetrics`: op-latency histogram,
  decryption error rate, per-key age gauges, Prometheus text exposition.

## Consequences

### Accepted gains

- Secrets are protected at rest with NIST-PQC key wrapping from day one;
  no classical-crypto migration debt (no-backcompat honoured).
- The provider trait is the seam a future gRPC KMS plugin implements
  without touching the envelope or keyring.
- The crate stays pure logic + self-contained crypto: no network, no
  clock, no `cave-core` dependency; fully unit-testable.

### Accepted costs

- The crate gains crypto dependencies (`aes-gcm`, `ml-kem`, `hkdf`,
  `sha2`, `zeroize`, `rand`) — the one documented exception to its
  otherwise std-only/no-crypto scope.
- The envelope is ~1.2 KB of fixed overhead per object (ML-KEM-768
  ciphertext 1088 B + wrapped DEK 48 B + nonces/magic). Acceptable for
  Secrets (small, infrequent); not intended for bulk data.
- The live storage wiring, gRPC plugin transport, keyring persistence,
  and cross-node re-encrypt controller are deferred to phase-1b and are
  enumerated as `[[unmapped]]`.

### Charter §6.3 / ADR-007 compliance

Infrastructure, hidden from end users: no home-world vocabulary, no i18n.
Surfaced only via the power-user `cavehomectl` path and Portal's advanced
Security section.

## Alternatives considered

- **(a) Classical KMS-v2 wrap (RSA-OAEP / ECDH-ES).** Rejected — violates
  the PQC-ready / no-backcompat mandate; would require a forced rotation
  to PQC later.
- **(b) Hybrid X25519 + ML-KEM-768 wrap.** Considered (defence-in-depth
  against a flaw in either primitive). Deferred, not rejected: it doubles
  the KEM machinery for a self-hosted hub whose threat model is local
  datastore theft, where a single NIST-standard KEM is sufficient. Can be
  added behind the same `KmsProvider` seam if the threat model changes.
- **(c) `aescbc`/`secretbox` symmetric-only providers (k8s legacy).**
  Rejected — a single static symmetric key with no KEK envelope is exactly
  what KMS-v2 + rotation exists to replace.
- **(d) Depend on `cave-runtime`'s `cave-core/crypto`.** Rejected — breaks
  the cave-runtime ↔ cave-home isolation rule; cave-home owns its crypto
  deps.

## Notes

The port method is a **behavioural reimplementation** of the documented
k8s envelope-encryption contract and the k3s secrets-encrypt lifecycle,
not a verbatim Go transcription (consistent with the rest of
`cave-home-orchestration`). The cryptographic substitution (external KMS
→ in-process ML-KEM-768) is a cave-home design decision, recorded here so
it is not silently re-litigated.
