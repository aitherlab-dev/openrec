use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};

#[derive(Debug, Clone)]
pub enum TimelineMessage {
    Seek(u64),
    DragStart,
    DragMove(u64),
    DragEnd,
}

#[derive(Debug, Clone)]
pub struct TrimSegment {
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ZoomRegion {
    pub start_ms: u64,
    pub end_ms: u64,
}

pub struct TimelineWidget {
    pub duration_ms: u64,
    pub position_ms: u64,
    pub trim_segments: Vec<TrimSegment>,
    pub zoom_regions: Vec<ZoomRegion>,
    pub height: f32,
}

#[derive(Debug, Default)]
pub struct TimelineState {
    dragging: bool,
}

pub fn ms_to_x(ms: u64, duration_ms: u64, width: f32) -> f32 {
    if duration_ms == 0 {
        return 0.0;
    }
    (ms as f64 / duration_ms as f64 * width as f64) as f32
}

pub fn x_to_ms(x: f32, duration_ms: u64, width: f32) -> u64 {
    if width <= 0.0 || duration_ms == 0 {
        return 0;
    }
    let ratio = (x / width).clamp(0.0, 1.0);
    (ratio as f64 * duration_ms as f64) as u64
}

pub fn format_time_marker(ms: u64) -> String {
    let total_secs = ms / 1000;
    let minutes = total_secs / 60;
    let seconds = total_secs % 60;
    format!("{minutes}:{seconds:02}")
}

/// Compute interval between time markers based on duration.
fn marker_interval_ms(duration_ms: u64) -> u64 {
    if duration_ms <= 10_000 {
        1_000 // every second for <=10s
    } else if duration_ms <= 60_000 {
        5_000 // every 5s for <=1min
    } else if duration_ms <= 300_000 {
        15_000 // every 15s for <=5min
    } else {
        60_000 // every minute
    }
}

const BG_COLOR: Color = Color::from_rgb(0.15, 0.15, 0.17);
const MARKER_COLOR: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.4);
const PLAYHEAD_COLOR: Color = Color::from_rgb(0.9, 0.15, 0.15);
const TRIM_COLOR: Color = Color::from_rgba(0.9, 0.15, 0.15, 0.25);
const ZOOM_COLOR: Color = Color::from_rgba(0.2, 0.4, 0.9, 0.25);
const TEXT_COLOR: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.6);

impl canvas::Program<TimelineMessage> for TimelineWidget {
    type State = TimelineState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<TimelineMessage>> {
        let canvas::Event::Mouse(mouse_event) = event else {
            return None;
        };

        match mouse_event {
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    state.dragging = true;
                    let ms = x_to_ms(pos.x, self.duration_ms, bounds.width);
                    return Some(
                        canvas::Action::publish(TimelineMessage::Seek(ms)).and_capture(),
                    );
                }
            }
            mouse::Event::CursorMoved { .. } => {
                if state.dragging {
                    if let Some(pos) = cursor.position_in(bounds) {
                        let ms = x_to_ms(pos.x, self.duration_ms, bounds.width);
                        return Some(canvas::Action::publish(TimelineMessage::DragMove(ms)));
                    }
                }
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                if state.dragging {
                    state.dragging = false;
                    return Some(canvas::Action::publish(TimelineMessage::DragEnd));
                }
            }
            _ => {}
        }

        None
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;

        // Background
        frame.fill_rectangle(Point::ORIGIN, Size::new(w, h), BG_COLOR);

        // Trim segments
        for seg in &self.trim_segments {
            let x1 = ms_to_x(seg.start_ms, self.duration_ms, w);
            let x2 = ms_to_x(seg.end_ms, self.duration_ms, w);
            frame.fill_rectangle(
                Point::new(x1, 0.0),
                Size::new(x2 - x1, h),
                TRIM_COLOR,
            );
        }

        // Zoom regions
        for region in &self.zoom_regions {
            let x1 = ms_to_x(region.start_ms, self.duration_ms, w);
            let x2 = ms_to_x(region.end_ms, self.duration_ms, w);
            frame.fill_rectangle(
                Point::new(x1, 0.0),
                Size::new(x2 - x1, h),
                ZOOM_COLOR,
            );
        }

        // Time markers
        if self.duration_ms > 0 {
            let interval = marker_interval_ms(self.duration_ms);
            let mut t = interval;
            while t < self.duration_ms {
                let x = ms_to_x(t, self.duration_ms, w);

                // Tick line
                frame.fill_rectangle(
                    Point::new(x, 0.0),
                    Size::new(1.0, h * 0.3),
                    MARKER_COLOR,
                );

                // Label
                frame.fill_text(canvas::Text {
                    content: format_time_marker(t),
                    position: Point::new(x + 3.0, 2.0),
                    color: TEXT_COLOR,
                    size: 11.0.into(),
                    ..Default::default()
                });

                t += interval;
            }
        }

        // Playhead
        let playhead_x = ms_to_x(self.position_ms, self.duration_ms, w);
        frame.fill_rectangle(
            Point::new(playhead_x - 1.0, 0.0),
            Size::new(2.0, h),
            PLAYHEAD_COLOR,
        );

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ms_to_x() {
        // Midpoint
        assert!((ms_to_x(5000, 10000, 800.0) - 400.0).abs() < 0.01);
        // Start
        assert!((ms_to_x(0, 10000, 800.0) - 0.0).abs() < 0.01);
        // End
        assert!((ms_to_x(10000, 10000, 800.0) - 800.0).abs() < 0.01);
        // Quarter
        assert!((ms_to_x(2500, 10000, 1000.0) - 250.0).abs() < 0.01);
    }

    #[test]
    fn test_x_to_ms() {
        assert_eq!(x_to_ms(400.0, 10000, 800.0), 5000);
        assert_eq!(x_to_ms(0.0, 10000, 800.0), 0);
        assert_eq!(x_to_ms(800.0, 10000, 800.0), 10000);
        // Clamp: beyond right edge
        assert_eq!(x_to_ms(1000.0, 10000, 800.0), 10000);
        // Clamp: negative
        assert_eq!(x_to_ms(-50.0, 10000, 800.0), 0);
    }

    #[test]
    fn test_format_time_marker() {
        assert_eq!(format_time_marker(0), "0:00");
        assert_eq!(format_time_marker(1000), "0:01");
        assert_eq!(format_time_marker(5000), "0:05");
        assert_eq!(format_time_marker(60_000), "1:00");
        assert_eq!(format_time_marker(90_000), "1:30");
        assert_eq!(format_time_marker(600_000), "10:00");
        assert_eq!(format_time_marker(3_661_000), "61:01");
        // Sub-second precision is truncated
        assert_eq!(format_time_marker(1500), "0:01");
    }

    #[test]
    fn test_ms_to_x_zero_duration() {
        assert_eq!(ms_to_x(0, 0, 800.0), 0.0);
        assert_eq!(ms_to_x(5000, 0, 800.0), 0.0);
    }

    #[test]
    fn test_x_to_ms_zero_width() {
        assert_eq!(x_to_ms(100.0, 10000, 0.0), 0);
        assert_eq!(x_to_ms(100.0, 10000, -1.0), 0);
    }

    #[test]
    fn test_x_to_ms_zero_duration() {
        assert_eq!(x_to_ms(400.0, 0, 800.0), 0);
    }

    #[test]
    fn test_roundtrip_conversion() {
        let duration = 120_000u64;
        let width = 1200.0f32;
        for ms in [0, 1000, 30_000, 60_000, 119_999, 120_000] {
            let x = ms_to_x(ms, duration, width);
            let back = x_to_ms(x, duration, width);
            assert!(
                (back as i64 - ms as i64).unsigned_abs() <= 1,
                "roundtrip failed for {ms}ms: got {back}ms"
            );
        }
    }
}
