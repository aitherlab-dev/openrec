use crate::project::persistence::{Annotation, AnnotationKind};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ArrowDirection {
    Up,
    Down,
    Left,
    Right,
    UpLeft,
    UpRight,
    DownLeft,
    DownRight,
}

impl ArrowDirection {
    pub fn to_vector(self) -> (f32, f32) {
        let d = std::f32::consts::FRAC_1_SQRT_2;
        match self {
            Self::Up => (0.0, -1.0),
            Self::Down => (0.0, 1.0),
            Self::Left => (-1.0, 0.0),
            Self::Right => (1.0, 0.0),
            Self::UpLeft => (-d, -d),
            Self::UpRight => (d, -d),
            Self::DownLeft => (-d, d),
            Self::DownRight => (d, d),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AnnotationStyle {
    pub font_size: f32,
    pub color: [f32; 4],
    pub stroke_color: [f32; 4],
    pub stroke_width: f32,
    pub arrow_head_size: f32,
}

impl Default for AnnotationStyle {
    fn default() -> Self {
        Self {
            font_size: 24.0,
            color: [1.0, 1.0, 1.0, 1.0],
            stroke_color: [0.0, 0.0, 0.0, 1.0],
            stroke_width: 2.0,
            arrow_head_size: 20.0,
        }
    }
}

pub struct AnnotationManager {
    pub annotations: Vec<Annotation>,
}

impl AnnotationManager {
    pub fn new(annotations: Vec<Annotation>) -> Self {
        Self { annotations }
    }

    pub fn visible_at(&self, position_ms: u64) -> Vec<&Annotation> {
        self.annotations
            .iter()
            .filter(|a| a.start_ms <= position_ms && position_ms < a.end_ms)
            .collect()
    }

    pub fn add_text(
        &mut self,
        x: f32,
        y: f32,
        content: String,
        start_ms: u64,
        end_ms: u64,
    ) {
        self.annotations.push(Annotation {
            kind: AnnotationKind::Text,
            start_ms,
            end_ms,
            x,
            y,
            content,
        });
    }

    pub fn add_arrow(
        &mut self,
        x: f32,
        y: f32,
        direction: ArrowDirection,
        start_ms: u64,
        end_ms: u64,
    ) {
        self.annotations.push(Annotation {
            kind: AnnotationKind::Arrow,
            start_ms,
            end_ms,
            x,
            y,
            content: format!("{direction:?}"),
        });
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.annotations.len() {
            self.annotations.remove(index);
        }
    }

    pub fn move_annotation(&mut self, index: usize, new_x: f32, new_y: f32) {
        if let Some(a) = self.annotations.get_mut(index) {
            a.x = new_x;
            a.y = new_y;
        }
    }

    pub fn resize_time(&mut self, index: usize, new_start: u64, new_end: u64) {
        if let Some(a) = self.annotations.get_mut(index) {
            a.start_ms = new_start;
            a.end_ms = new_end;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manager() -> AnnotationManager {
        AnnotationManager::new(vec![])
    }

    #[test]
    fn test_visible_at_empty() {
        let mgr = make_manager();
        assert!(mgr.visible_at(0).is_empty());
        assert!(mgr.visible_at(5000).is_empty());
    }

    #[test]
    fn test_visible_at_in_range() {
        let mgr = AnnotationManager::new(vec![Annotation {
            kind: AnnotationKind::Text,
            start_ms: 1000,
            end_ms: 5000,
            x: 10.0,
            y: 20.0,
            content: "hello".into(),
        }]);
        assert_eq!(mgr.visible_at(1000).len(), 1);
        assert_eq!(mgr.visible_at(3000).len(), 1);
        assert_eq!(mgr.visible_at(4999).len(), 1);
    }

    #[test]
    fn test_visible_at_out_of_range() {
        let mgr = AnnotationManager::new(vec![Annotation {
            kind: AnnotationKind::Text,
            start_ms: 1000,
            end_ms: 5000,
            x: 0.0,
            y: 0.0,
            content: String::new(),
        }]);
        // Before start
        assert!(mgr.visible_at(999).is_empty());
        // At end (exclusive)
        assert!(mgr.visible_at(5000).is_empty());
        // After end
        assert!(mgr.visible_at(6000).is_empty());
    }

    #[test]
    fn test_add_text_annotation() {
        let mut mgr = make_manager();
        mgr.add_text(100.0, 200.0, "Test".into(), 0, 3000);

        assert_eq!(mgr.annotations.len(), 1);
        let a = &mgr.annotations[0];
        assert_eq!(a.kind, AnnotationKind::Text);
        assert_eq!(a.x, 100.0);
        assert_eq!(a.y, 200.0);
        assert_eq!(a.content, "Test");
        assert_eq!(a.start_ms, 0);
        assert_eq!(a.end_ms, 3000);
    }

    #[test]
    fn test_add_arrow_annotation() {
        let mut mgr = make_manager();
        mgr.add_arrow(50.0, 60.0, ArrowDirection::UpRight, 1000, 2000);

        assert_eq!(mgr.annotations.len(), 1);
        let a = &mgr.annotations[0];
        assert_eq!(a.kind, AnnotationKind::Arrow);
        assert_eq!(a.x, 50.0);
        assert_eq!(a.y, 60.0);
        assert!(a.content.contains("UpRight"));
        assert_eq!(a.start_ms, 1000);
        assert_eq!(a.end_ms, 2000);
    }

    #[test]
    fn test_remove_annotation() {
        let mut mgr = make_manager();
        mgr.add_text(0.0, 0.0, "A".into(), 0, 1000);
        mgr.add_text(0.0, 0.0, "B".into(), 1000, 2000);
        mgr.add_text(0.0, 0.0, "C".into(), 2000, 3000);

        mgr.remove(1); // remove "B"
        assert_eq!(mgr.annotations.len(), 2);
        assert_eq!(mgr.annotations[0].content, "A");
        assert_eq!(mgr.annotations[1].content, "C");

        // Out of bounds — no panic
        mgr.remove(99);
        assert_eq!(mgr.annotations.len(), 2);
    }

    #[test]
    fn test_move_annotation() {
        let mut mgr = make_manager();
        mgr.add_text(10.0, 20.0, "move me".into(), 0, 1000);

        mgr.move_annotation(0, 300.0, 400.0);
        assert_eq!(mgr.annotations[0].x, 300.0);
        assert_eq!(mgr.annotations[0].y, 400.0);

        // Out of bounds — no panic
        mgr.move_annotation(99, 0.0, 0.0);
    }

    #[test]
    fn test_arrow_direction_vectors() {
        let directions = [
            ArrowDirection::Up,
            ArrowDirection::Down,
            ArrowDirection::Left,
            ArrowDirection::Right,
            ArrowDirection::UpLeft,
            ArrowDirection::UpRight,
            ArrowDirection::DownLeft,
            ArrowDirection::DownRight,
        ];

        for dir in &directions {
            let (vx, vy) = dir.to_vector();
            let len = (vx * vx + vy * vy).sqrt();
            assert!(
                (len - 1.0).abs() < 0.001,
                "{dir:?} vector length {len} != 1.0"
            );
        }

        // Spot-check specific directions
        let (x, y) = ArrowDirection::Up.to_vector();
        assert_eq!(x, 0.0);
        assert!(y < 0.0);

        let (x, y) = ArrowDirection::Right.to_vector();
        assert!(x > 0.0);
        assert_eq!(y, 0.0);
    }

    #[test]
    fn test_annotation_style_default() {
        let style = AnnotationStyle::default();
        assert_eq!(style.font_size, 24.0);
        assert_eq!(style.color, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(style.stroke_color, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(style.stroke_width, 2.0);
        assert_eq!(style.arrow_head_size, 20.0);
    }

    #[test]
    fn test_resize_time() {
        let mut mgr = make_manager();
        mgr.add_text(0.0, 0.0, "t".into(), 0, 1000);

        mgr.resize_time(0, 500, 3000);
        assert_eq!(mgr.annotations[0].start_ms, 500);
        assert_eq!(mgr.annotations[0].end_ms, 3000);
    }
}
