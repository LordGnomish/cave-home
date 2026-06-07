# cave-home-apiserver-rs — Handoff (TLS + webhooks round, 2026-06-07)

## Where this lives
- **Worktree:** `../cave-home-apiserver-transport`
- **Branch:** `feature/apiserver-http-transport` (NOT pushed, NOT merged)
- **Base:** round-1 HTTP-transport tip `ac280e4`
- **3 new commits** (x509 → webhook → tls), see report §4.

## State
- `cargo test -p cave-home-apiserver-rs` → **230 pass**.
- `cargo test -p cave-home-apiserver-rs --features tls` → **234 pass**.
- `cargo build -p cave-home-apiserver-rs --features tls --offline` → builds
  (rustls 0.23 + ring, fully cached; no aws-lc/C toolchain).
- `cargo clippy --lib` (± `--features tls`) → no errors; the ~188 warnings are
  the crate's pre-existing pedantic baseline (every module emits them — see the
  per-file count in the round notes), my files match peer density.

## What was added
- `src/x509.rs` — std DER Subject parser, `subject_identity(&[u8]) -> Option<UserInfo>`.
- `src/webhook.rs` — `admission.k8s.io/v1` codec, `WebhookClient` seam
  (`MockWebhookClient`, `HttpWebhookClient`), `WebhookMutating/ValidatingPlugin`,
  `FailurePolicy`, base64.
- `src/tls.rs` *(feature `tls`)* — `server_config`, `server_config_mtls`,
  `serve_tls_stream`, `TlsServer`, `load_certs`/`load_private_key`, `ring_provider`.
- `src/authn.rs` — `RequestHeaderAuthenticator`.
- `src/http.rs` — `Headers::get_all` / `Headers::remove_all`.
- `tests/fixtures/{ca,server,client}.{crt,key}` + `client.der` — locally
  generated, 100-yr self-signed; client subject `O=system:masters,O=dev,CN=alice`,
  leaf EKUs set (server=serverAuth, client=clientAuth, required by webpki).

## Gotchas for the next ray
- **`unsafe_code = "forbid"`** is workspace-wide; rustls/ring are deps (fine),
  our code is unsafe-free — keep it that way.
- **Cargo.lock is gitignored** in this repo — do not try to commit it.
- The `tls` feature MUST stay off by default; the decision core's selling point
  is std-only. rustls **must** keep `default-features = false, features=["ring",…]`
  or the build pulls aws-lc and breaks offline.
- TLS read at EOF: a `Connection: close` server triggers an unclean-close error
  on the rustls client read; the test helper treats any read error after the
  body as end-of-stream. Real clients should send/expect `close_notify` for a
  clean shutdown (not implemented — single-request-per-connection like the plain
  server).

## Genuine next gaps (now narrowed in `parity.manifest.toml`)
1. **`*WebhookConfiguration` API objects** — the call mechanism works; what's
   missing is registering webhooks declaratively + `rules`/`namespaceSelector`/
   `objectSelector` matching to decide *which* webhooks fire. Natural next ray.
2. **HTTP keep-alive + HTTP/2** on both plain and TLS paths (currently
   one-request-per-connection).
3. **OIDC / service-account-JWT authenticators** — more `Authenticator` impls.
4. **Node authorizer + SubjectAccessReview** — security-phase authz.
5. **Wire `Backend` behind `Registry`** for durable kine-backed storage
   (object↔KV (de)serialization + watch off the kine stream).
6. **`https://` webhook client** — a rustls-backed `WebhookClient` under the
   `tls` feature (the seam + cert loaders already exist in `tls.rs`).
