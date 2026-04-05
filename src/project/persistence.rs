use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::capture::cursor::CursorPosition;
use crate::editor::speed::SpeedSegment;

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
    #[serde(default)]
    pub speed_segments: Vec<SpeedSegment>,
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
            speed_segments: Vec::new(),
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

    pub fn list_projects() -> Result<Vec<(String, PathBuf)>> {
        let dir = Self::project_dir()?;
        let mut projects = Vec::new();

        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(projects),
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("failed to read projects dir {}", dir.display()))
            }
        };

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                if let Ok(project) = Self::load(&path) {
                    projects.push((project.name, path));
                }
            }
        }

        Ok(projects)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_new() {
        let project = Project::new("Test Recording", PathBuf::from("/tmp/video.mp4"), 5000);

        assert!(!project.id.is_nil());
        assert_eq!(project.name, "Test Recording");
        assert_eq!(project.source_video, PathBuf::from("/tmp/video.mp4"));
        assert_eq!(project.duration_ms, 5000);
        assert!(project.cursor_data.is_empty());
        assert!(project.zoom_regions.is_empty());
        assert!(project.annotations.is_empty());
        assert!(project.trim_segments.is_empty());
        assert!(!project.created_at.is_empty());
        assert_eq!(project.created_at, project.updated_at);
    }

    #[test]
    fn test_project_save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_project.json");

        let project = Project::new("Roundtrip Test", PathBuf::from("/tmp/rec.mp4"), 10000);
        let original_id = project.id;

        project.save(&path).unwrap();
        let loaded = Project::load(&path).unwrap();

        assert_eq!(loaded.id, original_id);
        assert_eq!(loaded.name, "Roundtrip Test");
        assert_eq!(loaded.source_video, PathBuf::from("/tmp/rec.mp4"));
        assert_eq!(loaded.duration_ms, 10000);
        assert_eq!(loaded.created_at, project.created_at);
    }

    #[test]
    fn test_project_with_zoom_regions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zoom.json");

        let mut project = Project::new("Zoom", PathBuf::from("/tmp/v.mp4"), 3000);
        project.zoom_regions.push(ZoomRegion {
            start_ms: 100,
            end_ms: 500,
            level: 2.0,
            focus_x: 0.5,
            focus_y: 0.3,
        });
        project.zoom_regions.push(ZoomRegion {
            start_ms: 1000,
            end_ms: 2000,
            level: 1.5,
            focus_x: 0.8,
            focus_y: 0.9,
        });

        project.save(&path).unwrap();
        let loaded = Project::load(&path).unwrap();

        assert_eq!(loaded.zoom_regions.len(), 2);
        assert_eq!(loaded.zoom_regions[0], project.zoom_regions[0]);
        assert_eq!(loaded.zoom_regions[1], project.zoom_regions[1]);
    }

    #[test]
    fn test_project_with_annotations() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("annot.json");

        let mut project = Project::new("Annotated", PathBuf::from("/tmp/v.mp4"), 5000);
        project.annotations.push(Annotation {
            kind: AnnotationKind::Text,
            start_ms: 0,
            end_ms: 1000,
            x: 100.0,
            y: 200.0,
            content: "Hello world".into(),
        });
        project.annotations.push(Annotation {
            kind: AnnotationKind::Arrow,
            start_ms: 500,
            end_ms: 1500,
            x: 300.0,
            y: 400.0,
            content: String::new(),
        });

        project.save(&path).unwrap();
        let loaded = Project::load(&path).unwrap();

        assert_eq!(loaded.annotations.len(), 2);
        assert_eq!(loaded.annotations[0], project.annotations[0]);
        assert_eq!(loaded.annotations[1].kind, AnnotationKind::Arrow);
    }

    #[test]
    fn test_project_with_trim_segments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trim.json");

        let mut project = Project::new("Trimmed", PathBuf::from("/tmp/v.mp4"), 8000);
        project.trim_segments.push(TrimSegment {
            start_ms: 0,
            end_ms: 500,
        });
        project.trim_segments.push(TrimSegment {
            start_ms: 7000,
            end_ms: 8000,
        });

        project.save(&path).unwrap();
        let loaded = Project::load(&path).unwrap();

        assert_eq!(loaded.trim_segments.len(), 2);
        assert_eq!(loaded.trim_segments[0], project.trim_segments[0]);
        assert_eq!(loaded.trim_segments[1], project.trim_segments[1]);
    }

    #[test]
    fn test_list_projects_empty() {
        let dir = tempfile::tempdir().unwrap();
        let entries = fs::read_dir(dir.path()).unwrap();
        assert_eq!(entries.count(), 0);
    }

    #[test]
    fn test_project_serialization() {
        let project = Project::new("Serialize Test", PathBuf::from("/tmp/v.mp4"), 1000);
        let json = serde_json::to_string_pretty(&project).unwrap();

        assert!(json.contains("\"name\": \"Serialize Test\""));
        assert!(json.contains("\"duration_ms\": 1000"));
        assert!(json.contains("\"zoom_regions\": []"));
        assert!(json.contains("\"annotations\": []"));
        assert!(json.contains("\"trim_segments\": []"));

        // Проверяем что JSON парсится обратно
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["id"].is_string());
        assert!(parsed["created_at"].is_string());
    }
}
