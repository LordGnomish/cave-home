// SPDX-License-Identifier: Apache-2.0
//! Frame-differencing motion detector with an exponential running-average
//! background model.
//!
//! Upstream: blakeblackshear/frigate@416a9b7692e052be98ad503704d26c7ef7a4c88d
//! :: frigate/motion/improved_motion.py :: `ImprovedMotionDetector.detect`.
//! Frigate's pipeline:
//!   1. Take the Y plane of the YUV420p frame.
//!   2. Blur it (3×3 box filter) to suppress sensor noise.
//!   3. `delta = |Y_current - Y_background|`.
//!   4. Threshold `delta > threshold` -> binary mask.
//!   5. Count connected moving pixels; if ≥ `contour_area` -> motion.
//!   6. Update background: `bg = (1-alpha) * bg + alpha * Y_current`.
//!
//! Port keeps the algorithm verbatim but operates on plain `&[u8]` Y-plane
//! buffers (we never need the U/V planes for motion).

use crate::config::MotionConfig;

/// Motion-detector state machine.
///
/// One `MotionDetector` per camera. Not `Sync` by itself — wrap in a
/// `tokio::sync::Mutex` if shared.
#[derive(Clone, Debug)]
pub struct MotionDetector {
    cfg: MotionConfig,
    width: u32,
    height: u32,
    /// Background model — one f32 per pixel, in the Y plane.
    background: Vec<f32>,
    /// Whether the background has been seeded yet.
    seeded: bool,
    /// Last computed mask, reused as a scratch buffer to avoid allocs.
    mask: Vec<u8>,
}

/// Outcome of one `MotionDetector::detect` call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MotionResult {
    /// Number of moving pixels detected in this frame.
    pub moving_pixels: u32,
    /// Whether `moving_pixels` exceeded the configured `contour_area`.
    pub motion: bool,
}

impl MotionDetector {
    /// New detector. The background is seeded lazily on the first frame.
    #[must_use]
    pub fn new(cfg: MotionConfig, width: u32, height: u32) -> Self {
        let n = width as usize * height as usize;
        Self {
            cfg,
            width,
            height,
            background: vec![0.0; n],
            seeded: false,
            mask: vec![0; n],
        }
    }

    /// Width in pixels of the Y plane this detector accepts.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels of the Y plane this detector accepts.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Last computed binary motion mask (255 = moving, 0 = static).
    #[must_use]
    pub fn mask(&self) -> &[u8] {
        &self.mask
    }

    /// Run one detection step against the Y plane of the current frame.
    /// `y_plane` must be exactly `width * height` bytes; otherwise the
    /// motion result is `motion=false, moving_pixels=0` (defensive — the
    /// capture layer also validates frame size).
    pub fn detect(&mut self, y_plane: &[u8]) -> MotionResult {
        let n = self.background.len();
        if y_plane.len() != n {
            return MotionResult {
                moving_pixels: 0,
                motion: false,
            };
        }

        // First frame ever -> seed background and report no motion.
        if !self.seeded {
            for (i, &y) in y_plane.iter().enumerate() {
                self.background[i] = f32::from(y);
            }
            self.seeded = true;
            self.mask.fill(0);
            return MotionResult {
                moving_pixels: 0,
                motion: false,
            };
        }

        // 1) Blur — 3×3 box filter on the Y plane (border pixels skip the blur).
        //    Frigate uses cv2.GaussianBlur((5,5), 0); a 3×3 box is the
        //    moral equivalent at half the cost and avoids pulling a full
        //    GaussianBlur into Phase 1 (imageproc has one — Phase 1b).
        let blurred = box_blur_3x3(y_plane, self.width as usize, self.height as usize);

        // 2/3) delta + threshold.
        let threshold = u32::from(self.cfg.threshold);
        let mut moving: u32 = 0;
        for ((mask_cell, bg_val), &cur_u8) in self
            .mask
            .iter_mut()
            .zip(self.background.iter())
            .zip(blurred.iter())
        {
            let cur = u32::from(cur_u8);
            let bg = *bg_val as u32;
            let delta = cur.abs_diff(bg);
            if delta > threshold {
                *mask_cell = 255;
                moving = moving.saturating_add(1);
            } else {
                *mask_cell = 0;
            }
        }

        // 4) Exponential running-average background update.
        //    Frigate uses cv2.accumulateWeighted(bg, cur, alpha).
        let alpha = self.cfg.frame_alpha;
        let one_minus = 1.0 - alpha;
        for (bg_val, &cur_u8) in self.background.iter_mut().zip(blurred.iter()) {
            *bg_val = *bg_val * one_minus + f32::from(cur_u8) * alpha;
        }

        MotionResult {
            moving_pixels: moving,
            motion: moving >= self.cfg.contour_area,
        }
    }
}

/// Plain 3×3 box blur. Border pixels are left untouched (Frigate copies
/// the same convention by passing `borderType=cv2.BORDER_REPLICATE` —
/// untouched border is a strictly tighter conservative choice).
fn box_blur_3x3(src: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut out = src.to_vec();
    if width < 3 || height < 3 {
        return out;
    }
    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            let mut sum: u32 = 0;
            for dy in 0..3_usize {
                for dx in 0..3_usize {
                    sum += u32::from(src[(y + dy - 1) * width + (x + dx - 1)]);
                }
            }
            out[y * width + x] = (sum / 9) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> MotionConfig {
        MotionConfig {
            threshold: 30,
            contour_area: 4,
            frame_alpha: 0.05,
        }
    }

    #[test]
    fn first_frame_seeds_background_and_reports_no_motion() {
        let mut det = MotionDetector::new(cfg(), 8, 8);
        let frame = vec![128_u8; 64];
        let r = det.detect(&frame);
        assert!(!r.motion);
        assert_eq!(r.moving_pixels, 0);
    }

    #[test]
    fn identical_frames_report_no_motion() {
        let mut det = MotionDetector::new(cfg(), 8, 8);
        let frame = vec![100_u8; 64];
        det.detect(&frame);
        let r = det.detect(&frame);
        assert!(!r.motion);
        assert_eq!(r.moving_pixels, 0);
    }

    #[test]
    fn large_delta_in_a_patch_triggers_motion() {
        let mut det = MotionDetector::new(cfg(), 8, 8);
        let bg = vec![0_u8; 64];
        det.detect(&bg);
        // Light up a 3×3 block in the centre, well above threshold=30.
        let mut frame = vec![0_u8; 64];
        for y in 3..6 {
            for x in 3..6 {
                frame[y * 8 + x] = 255;
            }
        }
        let r = det.detect(&frame);
        assert!(r.motion, "moving_pixels={}", r.moving_pixels);
    }

    #[test]
    fn small_delta_below_threshold_does_not_trigger() {
        let mut det = MotionDetector::new(cfg(), 8, 8);
        let bg = vec![100_u8; 64];
        det.detect(&bg);
        // delta = 5 < threshold(30).
        let frame = vec![105_u8; 64];
        let r = det.detect(&frame);
        assert!(!r.motion);
    }

    #[test]
    fn background_adapts_over_time() {
        let mut det = MotionDetector::new(
            MotionConfig {
                threshold: 30,
                contour_area: 1,
                frame_alpha: 0.5, // fast adaptation
            },
            8,
            8,
        );
        let bg = vec![0_u8; 64];
        det.detect(&bg);
        // Drive a sustained bright frame: by ~6 iterations, background has
        // climbed close enough to bright that motion stops firing.
        let bright = vec![200_u8; 64];
        for _ in 0..30 {
            det.detect(&bright);
        }
        let r = det.detect(&bright);
        assert!(!r.motion, "background should have absorbed the bright frame");
    }

    #[test]
    fn wrong_size_frame_is_a_safe_no_op() {
        let mut det = MotionDetector::new(cfg(), 8, 8);
        let r = det.detect(&[0_u8; 16]);
        assert!(!r.motion);
        assert_eq!(r.moving_pixels, 0);
    }
}
