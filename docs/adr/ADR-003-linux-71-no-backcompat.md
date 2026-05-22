# ADR-003 — Linux 7.1+ only; no backward compatibility

## Status

**Accepted** — 2026-05-14, finalised by Burak Tartan (founder).

Created: 2026-05-14
Supersedes: —
Superseded by: —

## Context

Charter §6 ("golden rule") and §8 ("no backcompat") both pointed at
"a recent mainline Linux kernel" without naming a version. That gap
needs to close: every code-level decision (cgroup v2 use, eBPF
hooks, io_uring usage, modern syscalls, KMS/DRM acceleration paths,
systemd unit assumptions) depends on it, and contributors need a
single answer to "what kernel am I allowed to assume?".

Cave Runtime locked in the same mandate (Linux 7.1+, no backcompat)
for the same reason: legacy hardware support is a perpetual tax on
performance, security, and code complexity. cave-home is an
independent project (Charter §5.1), but the smart-home stack faces
the same trade-offs:

- Camera / NVR (Frigate-class) wants modern KMS / DRM /
  hardware-accel paths.
- Voice (whisper.cpp / piper) wants modern SIMD, optional NPU.
- Automation engine wants io_uring and modern async I/O without
  fallbacks.
- Privacy-first crypto (post-quantum, modern TLS) wants kernel /
  userland that ships these without backports.
- eBPF observability is a net win for "is everything healthy?"
  signals.

Carrying support for 32-bit ARM (Pi 1–3 class), kernels < 7.0, or
legacy glibc / musl feature sets would mean `#ifdef` farms, perf
regressions on the *primary* target, and a constant audit cost.

cave-home explicitly does **not** target museum hardware.

## Decision

**cave-home runs on Linux 7.1+ kernels only. Backward compatibility
is not supported.** Concretely:

- **Kernel floor:** Linux 7.1 mainline.
- **Architecture:** 64-bit only. ARM64 floor = Pi 5 / Apple Silicon
  class; x86-64 floor = modern (post-2018) consumer or server CPUs.
  **32-bit ARM (Pi 1–3) is not a target.**
- **cgroup v2:** mandatory; no cgroup v1 fallback.
- **io_uring, eBPF, modern syscalls:** freely used. No alternative
  paths for kernels that don't support them.
- **KMS / DRM:** modern. No legacy fbdev fallback for the camera /
  NVR pillar.
- **systemd / init1:** modern. cave-home assumes systemd-class init
  and the modern service-manager surface (`sd_notify`, socket
  activation, hardening directives, etc.). Non-systemd inits are
  out of scope for the reference profile.
- **glibc / musl:** track current stable. No legacy feature-flag
  `#ifdef`s.
- **Hardware accelerators:** assume modern NPU / iGPU / Coral /
  CUDA paths. No CPU-only-only fallback path for the NVR pillar
  on the reference profile (CPU is supported, but not optimised
  past what the reference hardware needs).

This mandate lives at golden-rule level (Charter §6.2). Any
deviation requires an ADR amending §6.2.

## Consequences

### Accepted costs

- **Pi 1 / Pi 2 / Pi 3 (32-bit) are not a target.** Users on
  hardware below the floor must run an alternative (Home Assistant
  OS still supports these; that's a legitimate use case for HA OS).
- **Old Synology / Asustor boxes are not a target.** Same reason.
- **Distro packagers shipping for ancient LTS kernels cannot ship
  cave-home.** A current-stable distro is required (Debian 14+,
  Ubuntu 28.04+, Fedora 44+, Arch / NixOS / openSUSE Tumbleweed
  current — all support Linux 7.1+).
- **Contributors writing `#ifdef KERNEL_BACKCOMPAT` paths get the
  PR rejected.** Not a soft preference — a hard rule.

### Accepted gains

- **Performance ceiling raised.** Automation evaluation, MQTT broker
  hot path, voice / camera inference all run on the latest kernel
  primitives without compatibility detours.
- **Security.** Modern crypto (PQ candidates), modern syscall
  filtering (seccomp-bpf landlines), modern TLS, modern
  Wayland-class display stack for any local UI.
- **Complexity floor lowered.** No legacy-arch `#ifdef`s, no dual
  cgroup logic, no fallback observability path.
- **eBPF observability** becomes a first-class tool for the "is
  everything healthy?" signal the Charter §1 vision demands.
- **Mandate matches Cave Runtime's** kernel floor exactly, so any
  Linux distro work cave-home contributors do (drivers, packaging,
  kernel patches) is reusable in both ecosystems — without sharing
  any code (Charter §5.1).

### Concrete follow-ups

1. CI matrices target Linux 7.1+ only. No older kernels in test
   images.
2. Reference hardware ADR (still pending) inherits this floor.
3. The reference installer image will not build for 32-bit ARM
   targets.
4. Contributor docs (CONTRIBUTING.md "No backcompat" section) point
   at Charter §6.2 / ADR-003 for the authoritative wording.

## Alternatives considered

### Linux 6.x LTS

- **Pro:** Wider distro support today.
- **Con:** Misses io_uring features cave-home wants on the
  automation / broker hot path; misses kernel-side PQC crypto;
  misses recent eBPF improvements.
- **Not chosen.**

### "Latest LTS minus one" rolling

- **Pro:** Familiar enterprise posture.
- **Con:** Adds a moving-target version-skew tax with no value to
  the cave-home audience (homeowners on current distros).
- **Not chosen.**

### Honour 32-bit ARM via `#ifdef` paths

- **Pro:** Captures the Pi 1–3 audience.
- **Con:** That audience is well-served by Home Assistant OS today;
  cave-home does not differentiate against HA OS on legacy
  hardware, only on modern hardware where the unified-binary +
  Rust-perf bet pays off.
- **Not chosen.**

## Notes

This ADR is the **kernel / userland** floor only. The **hardware
BoM** for the v0.1 reference box is the subject of a separate ADR
(reference hardware profile, to be written). ADR-003 is a
necessary input to that one but does not pre-determine the BoM.
