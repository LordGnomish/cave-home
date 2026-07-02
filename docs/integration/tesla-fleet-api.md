<!-- SPDX-License-Identifier: Apache-2.0 -->
# Tesla Fleet API integration (`cave-home-tesla`)

`cave-home-tesla` is the home-energy adapter for Tesla Powerwall / solar / energy
sites. It speaks two surfaces:

- the **Fleet API** cloud control plane (OAuth2-PKCE, the
  `/api/1/energy_sites/*` endpoints), and
- the **Powerwall local Gateway** on your LAN (a low-latency fallback).

It is a library crate compiled into the single cave-home binary (Charter §5) —
never a separate pod or Helm release. This guide is for an operator wiring it up;
**no real credentials live in the repo.**

## 1. Register a Fleet API application

1. Sign in at <https://developer.tesla.com> and create an application.
2. Note your **client id** and **client secret**.
3. Add a **redirect URI** — for the loopback desktop/CLI flow use
   `https://localhost:8443/callback`.
4. Request the energy scopes: `openid`, `offline_access`, `energy_device_data`,
   `energy_cmds`.
5. Host the public key / complete partner-token registration per Tesla's current
   onboarding (region-specific).

## 2. Configure cave-home

The adapter ships **disabled with placeholders**. Fill in the energy section of
your node config (the binary's layered config feeds this `TeslaConfig` shape):

```toml
[tesla]
enabled        = true
region         = "eu"            # na | eu | cn
site_id        = 0              # your energy_site_id (see step 4)
client_id      = "REPLACE_WITH_TESLA_CLIENT_ID"
redirect_uri   = "https://localhost:8443/callback"
rate_limit_secs = 30            # 1 request / 30 s per endpoint (hard limit)

# Optional Powerwall LAN fallback:
# [tesla.gateway]
# host = "https://192.168.1.10"
```

Secrets (`client_secret`, the gateway `password`, and the OAuth tokens) are
**never** committed. They are supplied at runtime via the environment or the
credential file below, and are wrapped in `Secret`, which redacts in every
`Debug`/`Display` so tracing can never leak a token.

## 3. Authenticate (OAuth2 PKCE)

The flow is the standard Authorization-Code + PKCE (S256):

1. Generate a PKCE pair (`PkcePair::generate(entropy)`), build the authorize URL
   (`authorize_url`) and open it.
2. Approve access; Tesla redirects to your `redirect_uri` with `?code=...`.
3. Exchange the code (`token_exchange_body` → POST to the token endpoint) using
   the PKCE `code_verifier`.
4. Persist the resulting `Credentials` (access + refresh token, expiry, region).

Tokens are stored at:

```
~/.cave-home/tesla-credentials.json   (mode 0600, owner-only)
```

The access token is auto-refreshed via the stored `refresh_token`
(`refresh_body`) before it lapses (with a clock skew margin).

## 4. Find your energy site id

`GET /api/1/products` returns vehicles and energy sites mixed; the energy
products carry an `energy_site_id`. Put that value in `site_id`.

## 5. What you get

| Action                              | Trait method                | CLI |
|-------------------------------------|-----------------------------|-----|
| Live flow + battery                 | `get_power_flow`            | `cavehomectl energy status` |
| Site status (mode, storm, reserve)  | `get_status`                | — |
| Set operation mode                  | `set_operation_mode`        | `cavehomectl energy mode <self-consumption\|backup\|tbc>` |
| Set backup reserve                  | `set_backup_reserve`        | `cavehomectl energy backup-reserve <percent>` |
| History                             | `get_history`               | `cavehomectl energy history --range 24h` |

The Portal `/energy` page renders the live flow diagram, the state-of-charge
bar, a 24-hour graph, the mode selector and the backup toggle.

## 6. Resilience & limits

- **Rate limit:** one request / 30 s **per endpoint**, enforced client-side;
  HTTP 429 is retried with exponential back-off (capped).
- **Cache:** the last good reading is served for up to **5 minutes** if the
  cloud is briefly unreachable, so the household surfaces keep working.
- **Local fallback:** if a Powerwall gateway is configured, the live flow can be
  read straight off the LAN.

## 7. Observability

Prometheus metrics exported by the adapter:

```
tesla_pv_power_watts
tesla_battery_soc_percent
tesla_grid_import_watts
tesla_grid_export_watts
tesla_api_request_duration_seconds{endpoint="…"}   (summary: _count / _sum)
tesla_api_errors_total{endpoint="…",status="…"}
```

## Scope note (Phase 1b)

The only deferred piece is the real `reqwest`/TLS transport (cloud + self-signed
gateway). Every decision — PKCE, rate limiting, back-off, request building,
parsing, the domain mapping and the cache — is implemented and tested today
against an in-crate `MockTransport`. See `crates/cave-home-tesla/parity.manifest.toml`.
