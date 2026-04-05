use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::capture::cursor::CursorPosition;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub source_video: PathBuf,
    pub cursor_data: Vec<CursorPosition>,
    pub duration_ms: u64,
    pub zoom_regions: Vec<ZoomRegion>,
    pub annotations: Vec<Annotation>,
    pub trim_segments: Vec<TrimSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ZoomRegion {
    pub start_ms: u64,
    pub end_ms: u64,
    pub level: f32,
    pub focus_x: f32,
    pub focus_y: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Annotation {
    pub kind: AnnotationKind,
    pub start_ms: u64,
    pub end_ms: u64,
    pub x: f32,
    pub y: f32,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AnnotationKind {
    Text,
    Arrow,
    Image,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrimSegment {
    pub start_ms: u64,
    pub end_ms: u64,
}

impl Project {
    pub fn new(name: impl Into<String>, source_video: PathBuf, duration_ms: u64) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            created_at: now.clone(),
            updated_at: now,
            source_video,
            cursor_data: Vec::new(),
            duration_ms,
            zoom_regions: Vec::new(),
            annotations: Vec::new(),
            trim_segments: Vec::new(),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create dir {}", parent.display()))?;
        }

        let content = serde_json::to_string_pretty(self)
            .context("failed to serialize project")?;

        fs::write(path, content)
            .with_context(|| format!("failed to write project to {}", path.display()))?;

        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read project from {}", path.display()))?;

        let project: Self = serde_json::from_str(&content)
            .with_context(|| format!("failed to parse project from {}", path.display()))?;

        Ok(project)
    }

    pub fn project_dir() -> Result<PathBuf> {
        let dirs = ProjectDirs::from("", "", "openrec")
            .context("cannot determine project directory")?;
        let path = dirs.data_dir().join("projects");
        fs::create_dir_all(&path)
            .with_context(|| format!("failed to create projects dir {}", path.display()))?;
        Ok(path)
    }
}
