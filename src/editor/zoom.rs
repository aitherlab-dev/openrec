use crate::capture::cursor::CursorPosition;
use crate::project::persistence::ZoomRegion;

/// Длительность перехода в/из зума (мс).
const TRANSITION_MS: u64 = 300;

/// Количество точек для сглаживания позиции курсора.
const SMOOTHING_WINDOW: usize = 5;

/// Трансформация зума для применения к кадру.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomTransform {
    pub scale: f32,
    pub translate_x: f32,
    pub translate_y: f32,
}

impl ZoomTransform {
    pub fn identity() -> Self {
        Self {
            scale: 1.0,
            translate_x: 0.0,
            translate_y: 0.0,
        }
    }
}

/// Движок зум-регионов и авто-следования.
pub struct ZoomEngine {
    pub regions: Vec<ZoomRegion>,
    pub cursor_data: Vec<CursorPosition>,
}

impl ZoomEngine {
    pub fn new(regions: Vec<ZoomRegion>, cursor_data: Vec<CursorPosition>) -> Self {
        Self {
            regions,
            cursor_data,
        }
    }

    /// Вычисляет трансформацию зума для позиции на таймлайне.
    /// Учитывает transition zone (300ms) для плавного входа/выхода.
    pub fn compute_transform(
        &self,
        position_ms: u64,
        frame_width: u32,
        frame_height: u32,
    ) -> ZoomTransform {
        for region in &self.regions {
            let trans = compute_region_transform(region, position_ms, frame_width, frame_height);
            if trans.scale > 1.0 {
                return trans;
            }
        }
        ZoomTransform::identity()
    }

    /// Авто-зум по данным курсора: adaptive smoothing + плавное следование.
    pub fn compute_auto_follow(
        &self,
        position_ms: u64,
        frame_width: u32,
        frame_height: u32,
    ) -> ZoomTransform {
        let pos = match smoothed_cursor_position(&self.cursor_data, position_ms) {
            Some(p) => p,
            None => return ZoomTransform::identity(),
        };

        let fw = frame_width as f32;
        let fh = frame_height as f32;

        // Нормализуем позицию курсора в [0, 1]
        let norm_x = (pos.0 as f32 / fw).clamp(0.0, 1.0);
        let norm_y = (pos.1 as f32 / fh).clamp(0.0, 1.0);

        // Отклонение от центра
        let dx = norm_x - 0.5;
        let dy = norm_y - 0.5;
        let distance = (dx * dx + dy * dy).sqrt();

        // Если курсор близко к центру — не следуем
        if distance < 0.1 {
            return ZoomTransform::identity();
        }

        // Масштаб следования пропорционален отклонению
        let follow_strength = ((distance - 0.1) / 0.4).clamp(0.0, 1.0);
        let scale = 1.0 + follow_strength * 0.5; // до 1.5x

        let translate_x = -dx * fw * follow_strength * 0.5;
        let translate_y = -dy * fh * follow_strength * 0.5;

        ZoomTransform {
            scale,
            translate_x,
            translate_y,
        }
    }
}

fn compute_region_transform(
    region: &ZoomRegion,
    position_ms: u64,
    frame_width: u32,
    frame_height: u32,
) -> ZoomTransform {
    let transition_start = region.start_ms.saturating_sub(TRANSITION_MS);
    let transition_end = region.end_ms + TRANSITION_MS;

    // Вычисляем progress (0..1) для easing
    let progress = if position_ms < transition_start || position_ms > transition_end {
        0.0
    } else if position_ms < region.start_ms {
        // Входим в зум
        let t = (position_ms - transition_start) as f32 / TRANSITION_MS as f32;
        ease_in_out_cubic(t)
    } else if position_ms <= region.end_ms {
        // Внутри региона
        1.0
    } else {
        // Выходим из зума
        let t = (position_ms - region.end_ms) as f32 / TRANSITION_MS as f32;
        1.0 - ease_in_out_cubic(t)
    };

    if progress <= 0.0 {
        return ZoomTransform::identity();
    }

    let scale = 1.0 + (region.level - 1.0) * progress;
    let fw = frame_width as f32;
    let fh = frame_height as f32;

    // Translate так чтобы focus point оставался в центре
    let translate_x = -(region.focus_x - 0.5) * fw * (scale - 1.0);
    let translate_y = -(region.focus_y - 0.5) * fh * (scale - 1.0);

    ZoomTransform {
        scale,
        translate_x,
        translate_y,
    }
}

/// Cubic ease-in-out: плавное ускорение и замедление.
pub fn ease_in_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// Линейная интерполяция позиции курсора по timestamp.
pub fn interpolate_cursor_position(
    cursor_data: &[CursorPosition],
    timestamp_ms: u64,
) -> Option<(f64, f64)> {
    if cursor_data.is_empty() {
        return None;
    }

    // Точное совпадение или до первой точки
    if timestamp_ms <= cursor_data[0].timestamp_ms {
        return Some((cursor_data[0].x, cursor_data[0].y));
    }

    let last = &cursor_data[cursor_data.len() - 1];
    if timestamp_ms >= last.timestamp_ms {
        return Some((last.x, last.y));
    }

    // Бинарный поиск по timestamp
    let idx = cursor_data
        .partition_point(|p| p.timestamp_ms <= timestamp_ms);

    if idx == 0 {
        return Some((cursor_data[0].x, cursor_data[0].y));
    }

    let before = &cursor_data[idx - 1];
    let after = &cursor_data[idx];

    let dt = (after.timestamp_ms - before.timestamp_ms) as f64;
    if dt == 0.0 {
        return Some((before.x, before.y));
    }

    let t = (timestamp_ms - before.timestamp_ms) as f64 / dt;
    let x = before.x + (after.x - before.x) * t;
    let y = before.y + (after.y - before.y) * t;
    Some((x, y))
}

/// Сглаженная позиция курсора (скользящее среднее по SMOOTHING_WINDOW точкам).
fn smoothed_cursor_position(
    cursor_data: &[CursorPosition],
    timestamp_ms: u64,
) -> Option<(f64, f64)> {
    if cursor_data.is_empty() {
        return None;
    }

    let idx = cursor_data
        .partition_point(|p| p.timestamp_ms <= timestamp_ms);

    let start = idx.saturating_sub(SMOOTHING_WINDOW);
    let end = idx.min(cursor_data.len());

    if start >= end {
        return interpolate_cursor_position(cursor_data, timestamp_ms);
    }

    let window = &cursor_data[start..end];
    let count = window.len() as f64;
    let avg_x = window.iter().map(|p| p.x).sum::<f64>() / count;
    let avg_y = window.iter().map(|p| p.y).sum::<f64>() / count;

    Some((avg_x, avg_y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zoom_transform_identity() {
        let t = ZoomTransform::identity();
        assert_eq!(t.scale, 1.0);
        assert_eq!(t.translate_x, 0.0);
        assert_eq!(t.translate_y, 0.0);
    }

    #[test]
    fn test_zoom_transform_scale() {
        let t = ZoomTransform {
            scale: 2.0,
            translate_x: -100.0,
            translate_y: -50.0,
        };
        assert_eq!(t.scale, 2.0);
        assert_eq!(t.translate_x, -100.0);
        assert_eq!(t.translate_y, -50.0);
    }

    #[test]
    fn test_ease_in_out_cubic() {
        let eps = 1e-6;
        assert!((ease_in_out_cubic(0.0)).abs() < eps);
        assert!((ease_in_out_cubic(0.5) - 0.5).abs() < eps);
        assert!((ease_in_out_cubic(1.0) - 1.0).abs() < eps);

        // Монотонно возрастает
        assert!(ease_in_out_cubic(0.25) < ease_in_out_cubic(0.5));
        assert!(ease_in_out_cubic(0.5) < ease_in_out_cubic(0.75));

        // Clamp
        assert!((ease_in_out_cubic(-1.0)).abs() < eps);
        assert!((ease_in_out_cubic(2.0) - 1.0).abs() < eps);
    }

    #[test]
    fn test_compute_transform_no_regions() {
        let engine = ZoomEngine::new(vec![], vec![]);
        let t = engine.compute_transform(500, 1920, 1080);
        assert_eq!(t, ZoomTransform::identity());
    }

    #[test]
    fn test_compute_transform_inside_region() {
        let region = ZoomRegion {
            start_ms: 1000,
            end_ms: 3000,
            level: 2.0,
            focus_x: 0.5,
            focus_y: 0.5,
        };
        let engine = ZoomEngine::new(vec![region], vec![]);
        let t = engine.compute_transform(2000, 1920, 1080);

        // Внутри региона: scale = level = 2.0
        assert_eq!(t.scale, 2.0);
        // focus at center → translate = 0
        assert_eq!(t.translate_x, 0.0);
        assert_eq!(t.translate_y, 0.0);
    }

    #[test]
    fn test_compute_transform_inside_region_off_center() {
        let region = ZoomRegion {
            start_ms: 1000,
            end_ms: 3000,
            level: 2.0,
            focus_x: 0.75,
            focus_y: 0.25,
        };
        let engine = ZoomEngine::new(vec![region], vec![]);
        let t = engine.compute_transform(2000, 1920, 1080);

        assert_eq!(t.scale, 2.0);
        // focus_x=0.75: translate_x = -(0.75-0.5)*1920*(2-1) = -480
        assert!((t.translate_x - (-480.0)).abs() < 0.1);
        // focus_y=0.25: translate_y = -(0.25-0.5)*1080*(2-1) = 270
        assert!((t.translate_y - 270.0).abs() < 0.1);
    }

    #[test]
    fn test_compute_transform_transition() {
        let region = ZoomRegion {
            start_ms: 1000,
            end_ms: 3000,
            level: 2.0,
            focus_x: 0.5,
            focus_y: 0.5,
        };
        let engine = ZoomEngine::new(vec![region], vec![]);

        // 150ms до региона — в середине transition (300ms)
        let t = engine.compute_transform(850, 1920, 1080);
        assert!(t.scale > 1.0, "should be zooming in");
        assert!(t.scale < 2.0, "should not be fully zoomed");

        // Далеко до региона — identity
        let t_before = engine.compute_transform(0, 1920, 1080);
        assert_eq!(t_before, ZoomTransform::identity());

        // Далеко после региона — identity
        let t_after = engine.compute_transform(5000, 1920, 1080);
        assert_eq!(t_after, ZoomTransform::identity());
    }

    #[test]
    fn test_interpolate_cursor_position_exact() {
        let data = vec![
            CursorPosition { x: 100.0, y: 200.0, timestamp_ms: 0 },
            CursorPosition { x: 300.0, y: 400.0, timestamp_ms: 100 },
            CursorPosition { x: 500.0, y: 600.0, timestamp_ms: 200 },
        ];

        let (x, y) = interpolate_cursor_position(&data, 100).unwrap();
        assert_eq!(x, 300.0);
        assert_eq!(y, 400.0);
    }

    #[test]
    fn test_interpolate_cursor_position() {
        let data = vec![
            CursorPosition { x: 100.0, y: 200.0, timestamp_ms: 0 },
            CursorPosition { x: 300.0, y: 400.0, timestamp_ms: 100 },
        ];

        // В середине
        let (x, y) = interpolate_cursor_position(&data, 50).unwrap();
        assert!((x - 200.0).abs() < 0.01);
        assert!((y - 300.0).abs() < 0.01);

        // На 25%
        let (x, y) = interpolate_cursor_position(&data, 25).unwrap();
        assert!((x - 150.0).abs() < 0.01);
        assert!((y - 250.0).abs() < 0.01);
    }

    #[test]
    fn test_interpolate_cursor_position_empty() {
        assert!(interpolate_cursor_position(&[], 50).is_none());
    }

    #[test]
    fn test_interpolate_cursor_position_clamp() {
        let data = vec![
            CursorPosition { x: 100.0, y: 200.0, timestamp_ms: 50 },
            CursorPosition { x: 300.0, y: 400.0, timestamp_ms: 150 },
        ];

        // До первой точки
        let (x, y) = interpolate_cursor_position(&data, 0).unwrap();
        assert_eq!(x, 100.0);
        assert_eq!(y, 200.0);

        // После последней
        let (x, y) = interpolate_cursor_position(&data, 999).unwrap();
        assert_eq!(x, 300.0);
        assert_eq!(y, 400.0);
    }

    #[test]
    fn test_auto_follow_no_data() {
        let engine = ZoomEngine::new(vec![], vec![]);
        let t = engine.compute_auto_follow(500, 1920, 1080);
        assert_eq!(t, ZoomTransform::identity());
    }

    #[test]
    fn test_auto_follow_cursor_center() {
        // Курсор в центре — identity
        let data = vec![
            CursorPosition { x: 960.0, y: 540.0, timestamp_ms: 0 },
            CursorPosition { x: 960.0, y: 540.0, timestamp_ms: 100 },
        ];
        let engine = ZoomEngine::new(vec![], data);
        let t = engine.compute_auto_follow(50, 1920, 1080);
        assert_eq!(t.scale, 1.0);
    }
}
