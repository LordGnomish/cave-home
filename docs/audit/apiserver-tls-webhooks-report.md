# cave-home-apiserver-rs — TLS / mTLS + Admission-Webhook Round Report (2026-06-07)

**Branch:** `feature/apiserver-http-transport` (isolated worktree
`../cave-home-apiserver-transport`). This is **round 2** on top of the HTTP
transport foundation (prior tip `ac280e4`, report
`apiserver-http-transport-report.md`).
**Push:** none. **Merge:** local `--no-ff` only, on request.

## 1. Starting point

Round 1 shipped the full std-only HTTP transport: HTTP/1.1 codec, JSON body
parser, the `authn→authz→admission→storage→audit` handler chain, **all** REST
verbs (`get/list/create/update/patch/delete/watch`) over a real `TcpListener`,
chunked watch streams, audit, metrics, and the kine storage seam — **208 tests**.

So the briefed gaps were re-checked against reality first:

| Briefed task | Reality at round-2 start |
|---|---|
| JSON body parser | **already done** (`json::parse`, used by every POST/PUT/PATCH) |
| POST/PUT/PATCH/DELETE handlers | **already done + tested** (handler + real-TCP session) |
| Watch chunked stream (SSE) | **already done** (`?watch=true`, chunked `ADDED/…`) |
| Auth/Authz middleware chain | **already done** (bearer+anonymous authn, RBAC authz) |
| **TLS support (rustls)** | **genuinely deferred → built this round** |
| **Admission webhook plumbing** | **genuinely deferred → built this round** |

This round therefore built the two real gaps, plus the **x509 client-cert
authenticator** that makes mTLS mean something.

## 2. What shipped

| Module | Impl LOC | Tests | Notes |
|---|---:|---:|---|
| `webhook.rs` | ~576 | 14 | `admission.k8s.io/v1` AdmissionReview codec; `WebhookClient` seam (`MockWebhookClient` + std `http://` `HttpWebhookClient`); `WebhookMutating/ValidatingPlugin` on the existing `AdmissionChain`; base64 JSONPatch (applied via `patch.rs`); uid correlation; `FailurePolicy` Fail/Ignore |
| `tls.rs` *(feature `tls`)* | ~213 | 4 | rustls 0.23 (ring provider, **offline**); `server_config` / `server_config_mtls`; `TlsServer` + `serve_tls_stream` reuse the same `read_request→handle→write` flow over a rustls `StreamOwned`; verified client-cert → front-proxy headers |
| `x509.rs` | ~158 | 5 | std-only, total/panic-free DER walker → Subject `CN` (user) + each `O` (group); operates on raw `&[u8]` so it needs no rustls |
| `authn.rs` (+) | ~50 | 2 | `RequestHeaderAuthenticator` (X-Remote-User / repeatable X-Remote-Group) with the documented strip-then-inject trust contract |
| `http.rs` (+) | ~25 | 1 | `Headers::get_all` / `Headers::remove_all` (repeatable + strippable headers) |

**Net new this round ≈ 1,487 inserted lines** (≈1,020 impl / ≈500 test) across 7
files. **Tests: 208 → 234** (`cargo test --features tls`); **230** without the
feature (TLS tests compile out cleanly).

### The TLS data path (handler chain untouched)

```
TcpListener.accept()  →  rustls ServerConnection / StreamOwned  ──┐
                                                                  │  (Read + Write)
crate::server::read_request  →  inject_client_identity  →  ApiServer::handle  →  write_all
```

`serve_tls_stream` is the only new entry point; everything below `read_request`
is the byte-for-byte same code the plain `Server` runs. The lib doc's promise —
*"the connection handler is generic over any `Read + Write`, so a rustls
`StreamOwned` slots in without touching the chain"* — is now realised, not
asserted.

### mTLS identity (anti-spoofing)

On a verified handshake the terminator reads the peer leaf cert, runs
`x509::subject_identity`, and sets `X-Remote-User`/`X-Remote-Group` —
**after first stripping any client-supplied copies**, so a client cannot forge
an identity by sending those headers in cleartext. `RequestHeaderAuthenticator`
then resolves them. Test `mtls_client_cert_becomes_the_authenticated_identity`
proves the client cert subject `CN=alice` becomes the *audited* identity (not
`system:anonymous`); `mtls_rejects_client_without_certificate` proves a
cert-less client fails the handshake.

### Admission webhooks (call mechanism)

`WebhookValidating/MutatingPlugin` implement the **existing**
`ValidatingPlugin`/`MutatingPlugin` traits, so they drop into
`AdmissionChain::with_validating/with_mutating` with zero changes to the chain.
The HTTP call is a `WebhookClient` seam: unit tests drive it with
`MockWebhookClient`; `http_client_posts_to_a_real_listener` exercises the std
`HttpWebhookClient` against a real loopback `TcpListener`. `https://` endpoints
plug into the same trait via a TLS-backed client (the `tls` feature supplies the
stream type).

## 3. Acceptance criteria

| Criterion | Status |
|---|---|
| `cargo test` PASS | ✅ 230 (default) / **234** (`--features tls`), 0 failed |
| kubectl create/get/update/delete roundtrip (mock) | ✅ pre-existing `kubectl_style_rest_session_over_tcp` (create/get/list/watch/delete over real TCP) still green |
| TLS handshake test | ✅ `tls_handshake_and_request_roundtrip` (real rustls client↔server, CA-trusted) |
| mTLS handshake + identity | ✅ `mtls_client_cert_becomes_the_authenticated_identity`, `mtls_rejects_client_without_certificate` |
| watch stream test | ✅ pre-existing chunked-watch tests green |
| admission webhook roundtrip | ✅ allow/deny/mutate via mock + `http_client_posts_to_a_real_listener` + `webhook_plugs_into_admission_chain` |
| LOC ratio report | ✅ this document |
| TDD git log | ✅ §4 |
| Handoff file | ✅ `docs/audit/apiserver-tls-webhooks-handoff.md` |

## 4. TDD / commit log (this round, on the worktree branch)

```
feat(apiserver): rustls TLS/mTLS termination + x509 client-cert authn (feature tls)
feat(apiserver): dynamic admission webhook plumbing (admission.k8s.io/v1)
feat(apiserver): std-only X.509 subject parser (CN->user, O->groups)
```

Each module was written test-first within its file (red on the missing symbol,
green on implement); the real-socket/handshake tests (`http_client_posts_to_a_real_listener`,
`tls_handshake_and_request_roundtrip`) caught two genuine bugs during the loop
(a mock server closing the TCP read-half mid-write → reset; client read-to-EOF
over TLS surfacing an unclean close), both fixed before commit.

## 5. Honest port-method note

- **`x509.rs` / `webhook.rs` are std-only**; the only external dependency is
  `rustls` (+ `rustls-pemfile` / `rustls-pki-types`), gated behind the
  **off-by-default `tls` feature** so the decision core stays dependency-free.
  rustls is pinned to the **`ring`** crypto provider (`default-features = false`)
  so the crate builds **fully offline** with no aws-lc C toolchain — verified.
- This remains a **behavioural reimplementation of the documented contracts**
  (RFC 5280 Subject Name for x509, `admission.k8s.io/v1` for AdmissionReview,
  the front-proxy `--requestheader-*` mechanism, RFC 4648 base64), **not** a
  verbatim transcription of `kubernetes/kubernetes`. `parity.manifest.toml`
  updated: `fill_ratio` 0.45 → **0.50**, `honest_ratio` **1.00**; the TLS/x509
  and admission-webhook `[[unmapped]]` entries narrowed to their genuine
  remainders (HTTP/2, OIDC/JWT authn, Node authorizer, `*WebhookConfiguration`
  registration objects, CRDs, aggregation).
- **License:** Apache-2.0 (workspace). Test certificates under
  `tests/fixtures/` are throwaway, locally generated, 100-year self-signed
  fixtures with no production value.
