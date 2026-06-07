# Kine transport + storage backend — port report (2026-06-07)

**Branch:** `feature/kine-transport` (isolated worktree off `bec7a9a`).
**Crate:** `cave-home-kine-rs`. **Upstream:** `k3s-io/kine` (Apache-2.0).
**Method:** strict TDD (test commit → verify RED → impl commit → verify GREEN),
faithful behavioural port of kine's `pkg/drivers/generic` + `pkg/server` over a
**real** SQLite driver and a **real** tonic gRPC server.

This closes the audit's Crit blocker — the decision core had *no real backend or
transport* (`docs/audit/k3s-ground-truth-2026-06-07.md` §0.2, §4.2). It now has
both: a live SQLite-backed etcd datastore that an unmodified apiserver / etcdctl
can speak to over the wire.

## What shipped (all behind feature gates; default = `sqlite`)

| Layer | Module | Real? | Tests |
|---|---|---|---|
| Per-driver SQL dialect (SQLite/Postgres/MySQL) | `dialect.rs` | pure SQL gen, executed for SQLite | 20 |
| **Real SQLite storage backend** (rusqlite, bundled) | `sqlite.rs` | **yes — live SQL, on-disk persistence proven** | 33 |
| etcd gRPC **KV** (Range/Put/DeleteRange/Txn/Compact) + **Maintenance** | `grpc.rs` | yes — tonic server over real backend | 13 |
| etcd gRPC **Watch** streaming (poll model, faithful to kine) | `grpc.rs` | yes | 2 |
| Live request/compaction **metrics** (Prometheus exposition) | `metrics.rs` | yes — wired into handlers | 7 + 1 |
| End-to-end **over-the-wire** round-trip (apiserver flow) | `tests/etcd_grpc_roundtrip.rs` | yes — real TCP, generated client | 4 |

**Totals:** default lib **128** tests, with `--features grpc` **144** lib + **4**
integration = **148**. All pass, 0 ignored. `cargo clippy --lib` clean in both
configs (the enforced gate).

The build is offline + single-binary friendly: rusqlite `bundled` compiles SQLite
into the binary; tonic/prost are pure Rust; PG/MySQL drivers are opt-in features.

## Faithfulness highlights

- **Dialect** reproduces kine's `generic.go` constants verbatim in structure:
  `revSQL`, `compactRevSQL`, the `MAX(mkv.id) … GROUP BY mkv.name` latest-row
  join (current + `mkv.id <= ?` historical), `afterSQL`, insert/delete/compact,
  and the `q()` `?`→`$N` placeholder rebind. Driver-correct DDL
  (AUTOINCREMENT/SERIAL/AUTO_INCREMENT, BLOB/BYTEA/MEDIUMBLOB).
- **SQLite backend** is the real append-only log: the auto-increment `id` *is*
  the global revision; `compact_rev_key` is pinned at id 0 so user revisions
  start at 1; `prev_revision` maintains the unique `(name, prev_revision)`
  generation chain; compaction keeps only the latest *live* row per key.
- **gRPC** honours etcd's `range_end` conventions (point / prefix / all-keys /
  `[k,end)` pagination), `count_only`/`keys_only`, `prev_kv`,
  `ignore_value`/`ignore_lease`, the compacted-revision `OutOfRange` guard, and
  the apiserver's Txn create/CAS idiom. Errors map to etcd's well-known gRPC
  statuses + messages.

## LOC ratio (port vs. upstream)

Measured on the ported **transport+storage subset** of kine (not the whole repo).

| Upstream Go (in-scope subset) | est. Go LOC | Port Rust (production, excl. tests) |
|---|---|---|
| `drivers/generic` (SQL + dialect) | ~700 | `dialect.rs` 293 + part of `sqlite.rs` |
| `drivers/sqlite` | ~150 | `sqlite.rs` ~528 (driver + MVCC exec) |
| `logstructured/sqllog` (create/list/after/compact) | ~700 | (folded into `sqlite.rs`) |
| `server` KV/Watch/Maintenance gRPC shim | ~1,200 | `grpc.rs` 556 + `proto` 232 |
| **In-scope subtotal** | **~2,750** | **~1,766** production lines |

Faithful-Rust estimate of the subset ≈ Go × 0.8 ≈ **~2,200**.
**LOC-ratio ≈ 1,766 / 2,200 ≈ 0.80** for the ported subset.
Plus ~1,150 lines of tests (the TDD suite) and the 175-line integration test.
Total diff: **2,933 insertions across 10 files, 6 strict test→feat commit pairs.**

(Against *full* kine ~7,744 Go LOC the figure is lower — the deferred surface
below is real and not yet ported.)

## Honestly deferred (not built — next increments)

1. **Postgres & MySQL live drivers.** The **dialect** (the shared query
   interface kine uses across all three) is done and unit-tested for all three
   drivers, and tokio-postgres / mysql_async are cached and ready. The live
   `PgStore` / `MysqlStore` execution layer is *not* written, because this
   environment has no PG/MySQL server to test against and shipping untested DB
   driver code would violate the no-stub/honest rule. Next: implement them
   against the existing dialect with live tests gated on `KINE_PG_DSN` /
   `KINE_MYSQL_DSN` (skipped when absent), mirroring upstream kine's CI matrix.
2. **Lease gRPC service.** The proto is vendored and the pure `LeaseTable`
   decision core exists; the `Lease` service handlers (Grant/Revoke/KeepAlive/
   TimeToLive) are not yet wired to the backend.
3. **TLS, endpoint-string parsing, leader election, the `compact_rev_key`
   periodic compaction loop.** Infra around the core that the apiserver does not
   strictly need for a single-node bring-up.
4. **4-track CLI/Portal:** a `cavehomectl kine` subcommand and a Portal status
   page. kine is hidden infrastructure (ADR-004 §6.3), so these are thin
   operator surfaces; the metrics registry (`KineMetrics::render`) is the
   Prometheus half of the observability track and is done.

## Merge status

All work is on `feature/kine-transport` (clean, isolated). A local no-ff merge
into the integration line was **not** performed automatically: the live
uplift-loop owns the shared checkout (memory `concurrent-uplift-loop` /
`isolate-in-worktree-when-loop-active`), so merging now would race it. Merge when
the loop is quiescent. **Not pushed** (per directive — push needs explicit
permission).
