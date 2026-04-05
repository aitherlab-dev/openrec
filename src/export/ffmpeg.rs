use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use anyhow::{bail, Context, Result};
use log::warn;

#[derive(Debug, Clone)]
pub enum Codec {
    H264,
    H265,
    AV1,
}

#[derive(Debug, Clone)]
pub struct EncoderConfig {
    pub output_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub codec: Codec,
    pub bitrate: Option<u32>,
    pub pixel_format: String,
}

impl Default for EncoderConfig {
    fn default() -> Self {
        Self {
            output_path: PathBuf::from("output.mp4"),
            width: 1920,
            height: 1080,
            fps: 60,
            codec: Codec::H264,
            bitrate: None,
            pixel_format: "bgra".to_string(),
        }
    }
}

pub struct FfmpegEncoder {
    child: Child,
    expected_frame_size: usize,
    finished: bool,
}

impl FfmpegEncoder {
    pub fn is_available() -> bool {
        Command::new("ffmpeg")
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    pub fn new(config: EncoderConfig) -> Result<Self> {
        let codec_args = match config.codec {
            Codec::H264 => vec!["-c:v", "libx264", "-preset", "fast", "-crf", "23"],
            Codec::H265 => vec!["-c:v", "libx265", "-preset", "fast", "-crf", "28"],
            Codec::AV1 => vec!["-c:v", "libsvtav1", "-crf", "30"],
        };

        let video_size = format!("{}x{}", config.width, config.height);
        let fps_str = config.fps.to_string();
        let expected_frame_size = (config.width * config.height * 4) as usize;

        let mut cmd = Command::new("ffmpeg");
        cmd.args([
            "-y",
            "-f",
            "rawvideo",
            "-pixel_format",
            &config.pixel_format,
            "-video_size",
            &video_size,
            "-framerate",
            &fps_str,
            "-i",
            "pipe:0",
        ]);
        cmd.args(&codec_args);

        if let Some(bitrate) = config.bitrate {
            cmd.args(["-b:v", &format!("{bitrate}k")]);
        }

        cmd.args(["-pix_fmt", "yuv420p"]);
        cmd.arg(&config.output_path);

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        let child = cmd.spawn().context("failed to spawn ffmpeg")?;

        Ok(Self {
            child,
            expected_frame_size,
            finished: false,
        })
    }

    pub fn write_frame(&mut self, data: &[u8]) -> Result<()> {
        if data.len() != self.expected_frame_size {
            bail!(
                "frame size mismatch: got {}, expected {}",
                data.len(),
                self.expected_frame_size
            );
        }
        let stdin = self
            .child
            .stdin
            .as_mut()
            .context("ffmpeg stdin not available")?;
        stdin
            .write_all(data)
            .context("failed to write frame to ffmpeg")?;
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        self.finished = true;
        drop(self.child.stdin.take());

        let status = self
            .child
            .wait()
            .context("failed to wait for ffmpeg")?;

        if !status.success() {
            bail!("ffmpeg exited with {}", status);
        }

        Ok(())
    }
}

impl Drop for FfmpegEncoder {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        drop(self.child.stdin.take());
        match self.child.wait() {
            Ok(status) => {
                if !status.success() {
                    warn!("ffmpeg exited with {} during drop", status);
                }
            }
            Err(e) => {
                warn!("failed to wait for ffmpeg during drop: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ffmpeg_available() {
        assert!(
            FfmpegEncoder::is_available(),
            "ffmpeg must be installed in PATH"
        );
    }

    #[test]
    fn test_encoder_config_default() {
        let config = EncoderConfig::default();
        assert_eq!(config.width, 1920);
        assert_eq!(config.height, 1080);
        assert_eq!(config.fps, 60);
        assert_eq!(config.pixel_format, "bgra");
        assert!(config.bitrate.is_none());
        assert_eq!(config.output_path, PathBuf::from("output.mp4"));
    }

    #[test]
    fn test_encode_single_frame() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let output = dir.path().join("test_single.mp4");

        let config = EncoderConfig {
            output_path: output.clone(),
            width: 320,
            height: 240,
            fps: 30,
            codec: Codec::H264,
            bitrate: None,
            pixel_format: "bgra".to_string(),
        };

        let frame_size = (config.width * config.height * 4) as usize;
        let frame = vec![0u8; frame_size];

        let mut encoder = FfmpegEncoder::new(config)?;
        encoder.write_frame(&frame)?;
        encoder.finish()?;

        assert!(output.exists(), "output file must exist");
        assert!(
            std::fs::metadata(&output)?.len() > 0,
            "output file must not be empty"
        );

        Ok(())
    }

    #[test]
    fn test_encode_multiple_frames() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let output = dir.path().join("test_multi.mp4");

        let config = EncoderConfig {
            output_path: output.clone(),
            width: 320,
            height: 240,
            fps: 30,
            codec: Codec::H264,
            bitrate: None,
            pixel_format: "bgra".to_string(),
        };

        let frame_size = (config.width * config.height * 4) as usize;
        let frame = vec![0u8; frame_size];

        let mut encoder = FfmpegEncoder::new(config)?;
        for _ in 0..30 {
            encoder.write_frame(&frame)?;
        }
        encoder.finish()?;

        assert!(output.exists(), "output file must exist");
        assert!(
            std::fs::metadata(&output)?.len() > 0,
            "output file must not be empty"
        );

        Ok(())
    }

    #[test]
    fn test_frame_size_mismatch() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let output = dir.path().join("test_mismatch.mp4");

        let config = EncoderConfig {
            output_path: output,
            width: 320,
            height: 240,
            fps: 30,
            codec: Codec::H264,
            bitrate: None,
            pixel_format: "bgra".to_string(),
        };

        let mut encoder = FfmpegEncoder::new(config)?;
        let wrong_frame = vec![0u8; 100];
        let err = encoder.write_frame(&wrong_frame).unwrap_err();
        assert!(
            err.to_string().contains("frame size mismatch"),
            "expected frame size mismatch error, got: {}",
            err
        );

        Ok(())
    }

    #[test]
    fn test_finish_without_frames() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let output = dir.path().join("test_empty.mp4");

        let config = EncoderConfig {
            output_path: output,
            width: 320,
            height: 240,
            fps: 30,
            codec: Codec::H264,
            bitrate: None,
            pixel_format: "bgra".to_string(),
        };

        let encoder = FfmpegEncoder::new(config)?;
        // finish() without frames should not panic — graceful shutdown
        let _ = encoder.finish();

        Ok(())
    }

    #[test]
    fn test_encoder_config_custom_bitrate() {
        let config = EncoderConfig {
            bitrate: Some(5000),
            ..Default::default()
        };
        assert_eq!(config.bitrate, Some(5000));
        assert_eq!(config.width, 1920);
        assert!(matches!(config.codec, Codec::H264));
    }

    #[test]
    fn test_encoder_config_all_codecs() {
        let h264 = EncoderConfig {
            codec: Codec::H264,
            ..Default::default()
        };
        let h265 = EncoderConfig {
            codec: Codec::H265,
            ..Default::default()
        };
        let av1 = EncoderConfig {
            codec: Codec::AV1,
            ..Default::default()
        };

        assert!(matches!(h264.codec, Codec::H264));
        assert!(matches!(h265.codec, Codec::H265));
        assert!(matches!(av1.codec, Codec::AV1));
    }

    #[test]
    #[ignore] // libsvtav1 may not be available
    fn test_encode_av1() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let output = dir.path().join("test_av1.mp4");

        let config = EncoderConfig {
            output_path: output.clone(),
            width: 320,
            height: 240,
            fps: 30,
            codec: Codec::AV1,
            bitrate: None,
            pixel_format: "bgra".to_string(),
        };

        let frame_size = (config.width * config.height * 4) as usize;
        let frame = vec![0u8; frame_size];

        let mut encoder = FfmpegEncoder::new(config)?;
        for _ in 0..10 {
            encoder.write_frame(&frame)?;
        }
        encoder.finish()?;

        assert!(output.exists(), "output file must exist");
        assert!(
            std::fs::metadata(&output)?.len() > 0,
            "output file must not be empty"
        );

        Ok(())
    }

    #[test]
    fn test_encode_custom_resolution() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let output = dir.path().join("test_720p.mp4");

        let config = EncoderConfig {
            output_path: output.clone(),
            width: 1280,
            height: 720,
            fps: 24,
            codec: Codec::H264,
            bitrate: Some(3000),
            pixel_format: "bgra".to_string(),
        };

        let frame_size = (config.width * config.height * 4) as usize;
        let frame = vec![0u8; frame_size];

        let mut encoder = FfmpegEncoder::new(config)?;
        for _ in 0..10 {
            encoder.write_frame(&frame)?;
        }
        encoder.finish()?;

        assert!(output.exists(), "output file must exist");
        assert!(
            std::fs::metadata(&output)?.len() > 0,
            "output file must not be empty"
        );

        Ok(())
    }

    #[test]
    fn test_encode_h265() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let output = dir.path().join("test_h265.mp4");

        let config = EncoderConfig {
            output_path: output.clone(),
            width: 320,
            height: 240,
            fps: 30,
            codec: Codec::H265,
            bitrate: None,
            pixel_format: "bgra".to_string(),
        };

        let frame_size = (config.width * config.height * 4) as usize;
        let frame = vec![0u8; frame_size];

        let mut encoder = FfmpegEncoder::new(config)?;
        for _ in 0..10 {
            encoder.write_frame(&frame)?;
        }
        encoder.finish()?;

        assert!(output.exists(), "output file must exist");
        assert!(
            std::fs::metadata(&output)?.len() > 0,
            "output file must not be empty"
        );

        Ok(())
    }
}
