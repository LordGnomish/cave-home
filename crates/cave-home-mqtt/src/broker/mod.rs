//! The cave-home MQTT broker decision core — I/O-free.
//!
//! Everything here is pure: packets in, [`Action`]s out. The async TCP /
//! WebSocket / TLS listeners (behind the `runtime` feature) drive this
//! core but contain no protocol logic, which keeps the broker fully
//! unit-testable without sockets.

pub mod auth;
pub mod retain;
pub mod topic;
