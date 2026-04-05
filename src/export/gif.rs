use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use anyhow::{bail, Context, Result};
use log::warn;

#[derive(Debug, Clone)]
pub struct GifConfig {
    pub output_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub max_colors: u16,
    pub pixel_format: String,
}

impl Default for GifConfig {
    fn default() -> Self {
        Self {
            output_path: PathBuf::from("output.gif"),
            width: 640,
            height: 480,
            fps: 15,
            max_colors: 256,
            pixel_format: "bgra".to_string(),
        }
    }
}

pub struct GifEncoder {
    child: Child,
    expected_frame_size: usize,
    finished: bool,
}

impl GifEncoder {
    pub fn new(config: GifConfig) -> Result<Self> {
        let video_size = format!("{}x{}", config.width, config.height);
        let fps_str = config.fps.to_string();
        let expected_frame_size = (config.width * config.height * 4) as usize;

        // Однопроходный GIF: split → palettegen + paletteuse в одном filtergraph
        let filter = format!(
            "fps={fps},split[s0][s1];[s0]palettegen=max_colors={colors}[p];[s1][p]paletteuse=dither=bayer:bayer_scale=5",
            fps = config.fps,
            colors = config.max_colors,
        );

        let child = Command::new("ffmpeg")
            .args([
                "-y",
                "-f", "rawvideo",
                "-pixel_format", &config.pixel_format,
                "-video_size", &video_size,
                "-framerate", &fps_str,
                "-i", "pipe:0",
                "-lavfi", &filter,
                "-loop", "0",
            ])
            .arg(&config.output_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to spawn ffmpeg for GIF encoding")?;

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
            bail!("ffmpeg GIF encoder exited with {}", status);
        }

        Ok(())
    }
}

impl Drop for GifEncoder {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        drop(self.child.stdin.take());
        match self.child.wait() {
            Ok(status) => {
                if !status.success() {
                    warn!("ffmpeg GIF encoder exited with {} during drop", status);
                }
            }
            Err(e) => {
                warn!("failed to wait for ffmpeg GIF encoder during drop: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gif_config_default() {
        let config = GifConfig::default();
        assert_eq!(config.width, 640);
        assert_eq!(config.height, 480);
        assert_eq!(config.fps, 15);
        assert_eq!(config.max_colors, 256);
        assert_eq!(config.pixel_format, "bgra");
        assert_eq!(config.output_path, PathBuf::from("output.gif"));
    }

    #[test]
    fn test_gif_encode_single_frame() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let output = dir.path().join("test_single.gif");

        let config = GifConfig {
            output_path: output.clone(),
            width: 160,
            height: 120,
            fps: 10,
            ..Default::default()
        };

        let frame_size = (config.width * config.height * 4) as usize;
        let frame = vec![128u8; frame_size];

        let mut encoder = GifEncoder::new(config)?;
        encoder.write_frame(&frame)?;
        encoder.finish()?;

        assert!(output.exists(), "GIF file must exist");
        assert!(
            std::fs::metadata(&output)?.len() > 0,
            "GIF file must not be empty"
        );

        Ok(())
    }

    #[test]
    fn test_gif_encode_multiple_frames() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let output = dir.path().join("test_multi.gif");

        let config = GifConfig {
            output_path: output.clone(),
            width: 160,
            height: 120,
            fps: 15,
            ..Default::default()
        };

        let frame_size = (config.width * config.height * 4) as usize;

        let mut encoder = GifEncoder::new(config)?;
        for i in 0..15u8 {
            let frame = vec![i * 17; frame_size];
            encoder.write_frame(&frame)?;
        }
        encoder.finish()?;

        assert!(output.exists(), "GIF file must exist");
        assert!(
            std::fs::metadata(&output)?.len() > 0,
            "GIF file must not be empty"
        );

        Ok(())
    }

    #[test]
    fn test_gif_custom_fps() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let output = dir.path().join("test_fps20.gif");

        let config = GifConfig {
            output_path: output.clone(),
            width: 160,
            height: 120,
            fps: 20,
            max_colors: 128,
            ..Default::default()
        };

        let frame_size = (config.width * config.height * 4) as usize;
        let frame = vec![200u8; frame_size];

        let mut encoder = GifEncoder::new(config)?;
        for _ in 0..5 {
            encoder.write_frame(&frame)?;
        }
        encoder.finish()?;

        assert!(output.exists(), "GIF file must exist");

        Ok(())
    }

    #[test]
    fn test_gif_frame_size_mismatch() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let output = dir.path().join("test_mismatch.gif");

        let config = GifConfig {
            output_path: output,
            width: 160,
            height: 120,
            ..Default::default()
        };

        let mut encoder = GifEncoder::new(config)?;
        let wrong_frame = vec![0u8; 100];
        let err = encoder.write_frame(&wrong_frame).unwrap_err();
        assert!(
            err.to_string().contains("frame size mismatch"),
            "expected frame size mismatch error, got: {}",
            err
        );

        Ok(())
    }
}
