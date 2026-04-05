use crate::project::persistence::{Annotation, TrimSegment, ZoomRegion};

#[derive(Debug, Clone, PartialEq)]
pub enum EditorAction {
    AddTrim(TrimSegment),
    RemoveTrim {
        index: usize,
        segment: TrimSegment,
    },
    AddZoomRegion(ZoomRegion),
    RemoveZoomRegion {
        index: usize,
        region: ZoomRegion,
    },
    AddAnnotation(Annotation),
    RemoveAnnotation {
        index: usize,
        annotation: Annotation,
    },
    MoveAnnotation {
        index: usize,
        old_x: f32,
        old_y: f32,
        new_x: f32,
        new_y: f32,
    },
    SetSpeed {
        old_speed: f32,
        new_speed: f32,
    },
}

impl EditorAction {
    pub fn inverse(&self) -> Self {
        match self {
            Self::AddTrim(seg) => Self::RemoveTrim {
                index: 0,
                segment: seg.clone(),
            },
            Self::RemoveTrim { index, segment } => {
                let _ = index;
                Self::AddTrim(segment.clone())
            }
            Self::AddZoomRegion(region) => Self::RemoveZoomRegion {
                index: 0,
                region: region.clone(),
            },
            Self::RemoveZoomRegion { index, region } => {
                let _ = index;
                Self::AddZoomRegion(region.clone())
            }
            Self::AddAnnotation(ann) => Self::RemoveAnnotation {
                index: 0,
                annotation: ann.clone(),
            },
            Self::RemoveAnnotation { index, annotation } => {
                let _ = index;
                Self::AddAnnotation(annotation.clone())
            }
            Self::MoveAnnotation {
                index,
                old_x,
                old_y,
                new_x,
                new_y,
            } => Self::MoveAnnotation {
                index: *index,
                old_x: *new_x,
                old_y: *new_y,
                new_x: *old_x,
                new_y: *old_y,
            },
            Self::SetSpeed {
                old_speed,
                new_speed,
            } => Self::SetSpeed {
                old_speed: *new_speed,
                new_speed: *old_speed,
            },
        }
    }
}

pub struct History {
    undo_stack: Vec<EditorAction>,
    redo_stack: Vec<EditorAction>,
    max_history: usize,
}

impl History {
    pub fn new(max_history: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_history,
        }
    }

    pub fn push(&mut self, action: EditorAction) {
        self.redo_stack.clear();
        self.undo_stack.push(action);
        if self.undo_stack.len() > self.max_history {
            self.undo_stack.remove(0);
        }
    }

    pub fn undo(&mut self) -> Option<EditorAction> {
        let action = self.undo_stack.pop()?;
        let inverse = action.inverse();
        self.redo_stack.push(action.clone());
        Some(inverse)
    }

    pub fn redo(&mut self) -> Option<EditorAction> {
        let action = self.redo_stack.pop()?;
        self.undo_stack.push(action.clone());
        Some(action)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_trim() -> TrimSegment {
        TrimSegment {
            start_ms: 1000,
            end_ms: 5000,
        }
    }

    fn sample_zoom() -> ZoomRegion {
        ZoomRegion {
            start_ms: 2000,
            end_ms: 4000,
            level: 2.0,
            focus_x: 0.5,
            focus_y: 0.5,
        }
    }

    fn sample_annotation() -> Annotation {
        Annotation {
            kind: crate::project::persistence::AnnotationKind::Text,
            start_ms: 0,
            end_ms: 3000,
            x: 100.0,
            y: 200.0,
            content: "test".into(),
        }
    }

    #[test]
    fn test_history_new() {
        let h = History::new(50);
        assert!(!h.can_undo());
        assert!(!h.can_redo());
    }

    #[test]
    fn test_push_and_undo() {
        let mut h = History::default();
        h.push(EditorAction::AddTrim(sample_trim()));
        assert!(h.can_undo());

        let inverse = h.undo().unwrap();
        // Inverse of AddTrim is RemoveTrim
        match inverse {
            EditorAction::RemoveTrim { segment, .. } => {
                assert_eq!(segment, sample_trim());
            }
            _ => panic!("expected RemoveTrim"),
        }
        assert!(!h.can_undo());
    }

    #[test]
    fn test_undo_redo_cycle() {
        let mut h = History::default();
        let action = EditorAction::SetSpeed {
            old_speed: 1.0,
            new_speed: 2.0,
        };
        h.push(action.clone());

        let _inverse = h.undo().unwrap();
        assert!(h.can_redo());

        let redone = h.redo().unwrap();
        assert_eq!(redone, action);
        assert!(!h.can_redo());
        assert!(h.can_undo());
    }

    #[test]
    fn test_push_clears_redo() {
        let mut h = History::default();
        h.push(EditorAction::AddTrim(sample_trim()));
        h.undo();
        assert!(h.can_redo());

        h.push(EditorAction::AddZoomRegion(sample_zoom()));
        assert!(!h.can_redo());
    }

    #[test]
    fn test_undo_empty() {
        let mut h = History::default();
        assert!(h.undo().is_none());
    }

    #[test]
    fn test_redo_empty() {
        let mut h = History::default();
        assert!(h.redo().is_none());
    }

    #[test]
    fn test_max_history() {
        let mut h = History::new(100);
        for i in 0..101 {
            h.push(EditorAction::SetSpeed {
                old_speed: i as f32,
                new_speed: (i + 1) as f32,
            });
        }
        assert_eq!(h.undo_stack.len(), 100);
    }

    #[test]
    fn test_inverse_add_trim() {
        let action = EditorAction::AddTrim(sample_trim());
        let inv = action.inverse();
        match inv {
            EditorAction::RemoveTrim { segment, .. } => {
                assert_eq!(segment, sample_trim());
            }
            _ => panic!("expected RemoveTrim"),
        }

        // Double inverse of RemoveTrim → AddTrim
        let action2 = EditorAction::RemoveTrim {
            index: 3,
            segment: sample_trim(),
        };
        let inv2 = action2.inverse();
        match inv2 {
            EditorAction::AddTrim(seg) => assert_eq!(seg, sample_trim()),
            _ => panic!("expected AddTrim"),
        }
    }

    #[test]
    fn test_inverse_move_annotation() {
        let action = EditorAction::MoveAnnotation {
            index: 5,
            old_x: 10.0,
            old_y: 20.0,
            new_x: 30.0,
            new_y: 40.0,
        };
        let inv = action.inverse();
        match inv {
            EditorAction::MoveAnnotation {
                index,
                old_x,
                old_y,
                new_x,
                new_y,
            } => {
                assert_eq!(index, 5);
                assert_eq!(old_x, 30.0);
                assert_eq!(old_y, 40.0);
                assert_eq!(new_x, 10.0);
                assert_eq!(new_y, 20.0);
            }
            _ => panic!("expected MoveAnnotation"),
        }
    }

    #[test]
    fn test_inverse_set_speed() {
        let action = EditorAction::SetSpeed {
            old_speed: 1.0,
            new_speed: 0.5,
        };
        let inv = action.inverse();
        match inv {
            EditorAction::SetSpeed {
                old_speed,
                new_speed,
            } => {
                assert_eq!(old_speed, 0.5);
                assert_eq!(new_speed, 1.0);
            }
            _ => panic!("expected SetSpeed"),
        }
    }

    #[test]
    fn test_clear() {
        let mut h = History::default();
        h.push(EditorAction::AddTrim(sample_trim()));
        h.push(EditorAction::AddAnnotation(sample_annotation()));
        h.undo();

        assert!(h.can_undo());
        assert!(h.can_redo());

        h.clear();
        assert!(!h.can_undo());
        assert!(!h.can_redo());
    }
}
