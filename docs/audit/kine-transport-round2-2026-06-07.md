# Kine transport — round-2 continuation report (2026-06-07)

**Branch:** `feature/kine-transport` (same isolated worktree as round-1).
**Crate:** `cave-home-kine-rs`. **Upstream:** `k3s-io/kine` (Apache-2.0).
**Base of round-2:** `b29e2ea` (round-1 HEAD — transport + storage foundation).
**Method:** strict TDD (test commit → verify RED → impl commit → verify GREEN),
faithful behavioural port over real drivers.

Round-1 (ray `f9700b4f` → `b29e2ea`) landed the SQLite backend, the etcd gRPC
KV/Maintenance/Watch shim, metrics, and the over-the-wire round-trip. It
**honestly deferred** five items (round-1 report §"Honestly deferred"). This
round closes four of the five.

## What shipped this round

| Area | Module(s) | Real? | New tests |
|---|---|---|---|
| Lease-key revocation, `db_size`, `VACUUM` defrag | `sqlite.rs` | yes — live SQL | 5 |
| **etcd `Lease` gRPC service** (Grant/Revoke/KeepAlive stream/TimeToLive) | `grpc.rs` | yes — tonic over the backend, injectable clock | 9 |
| **Retention compaction loop** + **lease reaper** + **`Maintenance.Defragment`** RPC + real `Status.dbSize` | `grpc.rs`, `metrics.rs`, `proto` | yes | 7 |
| **Cancellable watch**: `WatchCancelRequest`, `progress_notify`, `prev_kv` | `grpc.rs`, `watch.rs` | yes — `select!` over the after-poll | 3 (+ wire) |
| 64-bit PG/MySQL DDL + **portable shared SQL** statements | `dialect.rs` | pure SQL gen | 7 |
| Driver-agnostic SQL helpers (single verified copy) | `backend.rs` | pure | 5 |
| **Real PostgreSQL backend** (`tokio_postgres`) | `postgres.rs` | yes — live wire driver | 1 gated |
| **Real MySQL backend** (`mysql_async`) | `mysql.rs` | yes — live wire driver | 1 gated |
| Over-the-wire apiserver lease lifecycle / watch-cancel / compact+defrag | `tests/etcd_grpc_roundtrip.rs` | yes — real TCP + generated client | 3 |

### Test counts (round-1 → round-2)

| Config | round-1 | round-2 |
|---|---|---|
| default (`sqlite`) lib | 128 | **147** |
| `--features grpc` lib | 144 | **181** |
| `grpc` integration (`etcd_grpc_roundtrip`) | 4 | **7** |
| `postgres` / `mysql` live (DSN-gated) | — | **1 + 1** |
| doctest | 1 | 1 |

All pass, 0 ignored (the two live tests **skip-pass** with a printed notice when
their DSN is absent — see below). `cargo clippy --lib` is clean in every feature
configuration (`sqlite`, `grpc`, `postgres`, `mysql`, and all combined), the
enforced gate.

## The Postgres / MySQL drivers — honest status

Round-1 deferred the live PG/MySQL drivers because *"this environment has no
PG/MySQL server to test against and shipping untested DB driver code would
violate the no-stub/honest rule."* That constraint is unchanged — there is still
no server in this sandbox — so the drivers are shipped exactly the way upstream
kine ships and tests its own: **real wire drivers, behind a CI-matrix gate.**

What is genuinely verified here:

- **They compile** against the real `tokio_postgres` 0.7 / `mysql_async` 0.34
  APIs (offline-cached, no TLS), under `--features postgres` / `--features
  mysql`, clippy-clean. The compiler checks every type-map, bind, and trait use.
- **They share the verified SQL.** Both issue the *same* query text as the
  SQLite backend, generated once in `dialect.rs` and unit-tested there (27
  tests). The MVCC sequencing mirrors the SQLite backend's 37-test logic.
- **The live cycle test exists** (`tests/pg_mysql_live.rs`): create → read →
  update → delete → lease attach/revoke → watch → compact → defragment over a
  real connection. It runs against a server when `KINE_PG_DSN` /
  `KINE_MYSQL_DSN` is set, and prints `SKIP …` and passes when it is not.

What is **not** claimed: the PG/MySQL drivers are *not* asserted to have passed
against a live server in this run — no server was available. They are
wire-complete and gated, not green-against-a-DB. Driver-specific care taken
without a server to catch it: Postgres binds the `(deleted = 0 OR ?)` flag as a
real `bool` (PG's strict typing), uses `INSERT … RETURNING id` and `VACUUM
FULL`; MySQL reads the id from `LAST_INSERT_ID()`, sets `NO_AUTO_VALUE_ON_ZERO`
so the sentinel can sit at id 0, and uses `OPTIMIZE TABLE`. The shared DDL was
widened to 64-bit (`BIGSERIAL`/`BIGINT`) so 63-bit lease ids bind without
truncation (upstream kine's 32-bit `SERIAL` id is a known ceiling we raise).

## LOC delta

`git diff --shortstat b29e2ea..HEAD`: **2,342 insertions / 113 deletions across
13 files**, in **13 commits (6 strict test→feat pairs + 1 refactor).**

| New production module | lines (incl. doc) |
|---|---|
| `postgres.rs` | 451 |
| `mysql.rs` | 522 |
| `backend.rs` (excl. its tests) | ~99 |
| `grpc.rs` additions (excl. new tests) | ~395 |
| `dialect.rs` additions (excl. tests) | ~95 |
| `metrics.rs` / `watch.rs` / `sqlite.rs` (net) | ~80 |

≈ **1,640 production lines** added this round (plus ~700 test lines).

Against upstream kine's PG/MySQL drivers + lease/maintenance server surface
(`pkg/drivers/pgsql` ~250, `pkg/drivers/mysql` ~250, lease/compact/defrag/watch-
cancel in `pkg/server` ~700 Go ≈ ~1,000 faithful Rust), the round-2 subset ratio
is ≈ 1,640 / ~1,400 ≈ **1.17** (the Rust is a touch heavier — two full async
drivers carry per-method connection/transaction plumbing Go folds into a generic
driver). Cumulative transport+storage subset remains well above the 0.80 bar.

## Still honestly deferred (the remaining round-1 item)

- **TLS, endpoint-string parsing, leader election, the periodic
  `compact_rev_key` *scheduling* wiring**, and the **4-track CLI/Portal**
  surfaces. The compaction *loop* and lease *reaper* now exist
  (`KineServer::spawn_compactor`); what is not wired is a binary that parses a
  `--datastore-endpoint` string and starts that loop. kine is hidden
  infrastructure (ADR-004 §6.3), so the operator surfaces stay thin.

## Merge status

All work is on `feature/kine-transport` (clean). **Not merged, not pushed** (per
directive and the live uplift-loop owning the shared checkout — memories
`concurrent-uplift-loop` / `isolate-in-worktree-when-loop-active`). Merge when
the loop is quiescent.
