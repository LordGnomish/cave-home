// SPDX-License-Identifier: Apache-2.0
//! Birdseye — multi-camera mosaic.
//!
//! Upstream: blakeblackshear/frigate@416a9b7692e052be98ad503704d26c7ef7a4c88d
//! :: frigate/output/birdseye.py :: `Birdseye._update_birdseye_frame`.
//!
//! Frigate composes a single output frame from the most-recent frame
//! of each active camera in a grid. The grid geometry is chosen to fit
//! the active-camera count into the closest-to-square cell layout
//! without distorting individual aspect ratios.

use crate::capture::rtsp::YuvFrame;

/// One cell in the mosaic: position + source frame slot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MosaicCell {
    /// Pixel x of cell top-left.
    pub x: u32,
    /// Pixel y of cell top-left.
    pub y: u32,
    /// Cell width.
    pub w: u32,
    /// Cell height.
    pub h: u32,
    /// Source camera key, if any (None -> blank tile).
    pub camera: Option<String>,
}

/// Computed mosaic layout — grid + per-cell positions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BirdseyeLayout {
    /// Composite width.
    pub width: u32,
    /// Composite height.
    pub height: u32,
    /// Cells (length == cols * rows).
    pub cells: Vec<MosaicCell>,
    /// Columns.
    pub cols: u32,
    /// Rows.
    pub rows: u32,
}

/// Compute the grid geometry Frigate uses: the smallest `cols*rows`
/// square ≥ camera count.
#[must_use]
pub fn plan_layout(
    cameras: &[String],
    out_width: u32,
    out_height: u32,
) -> BirdseyeLayout {
    let n = cameras.len() as u32;
    let cols = if n == 0 { 1 } else { ceil_sqrt(n) };
    let rows = if cols == 0 { 1 } else { n.div_ceil(cols) };
    let rows = rows.max(1);
    let cell_w = out_width / cols.max(1);
    let cell_h = out_height / rows.max(1);
    let mut cells = Vec::with_capacity((cols * rows) as usize);
    for r in 0..rows {
        for c in 0..cols {
            let idx = (r * cols + c) as usize;
            let camera = cameras.get(idx).cloned();
            cells.push(MosaicCell {
                x: c * cell_w,
                y: r * cell_h,
                w: cell_w,
                h: cell_h,
                camera,
            });
        }
    }
    BirdseyeLayout {
        width: out_width,
        height: out_height,
        cells,
        cols,
        rows,
    }
}

fn ceil_sqrt(n: u32) -> u32 {
    if n == 0 {
        return 1;
    }
    let s = (n as f32).sqrt().ceil() as u32;
    s.max(1)
}

/// Compose: for each named cell, copy a downscaled Y plane into the
/// output buffer. Returns the composite Y plane (`width * height` bytes).
///
/// Frigate composes in YUV420p so the U/V planes have to follow the same
/// downscale; Phase 1 outputs the Y plane only — the Portal's HLS
/// preview re-encodes anyway. Composing all three planes is a
/// straightforward extension once the muxer lands (Phase 1b).
#[must_use]
pub fn compose_y_plane(
    layout: &BirdseyeLayout,
    sources: &[(String, YuvFrame)],
) -> Vec<u8> {
    let mut out = vec![0_u8; (layout.width * layout.height) as usize];
    for cell in &layout.cells {
        let cam = match &cell.camera {
            Some(c) => c,
            None => continue,
        };
        let frame = sources.iter().find(|(k, _)| k == cam).map(|(_, f)| f);
        let Some(frame) = frame else { continue };
        if cell.w == 0 || cell.h == 0 {
            continue;
        }
        // Nearest-neighbour downscale of the source Y plane into the cell.
        let src_y = frame.y_plane();
        let sw = frame.width;
        let sh = frame.height;
        for cy in 0..cell.h {
            for cx in 0..cell.w {
                // Map cell pixel (cx, cy) -> source pixel.
                let sx = (u64::from(cx) * u64::from(sw) / u64::from(cell.w)) as u32;
                let sy = (u64::from(cy) * u64::from(sh) / u64::from(cell.h)) as u32;
                let s_idx = (sy * sw + sx) as usize;
                let d_idx = ((cell.y + cy) * layout.width + (cell.x + cx)) as usize;
                if let (Some(&s), Some(d)) = (src_y.get(s_idx), out.get_mut(d_idx)) {
                    *d = s;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_camera_fills_the_whole_canvas() {
        let layout = plan_layout(&["front".into()], 320, 240);
        assert_eq!(layout.cols, 1);
        assert_eq!(layout.rows, 1);
        assert_eq!(layout.cells.len(), 1);
        let c = &layout.cells[0];
        assert_eq!((c.x, c.y, c.w, c.h), (0, 0, 320, 240));
        assert_eq!(c.camera.as_deref(), Some("front"));
    }

    #[test]
    fn two_cameras_lay_out_as_two_by_one() {
        let layout = plan_layout(&["a".into(), "b".into()], 320, 240);
        assert_eq!(layout.cols, 2);
        assert_eq!(layout.rows, 1);
        assert_eq!(layout.cells.len(), 2);
        assert_eq!(layout.cells[0].camera.as_deref(), Some("a"));
        assert_eq!(layout.cells[1].camera.as_deref(), Some("b"));
    }

    #[test]
    fn four_cameras_lay_out_as_two_by_two() {
        let layout = plan_layout(
            &["a".into(), "b".into(), "c".into(), "d".into()],
            400,
            400,
        );
        assert_eq!(layout.cols, 2);
        assert_eq!(layout.rows, 2);
    }

    #[test]
    fn five_cameras_lay_out_as_three_by_two_with_blank_cell() {
        let cams = vec!["a".into(), "b".into(), "c".into(), "d".into(), "e".into()];
        let layout = plan_layout(&cams, 600, 400);
        assert_eq!(layout.cols, 3);
        assert_eq!(layout.rows, 2);
        assert_eq!(layout.cells.len(), 6);
        assert!(layout.cells.iter().any(|c| c.camera.is_none()));
    }

    #[test]
    fn compose_copies_source_y_plane_into_cell() {
        let layout = plan_layout(&["front".into()], 4, 4);
        let frame = YuvFrame {
            width: 4,
            height: 4,
            seq: 0,
            data: {
                let mut d = vec![0_u8; 24];
                for (i, p) in d.iter_mut().take(16).enumerate() {
                    *p = u8::try_from(i * 10).unwrap_or(0);
                }
                d
            },
        };
        let out = compose_y_plane(&layout, &[("front".into(), frame)]);
        assert_eq!(out.len(), 16);
        assert_eq!(out[0], 0);
        assert_eq!(out[5], 50);
    }
}
