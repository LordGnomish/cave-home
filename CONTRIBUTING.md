# Contributing to cave-home

Thanks for being curious. cave-home is being built in the open from day
one, which means everything — including the charter — is up for
discussion until it's been approved by the maintainers and frozen in
an ADR.

This document covers how to participate during the **pre-alpha /
scaffolding phase**. It will get stricter once we hit our first
tagged release.

---

## Before you contribute code

1. Read [`docs/CHARTER.md`](docs/CHARTER.md) and
   [`docs/adr/ADR-001-cave-home-scope.md`](docs/adr/ADR-001-cave-home-scope.md).
   The charter is short on purpose; it tells you what's in scope (smart
   home only) and what isn't (NAS, media, photos — all out).
2. Open an issue describing the change you want to make **before**
   opening a PR. We don't want you to do work that doesn't fit.
3. If the change touches charter scope, licence, target hardware, or
   anything that smells like a one-way door, expect to write a small
   ADR for it.

## The golden rule (Charter §6)

cave-home reimplements a lot of upstream projects. When we do, we
follow a strict rule:

> **Line-by-line upstream parity, with TDD. No stubs. No
> "self-reported" feature parity.**

In practice:

- We don't fork-and-diverge. We track upstream tip-of-stable and
  document any local deltas.
- We don't ship a `TODO("integration")` and call it integrated.
- Tests are written against real upstream behaviour (captured
  fixtures, conformance vectors), not against our stubs. Mocks are
  only for hardware boundaries.

## The clean-room rule (ADR-002)

cave-home is **Apache-2.0**. Several upstream projects we care about
are not — Zigbee2MQTT and Tasmota are GPL-3.0, Mosquitto is
EPL-2.0 / EDL-1.0, parts of ESPHome are GPL. Line-by-line porting
their source would make the cave-home tree a derivative work and
spread copyleft into it.

For those upstreams, the **clean-room reimplementation rule** is
mandatory (Charter §6.1):

> **Do not read the source of a GPL / EPL upstream.** Reimplement
> from public spec, RFCs, behavioural documentation, public API
> docs, wire-format analysis (captured traces, Wireshark dissections)
> only. Write your own test fixtures — do not port the upstream's
> tests. Each clean-room crate's ADR carries an "implemented from
> spec; source not read" declaration.

Concrete contributor rules:

- **Do not have the GPL upstream's source open** while writing a
  clean-room contribution. If you previously read that source, do
  not contribute to that crate — pick a different one.
- **Do not paste from a GPL upstream's repository**, ever. Not lines,
  not function signatures, not test data.
- **Cite the spec, not the source.** A clean-room PR description
  references the public specification or wire-format document it
  implemented against. References to upstream source files are a
  red flag.
- **Reviewers do not grep the upstream source either.** If the spec
  doesn't say it, neither the contributor nor the reviewer goes
  looking in the GPL repo to find out. File a question against the
  upstream's spec maintainers instead.

Crates currently under the clean-room rule are listed in
[`docs/upstream/REFERENCES.md`](docs/upstream/REFERENCES.md) under
`Port method = clean-room` (or `hybrid` for mixed-licence upstreams).

## Always-latest mandate

Upstream stays current. We don't pin an integration to an old
upstream version "because that's what works locally". If the upstream
we track tags a new stable, we adopt it.

## No backcompat

cave-home assumes (Charter §6.2 + §8, ADR-003):

- **Linux 7.1+ kernel only.** No backcompat below the floor. cgroup
  v2 mandatory; io_uring / eBPF / modern syscalls freely used; modern
  KMS/DRM; modern systemd.
- **64-bit only.** ARM64 floor = Pi 5 / Apple Silicon class; x86-64
  floor = modern. 32-bit ARM (Pi 1–3) is **not** a target.
- ≥ 8 GB RAM (more for camera / voice pillars).
- Modern smart-home radios (Zigbee 3.0+, Matter, Z-Wave 700+).
- A user who can plug in an Ethernet cable, install an image, and
  follow a setup wizard.

We don't carry legacy weight. PRs adding `#ifdef KERNEL_BACKCOMPAT`
paths, 32-bit ARM support, or cgroup v1 fallbacks are rejected — the
mandate is at golden-rule level (Charter §6.2). If you have a use
case below the floor, run Home Assistant OS instead; it still
supports those targets.

## Privacy bar

cave-home's privacy posture is stricter than most OSS projects.
Contributions must:

- **Not add cloud calls on the critical path.** If a feature needs
  the internet, it is opt-in, off by default, and clearly marked.
- **Not add default-on telemetry.** Ever.
- **Not assume a vendor account.** Setup, login, recovery, automations
  must all work offline.

If your contribution can't honour the above, file an issue first to
discuss before writing code.

## Code of conduct

Be excellent. Assume good faith, criticise the work and not the
person, step away from the keyboard before you flame someone.

We'll formalise this with a proper CoC document before the first
tagged release; the rule above is binding from day one.

## How to talk to us

Right now: GitHub issues only.

Once there's a community, we'll add a chat channel and document it
here. We don't intend to require any specific account-having vendor
to participate (Charter §9).
