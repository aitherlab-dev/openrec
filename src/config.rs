use std::fs;
use std::path::PathBuf;

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

        let config: Self = serde_json::from_str(&content)
            .with_context(|| format!("failed to parse config from {}", path.display()))?;

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

    pub fn recordings_dir(&self) -> Result<PathBuf> {
        fs::create_dir_all(&self.recordings_dir)
            .with_context(|| {
                format!(
                    "failed to create recordings dir {}",
                    self.recordings_dir.display()
                )
            })?;

        Ok(self.recordings_dir.clone())
    }
}
