// SPDX-License-Identifier: Apache-2.0
//! Pure (no-IO) builder for the ffmpeg argv we use to ingest an RTSP
//! source and emit raw YUV420p frames on stdout.
//!
//! Upstream: blakeblackshear/frigate@416a9b7692e052be98ad503704d26c7ef7a4c88d
//! :: frigate/ffmpeg_presets.py :: `parse_preset_input` + the per-camera
//! input/output argument lists assembled by `frigate.config.ffmpeg_args`.
//!
//! Frigate's argument list, when stripped to the Phase 1 essentials:
//!     ffmpeg \
//!       -hide_banner -loglevel warning -avoid_negative_ts make_zero \
//!       -fflags +genpts+discardcorrupt -rtsp_transport tcp \
//!       -timeout 5000000 -use_wallclock_as_timestamps 1 \
//!       -i <rtsp_url> \
//!       -f rawvideo -pix_fmt yuv420p \
//!       -s <W>x<H> -r <FPS> -an pipe:1
//!
//! Everything else (hwaccel flags, audio passthrough, segment writer
//! outputs) lives in [[unmapped]] entries.

use crate::config::CameraConfig;

/// Concrete invocation: program + argv, ready for
/// `tokio::process::Command::new(...).args(...)`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FfmpegCommand {
    /// Program to run (`ffmpeg` by default — overridable via env in the
    /// runtime path).
    pub program: String,
    /// Argument vector (excludes argv[0]).
    pub args: Vec<String>,
}

/// Compose the ffmpeg argv for one camera. Pure function — easy to test.
#[must_use]
pub fn build_capture_argv(cfg: &CameraConfig) -> FfmpegCommand {
    let args = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "warning".into(),
        "-avoid_negative_ts".into(),
        "make_zero".into(),
        "-fflags".into(),
        "+genpts+discardcorrupt".into(),
        "-rtsp_transport".into(),
        "tcp".into(),
        "-timeout".into(),
        "5000000".into(),
        "-use_wallclock_as_timestamps".into(),
        "1".into(),
        "-i".into(),
        cfg.source.clone(),
        "-f".into(),
        "rawvideo".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-s".into(),
        format!("{w}x{h}", w = cfg.width, h = cfg.height),
        "-r".into(),
        format!("{fps}", fps = cfg.fps),
        "-an".into(),
        "pipe:1".into(),
    ];
    FfmpegCommand {
        program: "ffmpeg".into(),
        args,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DetectorKind, MotionConfig, RecordConfig};

    fn cam() -> CameraConfig {
        CameraConfig {
            name: "front".into(),
            source: "rtsp://cam.local/stream0".into(),
            width: 1280,
            height: 720,
            fps: 5,
            detector: DetectorKind::CpuYolo,
            motion: MotionConfig::default(),
            record: RecordConfig::default(),
        }
    }

    #[test]
    fn argv_includes_rtsp_url_after_i_flag() {
        let cmd = build_capture_argv(&cam());
        let i_idx = cmd
            .args
            .iter()
            .position(|a| a == "-i")
            .expect("argv contains -i");
        assert_eq!(cmd.args[i_idx + 1], "rtsp://cam.local/stream0");
    }

    #[test]
    fn argv_requests_yuv420p_pipe_output() {
        let cmd = build_capture_argv(&cam());
        assert!(cmd.args.iter().any(|a| a == "rawvideo"));
        assert!(cmd.args.iter().any(|a| a == "yuv420p"));
        assert!(cmd.args.iter().any(|a| a == "pipe:1"));
        let s_idx = cmd
            .args
            .iter()
            .position(|a| a == "-s")
            .expect("argv contains -s");
        assert_eq!(cmd.args[s_idx + 1], "1280x720");
    }

    #[test]
    fn argv_passes_fps_through() {
        let mut c = cam();
        c.fps = 10;
        let cmd = build_capture_argv(&c);
        let r_idx = cmd
            .args
            .iter()
            .position(|a| a == "-r")
            .expect("argv contains -r");
        assert_eq!(cmd.args[r_idx + 1], "10");
    }

    #[test]
    fn argv_uses_tcp_rtsp_transport() {
        let cmd = build_capture_argv(&cam());
        let t_idx = cmd
            .args
            .iter()
            .position(|a| a == "-rtsp_transport")
            .expect("argv contains -rtsp_transport");
        assert_eq!(cmd.args[t_idx + 1], "tcp");
    }

    #[test]
    fn argv_disables_audio() {
        let cmd = build_capture_argv(&cam());
        assert!(cmd.args.iter().any(|a| a == "-an"));
    }
}
