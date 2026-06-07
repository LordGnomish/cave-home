// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 cave-home contributors
//! Exponential reconnect backoff for the WebSocket subscription.

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn first_delay_is_base() {
        let mut b = Backoff::new(Duration::from_secs(1), Duration::from_secs(60));
        assert_eq!(b.next_delay(), Duration::from_secs(1));
    }

    #[test]
    fn delay_doubles_each_attempt() {
        let mut b = Backoff::new(Duration::from_secs(1), Duration::from_secs(60));
        assert_eq!(b.next_delay(), Duration::from_secs(1));
        assert_eq!(b.next_delay(), Duration::from_secs(2));
        assert_eq!(b.next_delay(), Duration::from_secs(4));
    }

    #[test]
    fn caps_at_max() {
        let mut b = Backoff::new(Duration::from_secs(1), Duration::from_secs(5));
        for _ in 0..10 {
            assert!(b.next_delay() <= Duration::from_secs(5));
        }
        assert_eq!(b.next_delay(), Duration::from_secs(5));
    }

    #[test]
    fn reset_returns_to_base() {
        let mut b = Backoff::new(Duration::from_secs(1), Duration::from_secs(60));
        b.next_delay();
        b.next_delay();
        b.reset();
        assert_eq!(b.attempt(), 0);
        assert_eq!(b.next_delay(), Duration::from_secs(1));
    }

    #[test]
    fn attempt_counts_up() {
        let mut b = Backoff::new(Duration::from_millis(100), Duration::from_secs(1));
        assert_eq!(b.attempt(), 0);
        b.next_delay();
        assert_eq!(b.attempt(), 1);
    }
}
