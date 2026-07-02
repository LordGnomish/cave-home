# K3s Reimplementation — Ground-Truth Audit (2026-06-07)

**Method:** READ-ONLY. No code written. LOC via `cloc` on actual `src/` trees; upstream
Go LOC measured by shallow-cloning each upstream repo and running `cloc` (code lines,
no tests/vendor); test pass-rates from `cargo test` on the checked-out workspace; TDD
compliance from `git log` commit ordering; integration from grepping `cave-home-binary`,
`cave-home-cli`, `cave-home-portal`.

**Honesty stance:** This report separates *paperwork* (ADRs, `honest_ratio`, parity
manifests claiming "fully implemented") from *shipped, tested, integrated code*. Where the
self-reported numbers overstate, it is called out explicitly.

---

## 0. Three findings that reframe the whole question

### 0.1 They are NOT modules under `cave-home-orchestration` — they are 11 separate crates
The task brief assumed `cave-home-orchestration::<subsystem>`. Ground truth: each K3s
subsystem is its own workspace crate (`cave-home-kine-rs`, `-apiserver-rs`, `-kubelet-rs`,
`-containerd-rs`, `-scheduler-rs`, `-controller-manager-rs`, `-cni-flannel`, `-traefik-rs`,
`-klipper-lb-rs`, `-helm-controller-rs`, `-kube-proxy-rs`). `cave-home-orchestration` is a
separate, thin bring-up/lifecycle crate (bootstrap/role/shutdown) — it is the *intended*
host, not the container of the subsystems.

### 0.2 `honest_ratio = 1.00` does NOT mean "complete" — `fill_ratio` is the real proxy
Every K3s crate's `parity.manifest.toml` declares `honest_ratio = 1.00`. This is computed as
`fill / (fill + (1-fill)·(1-adr_justified))`, which equals 1.00 **whenever every unbuilt
item carries an ADR deferral note** — regardless of how little is built. The real
completion proxy is **`fill_ratio`, which ranges 0.18–0.40**. Each crate explicitly states
it is a *"spec-based behavioural reimplementation — NOT a verbatim line-by-line port,"*
with the SQL driver / gRPC server / netlink / transport / apiserver-wiring **deferred**.
So: the *decision core* (the hard semantic kernel) is real and tested; the I/O, protocol,
and wiring shells around it are mostly not built.

### 0.3 The rays' latest work is UNMERGED, and a new wave is mid-flight RIGHT NOW
The 7 original rays committed to per-subsystem worktree branches
(`claude/cave-home-k3s-*-2026-06-07`) that are **ahead of and not merged into** the live
`honest-uplift` integration branch. During this audit the working-tree HEAD moved on its own
(live wave-loop, per memory `concurrent-uplift-loop`) onto
`claude/cave-home-k3s-secrets-encryption-2026-06-07`, and `cave-home-orchestration`
**currently does not compile** because an in-flight `metrics_server` module is referenced
before being declared. The 4 "new" subsystems are in the **RED phase** (failing-test commits
landed today; impl not yet). Numbers below use the **latest worktree-branch** source as
"Port LOC (latest)" and flag the merge gap.

---

## 1. Master table

Port LOC = Rust `src/` code lines on the **latest ray branch** (worktree). Test pass% from
`cargo test` on the checked-out (merged/older) workspace — **100% pass, 0 failures
everywhere it built**. "LOC-ratio" = Port LOC ÷ estimated faithful-Rust port size (upstream
Go × ~0.8) — an *independent* completion cross-check vs. the self-reported `fill_ratio`.
"Real %" = my synthesized honest estimate of **decision-core** completion (integration is a
separate column, and it is **0** for all).

| Subsystem | Upstream Go LOC (in-scope) | Port LOC (latest) | Tests (latest) | Test pass% | TDD compliant | self fill_ratio | LOC-ratio | Integrated | **Real %** |
|---|---|---|---|---|---|---|---|---|---|
| kine | 7,744 (full) | 1,912 | 93 | 100% | ✅ yes | 0.30 | ~0.31 | ❌ no | **~30%** |
| apiserver | ~30,000 (minimal subset; full pkg+apimachinery ~141k) | 3,210 | 115 | 100% | ✅ yes | 0.20 | ~0.13 | ❌ no | **~15%** |
| kubelet | ~13,500 (core; full kubelet 71k) | 3,151 | 201 | 100% | ✅ yes | 0.20 | ~0.29 | ❌ no | **~25%** |
| containerd (CRI/snapshotter) | ~900 hand-written CRI (+generated) | 1,681 | 81 | 100% | ✅ yes | 0.18 | n/a* | ❌ no | **~20%** |
| scheduler | ~18,000 (full 28.7k) | 3,201 | 102 | 100% | ✅ yes | 0.28 | ~0.18 | ❌ no | **~20%** |
| controller-mgr | ~6,000 (4 core controllers; full 46.5k) | 1,702 | 84 | 100% | ✅ yes | 0.18 | ~0.33 | ❌ no | **~25%** |
| flannel | 9,022 (full) | 2,202 | 113 | 100% | ✅ yes | 0.40 | ~0.31 | ❌ no | **~33%** |
| coredns | ~12,000 (core+6 plugins; full 27.8k) | 4,844 | 125 | 100%† | ✅ yes | n/r | ~0.49 | ❌ no (**not in workspace**) | **~45%** |
| traefik | ~14,500 (ingress+gateway subset; full 59k) | 2,208 | 107 | 100% | ✅ yes | 0.30 | ~0.18 | ❌ no | **~20%** |
| kube-proxy | ~10,000 (est., not measured) | ~1,437 | ~85 | 100% | ✅ yes | 0.35 | ~0.18 | ❌ no | **~25%** |
| klipper-lb | ~96 (shell; no Go) | 1,175 | 52 | 100% | ✅ yes | 0.30 | n/a* | ❌ no | **~35%** |
| helm-controller | ~3,000 (est., not measured) | 1,013 | 54 | 100% | ✅ yes | 0.30 | n/a | ❌ no | **~30%** |

\* klipper-lb / containerd: upstream is shell or mostly generated protobuf, so a LOC ratio
is category-mismatched (the Rust reimpl is *larger* than the thing it replaces). Judged on
internal coverage instead.
† coredns: 125 tests / TDD log all green, but it lives **only** in the
`k3s-coredns` worktree, is **absent from the workspace `Cargo.toml`**, and was not run by
this audit's `cargo test` (which only sees workspace members). Pass% inferred from its
red-green commit log, not independently executed here.

**Weighted overall (by upstream Go LOC, decision-core only): ≈ 24%.**
**Weighted overall toward a *running, integrated* K3s: ≈ 12–15%** (decision core ×
~0.6, because all transport/IO and 100% of integration are unbuilt — see §4).

---

## 2. The 4 newest subsystems — barely started (RED phase, today)

These were dispatched in parallel and are mid-flight as of this audit:

| Subsystem | Branch | State | Real % |
|---|---|---|---|
| **klipper-lb ServiceLB controller** | (extends existing `klipper-lb-rs`) | RED — commit `3a08814` *"test(klipper-lb): add failing tests for ServiceLB controller reconcile loop"* landed today; impl pending. The crate already had 1,175 LOC / 52 tests of prior daemonset/port-alloc/service logic. | **~5% of the new slice** (base crate ~35%) |
| **local-path-provisioner** | `claude/k3s-local-path-provisioner` + `orchestration::local_path_provisioner` | RED — commit `b6b24e1` *"test(orchestration): add failing tests for local-path-provisioner config"*; only `config.rs` (15k) + `mod.rs` exist, no provisioner impl. | **~5%** |
| **metrics-server** | `claude/cave-home-k3s-metrics-server-2026-06-07` + `orchestration::metrics_server` | RED — commit `f8ae4f6` *"test(metrics-server): add failing tests for resource.Quantity"*; `quantity.rs` (7.6k) + `mod.rs`. Module **not yet declared in `lib.rs`**, so orchestration is currently non-compiling. | **~5%** |
| **secrets-encryption-at-rest** | `claude/cave-home-k3s-secrets-encryption-2026-06-07` (current HEAD) | **Not started** — branch exists (32 commits, all inherited) but its tip commit is the metrics-server test; **no secrets/encryption code or tests found** in any tree. | **~0%** |

---

## 3. What's genuinely strong (not paperwork)

1. **TDD compliance is real and exemplary.** Every ray branch shows strict, *separately
   committed* red→green pairs: `test(x): add failing tests …` immediately followed by
   `feat(x): implement …`. Examples: coredns has 12 clean RED/GREEN pairs; kine, apiserver,
   kubelet, scheduler-cm, flannel, traefik all the same. The `cace208 tdd: enforce strict
   TDD ordering` directive is being honored. **No "test+impl in one commit" violations
   found** — the `feedback_strict_tdd_enforcement` rule is satisfied.
2. **Tests actually pass.** `cargo test` on the workspace: kine 70, apiserver 79, kubelet
   77(+test bins), containerd 64, scheduler 83, controller-mgr 69, flannel 88, traefik 74,
   klipper 52, helm 54, kube-proxy bins — **0 failures, 0 ignored anywhere.** Latest ray
   branches add more (kine 93, apiserver 115, kubelet 201, …) following the same green log.
3. **No `todo!()` / `unimplemented!()` / stub macros** in any K3s crate `src/`. Unbuilt
   surface is *omitted and documented*, not faked with panicking stubs. (Workspace lints
   `forbid(unsafe)`, warn on `todo`/`unimplemented`/`unwrap`/`panic`.)
4. **Provenance was cleaned.** Commit `7521137 honesty: workspace-wide provenance sweep —
   remove fabricated upstream SHAs` removed the fabricated-SHA problem noted in prior memory.
   Manifests now cite spec sources + Apache-2.0 upstream honestly and disclaim line-by-line
   porting.

## 4. What's overstated or missing (the honest deductions)

1. **`honest_ratio = 1.00` is a presentation artifact, not completion.** Read it as "we
   disclosed our gaps," never "done." The real signal — `fill_ratio` — sits at **0.18–0.40**.
2. **Integration is zero.** None of the 11 crates is a dependency of `cave-home-binary`
   (`grep` of its `Cargo.toml`: no kine/apiserver/kubelet/scheduler/flannel/traefik/…).
   The binary only references a `Component::Orchestration` *enum*. CLI files are explicit
   placeholders (`apiserver.rs`/`kubelet.rs`/`scheduler.rs` say *"Phase 1b/2b placeholder"*,
   return hardcoded subcommand-name vectors). Portal only has the literal strings
   `"kubelet"`, `"apiserver"`, `"K3s"` in a label list. **`cargo run` does not start any K3s
   subsystem.** There is no cross-subsystem wiring (apiserver↔kine↔scheduler↔kubelet).
3. **Self-reported `fill_ratio` diverges from the LOC cross-check in both directions:**
   - *Overstated:* flannel claims 0.40 but LOC-ratio ≈0.31; scheduler 0.28 vs ≈0.18;
     traefik 0.30 vs ≈0.18; kube-proxy 0.35 vs ≈0.18.
   - *Understated:* controller-manager 0.18 vs ≈0.33; kubelet 0.20 vs ≈0.29.
   Either way the manifests are estimates, not measurements — treat ±0.10.
4. **coredns and the 4 new subsystems are not in the workspace.** coredns (4,844 LOC, the
   single largest and most-complete port) is stranded on an unmerged worktree branch and not
   a `Cargo.toml` member, so it doesn't build, test, or ship with the rest.
5. **"Spec-based, not line-by-line" means the upstream-LOC denominator is partly
   aspirational.** They are not trying to port all of kubernetes' Go; they port the
   documented *semantics* of a chosen subset. Against *full* upstream the percentages are far
   lower (apiserver 3,210 / ~141k full ≈ 2%); against the *realistic minimal subset* used
   above they are the 15–45% shown.

## 5. Bottom line

- **Decision-core reimplementation is real, tested, and honestly TDD'd** — roughly **24%
  weighted** of the in-scope semantic surface across the 7 original subsystems, with coredns
  (~45%) and flannel (~33%) furthest along, apiserver (~15%) and traefik (~20%) thinnest.
- **Toward a *running, integrated* K3s the real figure is ~12–15%**, because 100% of the
  transport/IO/protocol shells and 100% of binary/CLI/portal integration remain unbuilt.
- **The 4 newest subsystems are ~0–5%** (RED phase started today; secrets-encryption ~0%).
- **No fabrication detected** in the sampled work: no stub macros, honest provenance, strict
  red-green TDD, all tests green. The dishonesty risk here is **not** fake code — it is the
  `honest_ratio = 1.00` framing inviting a reader to conclude "complete" when `fill_ratio`
  says 20–40% of the *core* (and ~0% of integration) is done.

## 6. Blockers / caveats for the reader

- **Live race:** the working tree HEAD moved during this audit and `cave-home-orchestration`
  was left non-compiling by an in-flight commit. Numbers are a snapshot of an actively-moving
  target; re-run after the current wave lands.
- **Unmerged work:** "Port LOC (latest)" reflects worktree branches not merged into
  `honest-uplift`; the integration branch carries an older, smaller snapshot.
- **Not independently re-run:** coredns + latest-branch test increments were not executed by
  this audit (inferred green from commit logs); workspace `cargo test` covers the
  merged/older versions only.
- **Upstream LOC for kube-proxy and helm-controller were estimated, not measured.**

---
*Audit performed read-only on 2026-06-07. Upstream clones measured at HEAD same day.*
