use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use anyhow::{bail, Context, Result};

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
}

impl FfmpegEncoder {
    pub fn is_available() -> bool {
        Command::new("which")
            .arg("ffmpeg")
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

        Ok(Self { child })
    }

    pub fn write_frame(&mut self, data: &[u8]) -> Result<()> {
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
        drop(self.child.stdin.take());

        let output = self
            .child
            .wait_with_output()
            .context("failed to wait for ffmpeg")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("ffmpeg exited with {}: {}", output.status, stderr);
        }

        Ok(())
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
}
