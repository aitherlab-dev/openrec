use crate::project::persistence::{Project, TrimSegment, ZoomRegion};

/// Инструмент редактора.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorTool {
    Select,
    Trim,
    Zoom,
    Annotate,
}

/// Выделение временного диапазона.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeSelection {
    pub start_ms: u64,
    pub end_ms: u64,
}

/// Состояние воспроизведения.
#[derive(Debug, Clone)]
pub struct PlaybackState {
    pub position_ms: u64,
    pub is_playing: bool,
    pub playback_speed: f32,
    pub loop_enabled: bool,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            position_ms: 0,
            is_playing: false,
            playback_speed: 1.0,
            loop_enabled: false,
        }
    }
}

/// Состояние таймлайна.
#[derive(Debug, Clone)]
pub struct TimelineState {
    pub zoom_level: f32,
    pub scroll_offset: f32,
    pub selection: Option<TimeSelection>,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            zoom_level: 1.0,
            scroll_offset: 0.0,
            selection: None,
        }
    }
}

/// Полное состояние редактора.
pub struct EditorState {
    pub project: Project,
    pub playback: PlaybackState,
    pub timeline: TimelineState,
    pub selected_tool: EditorTool,
}

impl EditorState {
    pub fn new(project: Project) -> Self {
        Self {
            project,
            playback: PlaybackState::default(),
            timeline: TimelineState::default(),
            selected_tool: EditorTool::Select,
        }
    }

    /// Перемотка к позиции (clamp к длительности).
    pub fn seek(&mut self, position_ms: u64) {
        self.playback.position_ms = position_ms.min(self.project.duration_ms);
    }

    /// Переключение play/pause.
    pub fn toggle_playback(&mut self) {
        self.playback.is_playing = !self.playback.is_playing;
    }

    /// Установка скорости воспроизведения (clamp 0.25..4.0).
    pub fn set_speed(&mut self, speed: f32) {
        self.playback.playback_speed = speed.clamp(0.25, 4.0);
    }

    /// Добавить trim-сегмент (обрезаемый участок).
    pub fn add_trim(&mut self, start_ms: u64, end_ms: u64) {
        self.project.trim_segments.push(TrimSegment { start_ms, end_ms });
    }

    /// Удалить trim-сегмент по индексу.
    pub fn remove_trim(&mut self, index: usize) {
        if index < self.project.trim_segments.len() {
            self.project.trim_segments.remove(index);
        }
    }

    /// Добавить zoom-регион.
    pub fn add_zoom_region(&mut self, region: ZoomRegion) {
        self.project.zoom_regions.push(region);
    }

    /// Эффективная длительность после всех trim'ов.
    pub fn effective_duration_ms(&self) -> u64 {
        let trimmed: u64 = self
            .project
            .trim_segments
            .iter()
            .map(|t| t.end_ms.saturating_sub(t.start_ms))
            .sum();
        self.project.duration_ms.saturating_sub(trimmed)
    }

    /// Проверяет, попадает ли позиция в обрезанный сегмент.
    pub fn is_trimmed(&self, position_ms: u64) -> bool {
        self.project
            .trim_segments
            .iter()
            .any(|t| position_ms >= t.start_ms && position_ms < t.end_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_project() -> Project {
        Project::new("Test", PathBuf::from("/tmp/test.mp4"), 10000)
    }

    #[test]
    fn test_editor_state_new() {
        let state = EditorState::new(test_project());
        assert_eq!(state.playback.position_ms, 0);
        assert!(!state.playback.is_playing);
        assert_eq!(state.playback.playback_speed, 1.0);
        assert!(!state.playback.loop_enabled);
        assert_eq!(state.timeline.zoom_level, 1.0);
        assert_eq!(state.timeline.scroll_offset, 0.0);
        assert!(state.timeline.selection.is_none());
        assert_eq!(state.selected_tool, EditorTool::Select);
        assert_eq!(state.project.duration_ms, 10000);
    }

    #[test]
    fn test_seek() {
        let mut state = EditorState::new(test_project());
        state.seek(5000);
        assert_eq!(state.playback.position_ms, 5000);

        // Clamp к duration
        state.seek(99999);
        assert_eq!(state.playback.position_ms, 10000);

        state.seek(0);
        assert_eq!(state.playback.position_ms, 0);
    }

    #[test]
    fn test_toggle_playback() {
        let mut state = EditorState::new(test_project());
        assert!(!state.playback.is_playing);
        state.toggle_playback();
        assert!(state.playback.is_playing);
        state.toggle_playback();
        assert!(!state.playback.is_playing);
    }

    #[test]
    fn test_set_speed() {
        let mut state = EditorState::new(test_project());

        state.set_speed(2.0);
        assert_eq!(state.playback.playback_speed, 2.0);

        // Минимум
        state.set_speed(0.1);
        assert_eq!(state.playback.playback_speed, 0.25);

        // Максимум
        state.set_speed(10.0);
        assert_eq!(state.playback.playback_speed, 4.0);
    }

    #[test]
    fn test_add_trim() {
        let mut state = EditorState::new(test_project());
        assert!(state.project.trim_segments.is_empty());

        state.add_trim(1000, 2000);
        assert_eq!(state.project.trim_segments.len(), 1);
        assert_eq!(state.project.trim_segments[0].start_ms, 1000);
        assert_eq!(state.project.trim_segments[0].end_ms, 2000);

        state.add_trim(5000, 6000);
        assert_eq!(state.project.trim_segments.len(), 2);
    }

    #[test]
    fn test_remove_trim() {
        let mut state = EditorState::new(test_project());
        state.add_trim(0, 500);
        state.add_trim(9000, 10000);
        assert_eq!(state.project.trim_segments.len(), 2);

        state.remove_trim(0);
        assert_eq!(state.project.trim_segments.len(), 1);
        assert_eq!(state.project.trim_segments[0].start_ms, 9000);

        // Удаление за пределами — ничего не делает
        state.remove_trim(99);
        assert_eq!(state.project.trim_segments.len(), 1);
    }

    #[test]
    fn test_effective_duration() {
        let mut state = EditorState::new(test_project());
        assert_eq!(state.effective_duration_ms(), 10000);

        state.add_trim(0, 1000); // -1000
        assert_eq!(state.effective_duration_ms(), 9000);

        state.add_trim(5000, 7000); // -2000
        assert_eq!(state.effective_duration_ms(), 7000);
    }

    #[test]
    fn test_is_trimmed() {
        let mut state = EditorState::new(test_project());
        state.add_trim(1000, 3000);

        assert!(!state.is_trimmed(999));
        assert!(state.is_trimmed(1000));
        assert!(state.is_trimmed(2000));
        assert!(!state.is_trimmed(3000)); // end_ms exclusive
        assert!(!state.is_trimmed(5000));
    }

    #[test]
    fn test_add_zoom_region() {
        let mut state = EditorState::new(test_project());
        assert!(state.project.zoom_regions.is_empty());

        state.add_zoom_region(ZoomRegion {
            start_ms: 1000,
            end_ms: 3000,
            level: 2.0,
            focus_x: 0.5,
            focus_y: 0.5,
        });

        assert_eq!(state.project.zoom_regions.len(), 1);
        assert_eq!(state.project.zoom_regions[0].level, 2.0);
    }

    #[test]
    fn test_time_selection() {
        let mut state = EditorState::new(test_project());
        assert!(state.timeline.selection.is_none());

        state.timeline.selection = Some(TimeSelection {
            start_ms: 2000,
            end_ms: 5000,
        });

        let sel = state.timeline.selection.unwrap();
        assert_eq!(sel.start_ms, 2000);
        assert_eq!(sel.end_ms, 5000);
    }

    #[test]
    fn test_editor_tool_variants() {
        assert_ne!(EditorTool::Select, EditorTool::Trim);
        assert_ne!(EditorTool::Zoom, EditorTool::Annotate);
        assert_eq!(EditorTool::Select, EditorTool::Select);
    }
}
