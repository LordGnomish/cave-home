// SPDX-License-Identifier: Apache-2.0
//! Deterministic in-process detector. Phase 1 tests + the
//! `cavehomectl camera snapshot --detector mock` smoke flow run against
//! it; the same trait is implemented for real Coral / CPU-YOLO /
//! NVIDIA below.
//!
//! Upstream parallel: `frigate.detectors.plugins.fakedetector` (Frigate
//! ships a `FakeDetector` for the same purpose).

use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::detectors::{Detection, Detector};
use crate::error::CameraResult;

/// A pre-seeded detector: every `detect()` returns the next "script"
/// of detections from a queue, or an empty vector once the queue is
/// drained.
#[derive(Clone, Debug, Default)]
pub struct MockDetector {
    script: Arc<Mutex<Vec<Vec<Detection>>>>,
    input_w: u32,
    input_h: u32,
    calls: Arc<Mutex<u64>>,
}

impl MockDetector {
    /// New mock with `(w, h)` advertised as input shape.
    #[must_use]
    pub fn new(input_w: u32, input_h: u32) -> Self {
        Self {
            script: Arc::new(Mutex::new(Vec::new())),
            input_w,
            input_h,
            calls: Arc::new(Mutex::new(0)),
        }
    }

    /// Push a frame's worth of detections onto the script. The first
    /// frame popped is the first one pushed (FIFO).
    pub fn push_frame(&self, detections: Vec<Detection>) {
        self.script.lock().push(detections);
    }

    /// Number of `detect()` calls made against this mock.
    #[must_use]
    pub fn call_count(&self) -> u64 {
        *self.calls.lock()
    }
}

#[async_trait]
impl Detector for MockDetector {
    fn input_shape(&self) -> (u32, u32) {
        (self.input_w, self.input_h)
    }

    async fn detect(&self, _tensor_input: &[u8]) -> CameraResult<Vec<Detection>> {
        *self.calls.lock() += 1;
        let mut script = self.script.lock();
        if script.is_empty() {
            Ok(Vec::new())
        } else {
            Ok(script.remove(0))
        }
    }
}
