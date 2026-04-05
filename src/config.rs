use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_video_fps")]
    pub video_fps: u32,

    #[serde(default)]
    pub video_codec: VideoCodec,

    #[serde(default)]
    pub audio_input_device: Option<String>,

    #[serde(default = "default_true")]
    pub audio_system_capture: bool,

    #[serde(default)]
    pub webcam_enabled: bool,

    #[serde(default)]
    pub webcam_device: Option<String>,

    #[serde(default)]
    pub webcam_shape: WebcamShape,

    #[serde(default)]
    pub export_quality: ExportQuality,

    #[serde(default)]
    pub export_format: ExportFormat,

    #[serde(default = "default_recordings_dir")]
    pub recordings_dir: PathBuf,

    #[serde(default)]
    pub hotkeys: HotkeyConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    #[serde(default = "default_toggle_recording")]
    pub toggle_recording: String,

    #[serde(default = "default_cancel_recording")]
    pub cancel_recording: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub enum VideoCodec {
    #[default]
    H264,
    H265,
    AV1,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub enum WebcamShape {
    #[default]
    Circle,
    Rectangle,
    RoundedRect,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub enum ExportQuality {
    Low,
    Medium,
    #[default]
    High,
    Custom(u32),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub enum ExportFormat {
    #[default]
    Mp4,
    Gif,
}

fn default_video_fps() -> u32 {
    60
}

fn default_true() -> bool {
    true
}

fn default_recordings_dir() -> PathBuf {
    project_dirs()
        .map(|dirs| dirs.data_dir().join("recordings"))
        .unwrap_or_else(|| PathBuf::from("recordings"))
}

fn default_toggle_recording() -> String {
    "Super+Shift+R".to_string()
}

fn default_cancel_recording() -> String {
    "Escape".to_string()
}

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("", "", "openrec")
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            video_fps: default_video_fps(),
            video_codec: VideoCodec::default(),
            audio_input_device: None,
            audio_system_capture: true,
            webcam_enabled: false,
            webcam_device: None,
            webcam_shape: WebcamShape::default(),
            export_quality: ExportQuality::default(),
            export_format: ExportFormat::default(),
            recordings_dir: default_recordings_dir(),
            hotkeys: HotkeyConfig::default(),
        }
    }
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            toggle_recording: default_toggle_recording(),
            cancel_recording: default_cancel_recording(),
        }
    }
}

impl AppConfig {
    fn config_path() -> Result<PathBuf> {
        let dirs = project_dirs().context("cannot determine config directory")?;
        Ok(dirs.config_dir().join("config.json"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;

        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config from {}", path.display()))?;

        let mut config: Self = serde_json::from_str(&content)
            .with_context(|| format!("failed to parse config from {}", path.display()))?;

        config.validate();

        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create config dir {}", parent.display()))?;
        }

        let content = serde_json::to_string_pretty(self)
            .context("failed to serialize config")?;

        fs::write(&path, content)
            .with_context(|| format!("failed to write config to {}", path.display()))?;

        Ok(())
    }

    pub fn recordings_dir(&self) -> &Path {
        &self.recordings_dir
    }

    pub fn ensure_recordings_dir(&self) -> Result<&Path> {
        fs::create_dir_all(&self.recordings_dir)
            .with_context(|| {
                format!(
                    "failed to create recordings dir {}",
                    self.recordings_dir.display()
                )
            })?;

        Ok(&self.recordings_dir)
    }

    pub fn validate(&mut self) {
        self.video_fps = self.video_fps.clamp(1, 120);

        if let ExportQuality::Custom(ref mut bitrate) = self.export_quality {
            *bitrate = (*bitrate).clamp(100, 100_000);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.video_fps, 60);
        assert_eq!(cfg.video_codec, VideoCodec::H264);
        assert!(cfg.audio_input_device.is_none());
        assert!(cfg.audio_system_capture);
        assert!(!cfg.webcam_enabled);
        assert!(cfg.webcam_device.is_none());
        assert_eq!(cfg.webcam_shape, WebcamShape::Circle);
        assert_eq!(cfg.export_quality, ExportQuality::High);
        assert_eq!(cfg.export_format, ExportFormat::Mp4);
        assert_eq!(cfg.hotkeys.toggle_recording, "Super+Shift+R");
        assert_eq!(cfg.hotkeys.cancel_recording, "Escape");
    }

    #[test]
    fn test_serialize_deserialize() {
        let mut cfg = AppConfig::default();
        cfg.video_fps = 30;
        cfg.video_codec = VideoCodec::AV1;
        cfg.webcam_enabled = true;
        cfg.export_quality = ExportQuality::Custom(5000);

        let json = serde_json::to_string(&cfg).unwrap();
        let restored: AppConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.video_fps, 30);
        assert_eq!(restored.video_codec, VideoCodec::AV1);
        assert!(restored.webcam_enabled);
        assert_eq!(restored.export_quality, ExportQuality::Custom(5000));
    }

    #[test]
    fn test_deserialize_partial_json() {
        let json = r#"{"video_fps": 30, "webcam_enabled": true}"#;
        let cfg: AppConfig = serde_json::from_str(json).unwrap();

        assert_eq!(cfg.video_fps, 30);
        assert!(cfg.webcam_enabled);
        // остальные — default
        assert_eq!(cfg.video_codec, VideoCodec::H264);
        assert!(cfg.audio_system_capture);
        assert_eq!(cfg.export_quality, ExportQuality::High);
    }

    #[test]
    fn test_deserialize_empty_json() {
        let cfg: AppConfig = serde_json::from_str("{}").unwrap();
        let def = AppConfig::default();

        assert_eq!(cfg.video_fps, def.video_fps);
        assert_eq!(cfg.video_codec, def.video_codec);
        assert_eq!(cfg.audio_system_capture, def.audio_system_capture);
        assert_eq!(cfg.webcam_enabled, def.webcam_enabled);
        assert_eq!(cfg.export_quality, def.export_quality);
        assert_eq!(cfg.export_format, def.export_format);
    }

    #[test]
    fn test_validate_fps_too_high() {
        let mut cfg = AppConfig::default();
        cfg.video_fps = 999;
        cfg.validate();
        assert_eq!(cfg.video_fps, 120);
    }

    #[test]
    fn test_validate_fps_zero() {
        let mut cfg = AppConfig::default();
        cfg.video_fps = 0;
        cfg.validate();
        assert_eq!(cfg.video_fps, 1);
    }

    #[test]
    fn test_validate_custom_bitrate_zero() {
        let mut cfg = AppConfig::default();
        cfg.export_quality = ExportQuality::Custom(0);
        cfg.validate();
        assert_eq!(cfg.export_quality, ExportQuality::Custom(100));
    }

    #[test]
    fn test_validate_custom_bitrate_huge() {
        let mut cfg = AppConfig::default();
        cfg.export_quality = ExportQuality::Custom(999_999);
        cfg.validate();
        assert_eq!(cfg.export_quality, ExportQuality::Custom(100_000));
    }

    #[test]
    fn test_recordings_dir_getter() {
        let cfg = AppConfig::default();
        let path = cfg.recordings_dir();
        assert!(path.to_str().unwrap().contains("recordings"));
    }

    #[test]
    fn test_config_save_load_roundtrip() {
        let mut original = AppConfig::default();
        original.video_fps = 24;
        original.video_codec = VideoCodec::H265;
        original.webcam_enabled = true;
        original.webcam_shape = WebcamShape::RoundedRect;
        original.export_quality = ExportQuality::Custom(8000);
        original.export_format = ExportFormat::Gif;
        original.audio_system_capture = false;
        original.hotkeys.toggle_recording = "Ctrl+R".to_string();

        let json = serde_json::to_string_pretty(&original).unwrap();
        let restored: AppConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.video_fps, 24);
        assert_eq!(restored.video_codec, VideoCodec::H265);
        assert!(restored.webcam_enabled);
        assert_eq!(restored.webcam_shape, WebcamShape::RoundedRect);
        assert_eq!(restored.export_quality, ExportQuality::Custom(8000));
        assert_eq!(restored.export_format, ExportFormat::Gif);
        assert!(!restored.audio_system_capture);
        assert_eq!(restored.hotkeys.toggle_recording, "Ctrl+R");
    }
}
