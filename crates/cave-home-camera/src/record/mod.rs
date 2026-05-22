// SPDX-License-Identifier: Apache-2.0
//! Recording + event-clip writers.
//!
//! Upstream: blakeblackshear/frigate@416a9b7692e052be98ad503704d26c7ef7a4c88d
//! :: frigate/record/ :: `SegmentMaintainer`, `EventProcessor`,
//! `cleanup.py`.

pub mod event;
pub mod segment;

pub use event::{EventClip, EventClipWriter};
pub use segment::{Segment, SegmentLog};
