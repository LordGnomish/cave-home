//! Deterministic shuffle — a seeded Fisher-Yates over a std-only linear
//! congruential generator (ADR-020).
//!
//! cave-home pulls in **no** random-number crate: shuffle must be reproducible
//! (the same seed gives the same order, so a "reshuffle" is testable and a UI
//! can show a stable order) and the engine must stay dependency-free. A 64-bit
//! LCG with well-known constants is more than enough entropy for ordering a
//! playback queue — this is not cryptography, it is "play these songs in a
//! pleasant unpredictable order".

/// A tiny seedable pseudo-random generator.
///
/// Constants are the widely-used Numerical-Recipes / PCG-style 64-bit LCG
/// multiplier and increment; the top bits are returned because the low bits of
/// an LCG have short periods.
#[derive(Debug, Clone)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// Multiplier (Knuth MMIX LCG).
    const MUL: u64 = 6_364_136_223_846_793_005;
    /// Increment (odd, as an LCG increment must be).
    const INC: u64 = 1_442_695_040_888_963_407;

    /// Seed the generator. Any `u64` is a valid seed; seed `0` is fine.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Advance and return the next 64-bit value.
    const fn next_u64(&mut self) -> u64 {
        // wrapping arithmetic is the defined LCG recurrence, not an overflow bug.
        self.state = self.state.wrapping_mul(Self::MUL).wrapping_add(Self::INC);
        // Return the high 32 bits (better-distributed) widened back to u64.
        self.state >> 32
    }

    /// A uniform value in `0..bound`. Returns `0` when `bound` is `0`.
    ///
    /// Uses rejection sampling to avoid modulo bias, so the distribution is
    /// even across the range regardless of `bound`.
    fn below(&mut self, bound: usize) -> usize {
        if bound <= 1 {
            return 0;
        }
        let bound = bound as u64;
        // Largest multiple of `bound` that fits the 32-bit value space; reject
        // anything at or above it so every residue class is equally likely.
        let limit = (u64::from(u32::MAX) + 1) / bound * bound;
        loop {
            let v = self.next_u64() & u64::from(u32::MAX);
            if v < limit {
                return usize::try_from(v % bound).unwrap_or(0);
            }
        }
    }
}

/// Produce a shuffled permutation of `0..len` for the given seed.
///
/// Deterministic: the same `(len, seed)` always yields the same order. Every
/// index in `0..len` appears exactly once (it is a permutation, never a
/// resample). An in-place Fisher-Yates over the generator above.
#[must_use]
pub fn shuffled_order(len: usize, seed: u64) -> Vec<usize> {
    let mut order: Vec<usize> = (0..len).collect();
    if len < 2 {
        return order;
    }
    let mut rng = Lcg::new(seed);
    // Fisher-Yates: walk from the end, swap each slot with a random earlier one.
    let mut i = len - 1;
    while i > 0 {
        let j = rng.below(i + 1);
        order.swap(i, j);
        i -= 1;
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn same_seed_is_deterministic() {
        let a = shuffled_order(50, 1234);
        let b = shuffled_order(50, 1234);
        assert_eq!(a, b, "identical seed must reproduce the exact order");
    }

    #[test]
    fn different_seed_usually_differs() {
        let a = shuffled_order(50, 1);
        let b = shuffled_order(50, 2);
        assert_ne!(a, b, "different seeds should give different orders for a large list");
    }

    #[test]
    fn covers_every_index_exactly_once() {
        let order = shuffled_order(100, 99);
        assert_eq!(order.len(), 100);
        let set: HashSet<usize> = order.iter().copied().collect();
        assert_eq!(set.len(), 100, "permutation must contain every index once");
        assert!(set.iter().all(|&i| i < 100));
    }

    #[test]
    fn handles_trivial_lengths() {
        assert_eq!(shuffled_order(0, 7), Vec::<usize>::new());
        assert_eq!(shuffled_order(1, 7), vec![0]);
    }

    #[test]
    fn actually_permutes_a_nontrivial_list() {
        // A correct shuffle of 0..20 is overwhelmingly unlikely to be the
        // identity; assert it moved at least one element.
        let order = shuffled_order(20, 42);
        let identity: Vec<usize> = (0..20).collect();
        assert_ne!(order, identity);
    }

    #[test]
    fn below_is_in_range() {
        let mut rng = Lcg::new(5);
        for _ in 0..1000 {
            assert!(rng.below(7) < 7);
        }
    }
}
