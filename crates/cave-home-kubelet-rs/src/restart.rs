// SPDX-License-Identifier: Apache-2.0
//! Container restart decision + CrashLoopBackOff backoff curve.
//!
//! Behavioural reimplementation of the documented kubelet restart logic
//! (`pkg/kubelet/kuberuntime/kuberuntime_manager.go::computePodActions` restart
//! gate + `pkg/kubelet/util/...` exponential backoff used by the crash-loop
//! back-off, a.k.a. `flowcontrol.Backoff` in `kubelet.go`).
//!
//! Two pure decisions live here:
//!
//! 1. [`should_restart`] — given the [`RestartPolicy`] and the last exit code,
//!    decide whether a terminated container should be restarted at all.
//! 2. [`backoff_delay`] — the CrashLoopBackOff curve: the delay before the
//!    `n`-th restart attempt. Upstream doubles a base delay each consecutive
//!    failure and caps it at a maximum (the documented values are an initial
//!    10s doubling up to 300s = 5 min).
//!
//! The caller supplies the current monotonic time and the attempt count; this
//! module performs no clock access of its own (`std`-only, no `time` crate).

use crate::api::RestartPolicy;

/// Initial crash-loop back-off delay, in milliseconds (documented: 10s).
pub const BACKOFF_BASE_MS: u64 = 10_000;

/// Maximum crash-loop back-off delay, in milliseconds (documented cap: 5 min).
pub const BACKOFF_MAX_MS: u64 = 300_000;

/// Decide whether a terminated container should be restarted.
///
/// * `Always`    — restart regardless of exit code.
/// * `OnFailure` — restart only if the container exited non-zero.
/// * `Never`     — never restart.
///
/// # Examples
///
/// ```
/// use cave_home_kubelet_rs::api::RestartPolicy;
/// use cave_home_kubelet_rs::restart::should_restart;
///
/// assert!(should_restart(RestartPolicy::Always, 0));
/// assert!(should_restart(RestartPolicy::OnFailure, 1));
/// assert!(!should_restart(RestartPolicy::OnFailure, 0));
/// assert!(!should_restart(RestartPolicy::Never, 1));
/// ```
#[must_use]
pub const fn should_restart(policy: RestartPolicy, last_exit_code: i32) -> bool {
    match policy {
        RestartPolicy::Always => true,
        RestartPolicy::OnFailure => last_exit_code != 0,
        RestartPolicy::Never => false,
    }
}

/// The CrashLoopBackOff delay (ms) before restart attempt number `attempt`.
///
/// `attempt` is 1-based: the **first** restart (`attempt == 1`) incurs the base
/// delay, and each subsequent consecutive failure doubles it, capped at
/// [`BACKOFF_MAX_MS`]. `attempt == 0` is defined as no delay.
///
/// The curve (base 10s, cap 300s): 0, 10s, 20s, 40s, 80s, 160s, 300s, 300s, …
#[must_use]
pub fn backoff_delay(attempt: u32) -> u64 {
    if attempt == 0 {
        return 0;
    }
    // delay = base * 2^(attempt-1), saturating, capped at max.
    let shift = attempt - 1;
    // Guard the shift so we never overflow; anything past the cap shift is max.
    let scaled = if shift >= 63 {
        u64::MAX
    } else {
        BACKOFF_BASE_MS.saturating_mul(1u64 << shift)
    };
    scaled.min(BACKOFF_MAX_MS)
}

/// Whether the container is currently held in CrashLoopBackOff: it wants to
/// restart but the back-off window since the last failure has not elapsed.
///
/// `now_ms` and `last_failure_ms` are caller-supplied monotonic millis; the
/// window is [`backoff_delay`]`(attempt)`. Returns `true` while still backing
/// off (must wait), `false` once the container may be restarted.
#[must_use]
pub fn in_crash_loop_backoff(now_ms: u64, last_failure_ms: u64, attempt: u32) -> bool {
    let delay = backoff_delay(attempt);
    let elapsed = now_ms.saturating_sub(last_failure_ms);
    elapsed < delay
}

/// Convenience: the absolute time (ms) at which the next restart becomes
/// eligible, given the last failure time and the attempt count.
#[must_use]
pub fn next_restart_at_ms(last_failure_ms: u64, attempt: u32) -> u64 {
    last_failure_ms.saturating_add(backoff_delay(attempt))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_restarts_on_any_exit() {
        assert!(should_restart(RestartPolicy::Always, 0));
        assert!(should_restart(RestartPolicy::Always, 1));
        assert!(should_restart(RestartPolicy::Always, 137));
    }

    #[test]
    fn onfailure_restarts_only_on_nonzero() {
        assert!(!should_restart(RestartPolicy::OnFailure, 0));
        assert!(should_restart(RestartPolicy::OnFailure, 1));
        assert!(should_restart(RestartPolicy::OnFailure, -1));
    }

    #[test]
    fn never_never_restarts() {
        assert!(!should_restart(RestartPolicy::Never, 0));
        assert!(!should_restart(RestartPolicy::Never, 1));
    }

    #[test]
    fn backoff_zero_attempt_is_immediate() {
        assert_eq!(backoff_delay(0), 0);
    }

    #[test]
    fn backoff_curve_doubles() {
        assert_eq!(backoff_delay(1), 10_000);
        assert_eq!(backoff_delay(2), 20_000);
        assert_eq!(backoff_delay(3), 40_000);
        assert_eq!(backoff_delay(4), 80_000);
        assert_eq!(backoff_delay(5), 160_000);
    }

    #[test]
    fn backoff_caps_at_max() {
        // 10s * 2^5 = 320s would exceed the 300s cap.
        assert_eq!(backoff_delay(6), BACKOFF_MAX_MS);
        assert_eq!(backoff_delay(7), BACKOFF_MAX_MS);
        assert_eq!(backoff_delay(100), BACKOFF_MAX_MS);
    }

    #[test]
    fn backoff_huge_attempt_does_not_overflow() {
        // attempt large enough to overflow the shift must still cap, not panic.
        assert_eq!(backoff_delay(u32::MAX), BACKOFF_MAX_MS);
    }

    #[test]
    fn crash_loop_backoff_holds_until_window_elapses() {
        // attempt 2 -> 20s window. last failure at t=1000ms.
        assert!(in_crash_loop_backoff(1000, 1000, 2)); // 0 elapsed
        assert!(in_crash_loop_backoff(20_000, 1000, 2)); // 19s elapsed < 20s
        assert!(!in_crash_loop_backoff(21_000, 1000, 2)); // 20s elapsed == window
        assert!(!in_crash_loop_backoff(100_000, 1000, 2)); // well past
    }

    #[test]
    fn crash_loop_backoff_zero_attempt_never_holds() {
        assert!(!in_crash_loop_backoff(0, 0, 0));
    }

    #[test]
    fn next_restart_at_adds_delay() {
        assert_eq!(next_restart_at_ms(5_000, 1), 15_000); // 5s + 10s
        assert_eq!(next_restart_at_ms(5_000, 3), 45_000); // 5s + 40s
    }

    #[test]
    fn next_restart_at_saturates() {
        assert_eq!(next_restart_at_ms(u64::MAX, 1), u64::MAX);
    }
}
