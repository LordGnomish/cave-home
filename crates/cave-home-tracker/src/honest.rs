// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors

//! The honest completion formula.
//!
//! "Real %" is *not* a paperwork number you assert in a manifest. It is derived
//! from three measured signals, multiplied together so that a weakness in any
//! one of them drags the whole score down:
//!
//! * **coverage** — how much of the upstream surface has actually been ported,
//!   measured as `port_loc / upstream_loc`, capped at 1.0. A port can never be
//!   "more than done" by writing more lines than the original.
//! * **quality** — the fraction of the port's own tests that pass. A port with
//!   no tests at all scores `0` here: untested code is not trusted code.
//! * **integrity** — a penalty for stub markers (`todo!`, `unimplemented!`,
//!   `panic!`). Every ~1 stub per 100 LOC of port erodes the score linearly to
//!   zero.
//!
//! `real = coverage * quality * integrity`, reported as a percentage.

// LOC and test counts are small enough that the u64 -> f64 widening used in the
// ratios never loses precision in practice; the lint is noise here.
#![allow(clippy::cast_precision_loss)]

use crate::snapshot::SubsystemMetric;
use crate::stubs::StubCount;

/// Inputs to the honest completion formula for one subsystem.
#[derive(Debug, Clone, Copy)]
pub struct CompletionInputs {
    /// Source LOC of the upstream surface being ported.
    pub upstream_loc: u64,
    /// Source LOC of the cave-home port.
    pub port_loc: u64,
    /// Whether the subsystem declares any upstream at all (vs. first-party).
    pub has_upstream: bool,
    /// Port tests that passed.
    pub tests_passed: u64,
    /// Port tests that failed.
    pub tests_failed: u64,
    /// Stub markers found in the port.
    pub stubs: StubCount,
}

/// How many stubs per 100 port LOC drive integrity to zero.
const STUB_ZERO_DENSITY: f64 = 1.0;

/// Coverage term in `[0, 1]`.
///
/// `has_upstream` distinguishes a genuinely first-party crate (no upstream
/// declared → coverage is "done" iff there is any code) from a subsystem that
/// *declares* an upstream which simply measured zero — not yet polled, or a
/// mis-pointed subpath. The latter must score `0`, never 100%, so an unmeasured
/// upstream can never masquerade as fully ported.
#[must_use]
pub fn coverage(upstream_loc: u64, port_loc: u64, has_upstream: bool) -> f64 {
    if has_upstream {
        if upstream_loc == 0 {
            // Upstream declared but unmeasured: we cannot claim any coverage.
            return 0.0;
        }
        return (port_loc as f64 / upstream_loc as f64).min(1.0);
    }
    // First-party: done iff there is any code at all.
    f64::from(u8::from(port_loc > 0))
}

/// Quality term in `[0, 1]`: the test pass rate, or `0` when there are no tests.
#[must_use]
pub fn quality(tests_passed: u64, tests_failed: u64) -> f64 {
    let total = tests_passed + tests_failed;
    if total == 0 {
        return 0.0;
    }
    tests_passed as f64 / total as f64
}

/// Integrity term in `[0, 1]`: `1` with no stubs, decreasing to `0` as stub
/// density rises.
#[must_use]
pub fn integrity(port_loc: u64, stubs: StubCount) -> f64 {
    let total = stubs.total();
    if total == 0 {
        return 1.0;
    }
    if port_loc == 0 {
        return 0.0;
    }
    let density = total as f64 / (port_loc as f64 / 100.0);
    (1.0 - density / STUB_ZERO_DENSITY).clamp(0.0, 1.0)
}

/// Combine the three terms into a real-completion percentage in `[0, 100]`.
#[must_use]
pub fn real_pct(inp: CompletionInputs) -> f64 {
    let c = coverage(inp.upstream_loc, inp.port_loc, inp.has_upstream);
    let q = quality(inp.tests_passed, inp.tests_failed);
    let i = integrity(inp.port_loc, inp.stubs);
    c * q * i * 100.0
}

/// Weighted aggregate real-% across many subsystems, weighting each by its
/// upstream LOC (so a 50k-line subsystem counts more than a 500-line one). When
/// every weight is zero, falls back to a plain mean.
#[must_use]
pub fn aggregate_real_pct<'a>(metrics: impl Iterator<Item = &'a SubsystemMetric>) -> f64 {
    let mut weighted = 0.0;
    let mut weight = 0.0;
    let mut plain = 0.0;
    let mut n = 0u64;
    for m in metrics {
        let w = m.upstream_loc as f64;
        weighted = m.real_pct.mul_add(w, weighted);
        weight += w;
        plain += m.real_pct;
        n += 1;
    }
    if weight > 0.0 {
        weighted / weight
    } else if n > 0 {
        plain / n as f64
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-6, "{a} != {b}");
    }

    #[test]
    fn coverage_is_capped_at_one() {
        approx(coverage(100, 50, true), 0.5);
        approx(coverage(100, 100, true), 1.0);
        approx(coverage(100, 250, true), 1.0);
    }

    #[test]
    fn coverage_first_party_when_no_upstream() {
        approx(coverage(0, 0, false), 0.0);
        approx(coverage(0, 10, false), 1.0);
    }

    #[test]
    fn coverage_declared_but_unmeasured_upstream_is_zero() {
        // Upstream declared (has_upstream=true) but measured 0 LOC: must NOT be
        // treated as first-party 100%; an unpolled upstream scores zero.
        approx(coverage(0, 500, true), 0.0);
    }

    #[test]
    fn quality_zero_without_tests() {
        approx(quality(0, 0), 0.0);
        approx(quality(9, 1), 0.9);
        approx(quality(10, 0), 1.0);
    }

    #[test]
    fn integrity_penalises_stubs() {
        approx(integrity(1000, StubCount::default()), 1.0);
        // 1 stub in 1000 LOC -> density 0.1 -> integrity 0.9
        approx(
            integrity(
                1000,
                StubCount {
                    todo: 1,
                    ..StubCount::default()
                },
            ),
            0.9,
        );
        // 10 stubs in 1000 LOC -> density 1.0 -> integrity 0
        approx(
            integrity(
                1000,
                StubCount {
                    panic: 10,
                    ..StubCount::default()
                },
            ),
            0.0,
        );
    }

    #[test]
    fn real_pct_multiplies_terms() {
        let inp = CompletionInputs {
            upstream_loc: 1000,
            port_loc: 500, // coverage 0.5
            has_upstream: true,
            tests_passed: 8,
            tests_failed: 2,             // quality 0.8
            stubs: StubCount::default(), // integrity 1.0
        };
        approx(real_pct(inp), 0.5 * 0.8 * 1.0 * 100.0);
    }

    #[test]
    fn untested_code_scores_zero() {
        let inp = CompletionInputs {
            upstream_loc: 100,
            port_loc: 100,
            has_upstream: true,
            tests_passed: 0,
            tests_failed: 0,
            stubs: StubCount::default(),
        };
        approx(real_pct(inp), 0.0);
    }

    #[test]
    fn aggregate_is_weighted_by_upstream_loc() {
        let big = SubsystemMetric {
            real_pct: 90.0,
            upstream_loc: 9000,
            ..SubsystemMetric::default()
        };
        let small = SubsystemMetric {
            real_pct: 10.0,
            upstream_loc: 1000,
            ..SubsystemMetric::default()
        };
        // (90*9000 + 10*1000) / 10000 = 82
        approx(aggregate_real_pct([&big, &small].into_iter()), 82.0);
    }

    #[test]
    fn aggregate_empty_is_zero() {
        approx(aggregate_real_pct(std::iter::empty()), 0.0);
    }
}
