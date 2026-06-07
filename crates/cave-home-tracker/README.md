# cave-home-tracker

A persistent, **config-driven upstream delta tracker**. Instead of dispatching a
one-off audit every time someone asks "how far along is the K3s port?", this
makes the answer a standing, reproducible measurement.

One generic binary tracks **any** project (cave-home, cave-runtime, …) — the
project, source root, upstreams and subsystem→crate mapping all come from a
`tracker.yaml`. See `examples/cave-runtime.tracker.yaml`.

## Subcommands

```
cave-home-tracker --config tracker.yaml poll        # shallow-clone/update upstreams
cave-home-tracker --config tracker.yaml measure     # snapshot LOC / tests / stubs
cave-home-tracker --config tracker.yaml diff        # day-over-day deltas
cave-home-tracker --config tracker.yaml report      # docs/audit/daily-progress-<date>.md
cave-home-tracker --config tracker.yaml dashboard   # Prometheus :9100/metrics
```

`poll` accepts `--upstream <name>` (repeatable) to clone a subset.
`measure` accepts `--no-tests` for a fast LOC+stub-only pass.
`report` accepts `--stdout` or `-o <path>`.
`dashboard` accepts `--addr <host:port>` (default `0.0.0.0:9100`).

## The honest completion formula

Real % is **not** a paperwork number. For each subsystem it is three measured
signals multiplied together — a weakness in any one drags the score down:

```
real % = coverage × test-pass-rate × stub-integrity × 100
```

- **coverage** — `port_loc / upstream_loc`, capped at 1.0. A subsystem that
  *declares* an upstream which has not been polled (or whose crate does not exist
  yet) scores **0**, never a false "first-party 100%". A genuinely first-party
  crate (no `upstreams:`) is "done" iff it has code.
- **test-pass-rate** — passing ÷ run tests. **No tests ⇒ 0**: untested code is
  not trusted code.
- **stub-integrity** — `1.0` with no stubs, falling linearly to `0` at ~1
  `todo!`/`unimplemented!`/`panic!` per 100 port LOC.

Group rollups (K3s, Smart-Home) and the overall figure are weighted by upstream
LOC, so a 50k-line subsystem counts more than a 500-line one.

> Heuristic note: the LOC and stub counters are line-based, not full parsers.
> The stub counter can count `todo!`/`panic!` tokens that appear inside string
> literals or test fixtures, slightly over-penalising crates whose tests embed
> those tokens. It is consistent over time, which is what matters for a trend.

## Layout (offline, std-only where it counts)

| module | responsibility |
|---|---|
| `loc` | language-classified source line counter (replaces `tokei`) |
| `stubs` | `todo!`/`unimplemented!`/`panic!` tally |
| `git` | `GitRunner` seam → `ShellGit` (`git clone --depth 1`) + poll orchestration |
| `config` | `tracker.yaml` model + validation + `~` expansion |
| `honest` | the completion formula + weighted aggregates |
| `measure` | `TestRunner` seam → `CargoTestRunner`; folds LOC/tests/stubs into a snapshot |
| `snapshot` | `Snapshot` + dated JSON `SnapshotStore` |
| `diff` | day-over-day deltas |
| `report` | daily markdown (tables, aggregates, 30-day sparkline) |
| `metrics` | Prometheus text exposition |
| `dashboard` | std-`TcpListener` HTTP server for `/metrics` |

No `tokei`, no async runtime, no HTTP framework — the counter and the metrics
server are implemented directly so the crate builds from the offline cache.

## Scheduled daily run (macOS)

`dist/com.gnomish.cave-home-tracker.plist` runs `poll && measure && report`
every day at 06:00:

```sh
cargo build --release -p cave-home-tracker
cp crates/cave-home-tracker/dist/com.gnomish.cave-home-tracker.plist ~/Library/LaunchAgents/
launchctl load ~/Library/LaunchAgents/com.gnomish.cave-home-tracker.plist
```

State (clones, snapshots) lives under `work_dir` (default
`~/.cache/cave-home-tracker`); the daily report is written to
`docs/audit/daily-progress-<date>.md`.
