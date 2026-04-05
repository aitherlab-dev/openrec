use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

/// Один декодированный кадр (RGBA).
#[derive(Debug, Clone)]
pub struct PreviewFrame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub timestamp_ms: u64,
}

impl PreviewFrame {
    pub fn expected_size(&self) -> usize {
        (self.width * self.height * 4) as usize
    }
}

/// Состояние виджета превью.
pub struct PreviewState {
    pub current_frame: Option<PreviewFrame>,
    pub video_path: Option<PathBuf>,
    pub video_width: u32,
    pub video_height: u32,
}

impl Default for PreviewState {
    fn default() -> Self {
        Self {
            current_frame: None,
            video_path: None,
            video_width: 1920,
            video_height: 1080,
        }
    }
}

/// Извлекает один кадр из видео по временной метке через ffmpeg.
///
/// Запускает ffmpeg как subprocess, выводит raw RGBA в stdout.
pub fn extract_frame(
    video_path: &Path,
    timestamp_ms: u64,
    width: u32,
    height: u32,
) -> Result<PreviewFrame> {
    if !video_path.exists() {
        bail!("video file not found: {}", video_path.display());
    }

    let timestamp_s = timestamp_ms as f64 / 1000.0;
    let size = format!("{width}x{height}");

    let output = Command::new("ffmpeg")
        .args([
            "-ss",
            &format!("{timestamp_s:.3}"),
            "-i",
        ])
        .arg(video_path)
        .args([
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-s",
            &size,
            "pipe:1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("failed to run ffmpeg")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("ffmpeg failed: {}", stderr);
    }

    let expected_size = (width * height * 4) as usize;
    if output.stdout.len() != expected_size {
        bail!(
            "unexpected frame data size: got {}, expected {}",
            output.stdout.len(),
            expected_size
        );
    }

    Ok(PreviewFrame {
        data: output.stdout,
        width,
        height,
        timestamp_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preview_frame_creation() {
        let frame = PreviewFrame {
            data: vec![0u8; 320 * 240 * 4],
            width: 320,
            height: 240,
            timestamp_ms: 1500,
        };
        assert_eq!(frame.width, 320);
        assert_eq!(frame.height, 240);
        assert_eq!(frame.timestamp_ms, 1500);
    }

    #[test]
    fn test_frame_data_size() {
        let w = 640u32;
        let h = 480u32;
        let frame = PreviewFrame {
            data: vec![0u8; (w * h * 4) as usize],
            width: w,
            height: h,
            timestamp_ms: 0,
        };
        assert_eq!(frame.data.len(), frame.expected_size());
        assert_eq!(frame.expected_size(), (w * h * 4) as usize);
    }

    #[test]
    fn test_preview_state_default() {
        let state = PreviewState::default();
        assert!(state.current_frame.is_none());
        assert!(state.video_path.is_none());
        assert_eq!(state.video_width, 1920);
        assert_eq!(state.video_height, 1080);
    }

    #[test]
    fn test_extract_frame_nonexistent_file() {
        let result = extract_frame(
            Path::new("/tmp/nonexistent_video_openrec_test.mp4"),
            0,
            320,
            240,
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not found"),
            "expected 'not found' in error: {err}"
        );
    }

    #[test]
    #[ignore] // requires a real video file
    fn test_extract_frame_from_real_video() {
        // Place a test video at /tmp/openrec_test.mp4 to run this test
        let path = Path::new("/tmp/openrec_test.mp4");
        if !path.exists() {
            return;
        }
        let frame = extract_frame(path, 0, 320, 240).unwrap();
        assert_eq!(frame.width, 320);
        assert_eq!(frame.height, 240);
        assert_eq!(frame.data.len(), frame.expected_size());
    }
}
