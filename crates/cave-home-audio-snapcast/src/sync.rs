//! Time-synchronisation model — the arithmetic that keeps every speaker playing
//! the same sample at the same wall-clock instant.
//!
//! Snapcast's synchronisation idea (modelled here from the public protocol
//! description, not its source) is that the server stamps each audio chunk with
//! the wall-clock time it should be played, and every client delays playback so
//! that — accounting for its own pipeline/hardware latency — it hits that
//! instant. To make a *set* of clients agree, the server picks a common target
//! buffer (a fixed lead time, e.g. 1000 ms) and each client computes how long it
//! must hold a freshly received chunk before it plays.
//!
//! This module is pure arithmetic over milliseconds: no clocks, no sockets. The
//! real wire-level time-sync (the clock-offset estimation handshake and the PCM
//! chunk timestamps) is network/timing-bound and deferred to Phase 1b (see the
//! parity manifest, ADR-020).

/// A signed latency / clock-offset value object, in milliseconds.
///
/// Positive means "this client's audio comes out later than the reference"
/// (it needs *less* added buffer to stay in sync); negative means it is ahead.
/// [`LatencyMs::new`] clamps to a sane hardware range so a bad device report
/// cannot poison the sync maths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LatencyMs(i32);

/// The widest latency cave-home will believe from a device, in milliseconds.
///
/// Real speaker/render-pipeline latencies live well inside ±2 s; anything
/// larger is a misreport and is clamped rather than trusted.
pub const MAX_ABS_LATENCY_MS: i32 = 2000;

impl LatencyMs {
    /// A zero offset.
    pub const ZERO: Self = Self(0);

    /// Construct a latency, clamping to ±[`MAX_ABS_LATENCY_MS`].
    #[must_use]
    pub const fn new(ms: i32) -> Self {
        if ms < -MAX_ABS_LATENCY_MS {
            Self(-MAX_ABS_LATENCY_MS)
        } else if ms > MAX_ABS_LATENCY_MS {
            Self(MAX_ABS_LATENCY_MS)
        } else {
            Self(ms)
        }
    }

    /// The raw millisecond value.
    #[must_use]
    pub const fn millis(self) -> i32 {
        self.0
    }
}

/// How long a single client must hold a freshly received chunk before playing
/// it, so that — together with its own latency — it lands on the shared target
/// buffer instant.
///
/// `delay = target_buffer - client_latency`, clamped to `0..=target_buffer`:
/// a client slower than the target cannot un-delay time, so it plays as soon as
/// it can (and the caller should raise the target buffer to re-converge).
#[must_use]
pub const fn client_delay_ms(target_buffer_ms: u32, client_latency: LatencyMs) -> u32 {
    let target = target_buffer_ms as i64;
    let delay = target - client_latency.millis() as i64;
    if delay < 0 {
        0
    } else if delay > target {
        // client_latency was negative (ahead): never delay past the buffer.
        target_buffer_ms
    } else {
        // delay is in 0..=target here, and target == target_buffer_ms (a u32),
        // so this cast is exact and non-negative.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            delay as u32
        }
    }
}

/// The smallest target buffer that keeps every client in sync.
///
/// The slowest (largest-latency) client sets the floor; this returns the larger
/// of `desired` and that floor, so a caller can ask for headroom. Returns
/// `desired` for an empty client set.
#[must_use]
pub fn min_target_buffer_ms(latencies: &[LatencyMs], desired: u32) -> u32 {
    let mut floor: i32 = 0;
    for l in latencies {
        if l.millis() > floor {
            floor = l.millis();
        }
    }
    // floor is >= 0 here (initialised to 0, only ever raised), so the cast is
    // exact and non-negative.
    #[allow(clippy::cast_sign_loss)]
    let floor = floor as u32;
    if floor > desired {
        floor
    } else {
        desired
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    #[test]
    fn latency_clamps_to_hardware_range() {
        assert_eq!(LatencyMs::new(50).millis(), 50);
        assert_eq!(LatencyMs::new(-50).millis(), -50);
        assert_eq!(LatencyMs::new(99_999).millis(), MAX_ABS_LATENCY_MS);
        assert_eq!(LatencyMs::new(-99_999).millis(), -MAX_ABS_LATENCY_MS);
    }

    #[test]
    fn delay_is_buffer_minus_latency() {
        // 1000 ms target, a 120 ms-slow client holds chunks 880 ms.
        assert_eq!(client_delay_ms(1000, LatencyMs::new(120)), 880);
        // A zero-latency client holds the whole buffer.
        assert_eq!(client_delay_ms(1000, LatencyMs::ZERO), 1000);
    }

    #[test]
    fn delay_clamps_to_zero_for_too_slow_client() {
        // A client slower than the whole buffer cannot un-delay; it plays asap.
        assert_eq!(client_delay_ms(500, LatencyMs::new(700)), 0);
    }

    #[test]
    fn delay_never_exceeds_buffer_for_fast_client() {
        // A client that is "ahead" (negative latency) must not over-delay.
        assert_eq!(client_delay_ms(1000, LatencyMs::new(-300)), 1000);
    }

    #[test]
    fn target_floor_follows_slowest_client() {
        let lats = [LatencyMs::new(40), LatencyMs::new(900), LatencyMs::new(120)];
        // Slowest is 900; a 500 desired must be raised to 900.
        assert_eq!(min_target_buffer_ms(&lats, 500), 900);
        // A generous 1500 desired already clears the floor.
        assert_eq!(min_target_buffer_ms(&lats, 1500), 1500);
    }

    #[test]
    fn target_floor_ignores_negative_latencies() {
        let lats = [LatencyMs::new(-200), LatencyMs::new(-50)];
        assert_eq!(min_target_buffer_ms(&lats, 300), 300);
    }

    #[test]
    fn empty_client_set_keeps_desired() {
        assert_eq!(min_target_buffer_ms(&[], 1000), 1000);
    }
}
