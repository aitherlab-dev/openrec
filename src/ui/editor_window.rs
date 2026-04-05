use iced::widget::{button, canvas, column, container, row, text};
use iced::{Alignment, Element, Length};

use crate::editor::preview::PreviewFrame;
use crate::editor::state::{EditorState, EditorTool};
use crate::editor::timeline::TimelineWidget;
use crate::ui::recorder_hud::format_duration;

/// Сообщения окна редактора.
#[derive(Debug, Clone)]
pub enum EditorMessage {
    SelectTool(EditorTool),
    TogglePlayback,
    SetSpeed(f32),
    TimelineSeek(u64),
    Export,
    Back,
}

/// Пресеты скорости воспроизведения.
pub const SPEED_PRESETS: [f32; 3] = [0.5, 1.0, 2.0];

/// Строит UI окна редактора.
pub fn editor_view<'a>(
    state: &'a EditorState,
    _preview_frame: &Option<PreviewFrame>,
) -> Element<'a, EditorMessage> {
    // --- Toolbar ---
    let tool_btn = |label: &'a str, tool: EditorTool, selected: EditorTool| -> Element<'a, EditorMessage> {
        let b = button(text(label).size(14));
        if tool == selected {
            // Активный инструмент — кнопка без on_press (выглядит нажатой)
            b.into()
        } else {
            b.on_press(EditorMessage::SelectTool(tool)).into()
        }
    };

    let tools = row![
        tool_btn("Select", EditorTool::Select, state.selected_tool),
        tool_btn("Trim", EditorTool::Trim, state.selected_tool),
        tool_btn("Zoom", EditorTool::Zoom, state.selected_tool),
        tool_btn("Annotate", EditorTool::Annotate, state.selected_tool),
    ]
    .spacing(4);

    let play_label = if state.playback.is_playing {
        "Пауза"
    } else {
        "Воспр."
    };
    let play_btn = button(text(play_label).size(14)).on_press(EditorMessage::TogglePlayback);

    let speed_selector = {
        let btns: Vec<Element<'a, EditorMessage>> = SPEED_PRESETS
            .iter()
            .map(|&speed| {
                let label = format!("{speed}x");
                let b = button(text(label).size(12));
                if (state.playback.playback_speed - speed).abs() < 0.01 {
                    b.into()
                } else {
                    b.on_press(EditorMessage::SetSpeed(speed)).into()
                }
            })
            .collect();
        row(btns).spacing(2)
    };

    let export_btn = button(text("Экспорт").size(14)).on_press(EditorMessage::Export);
    let back_btn = button(text("Назад").size(14)).on_press(EditorMessage::Back);

    let toolbar = row![tools, play_btn, speed_selector, export_btn, back_btn]
        .spacing(12)
        .align_y(Alignment::Center)
        .padding(8);

    // --- Preview area ---
    let position_text = format_duration(std::time::Duration::from_millis(
        state.playback.position_ms,
    ));
    let preview_placeholder = container(
        text(format!("Preview — {position_text}"))
            .size(18)
            .color(iced::Color::from_rgb(0.6, 0.6, 0.6)),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center(Length::Fill)
    .style(|_theme: &iced::Theme| container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgb(
            0.12, 0.12, 0.14,
        ))),
        ..Default::default()
    });

    // --- Timeline ---
    let timeline_widget = TimelineWidget {
        duration_ms: state.project.duration_ms,
        position_ms: state.playback.position_ms,
        trim_segments: state
            .project
            .trim_segments
            .iter()
            .map(|t| crate::editor::timeline::TrimSegment {
                start_ms: t.start_ms,
                end_ms: t.end_ms,
            })
            .collect(),
        zoom_regions: state
            .project
            .zoom_regions
            .iter()
            .map(|z| crate::editor::timeline::ZoomRegion {
                start_ms: z.start_ms,
                end_ms: z.end_ms,
            })
            .collect(),
        height: 80.0,
    };

    use crate::editor::timeline::TimelineMessage;
    let timeline_el: Element<'a, TimelineMessage> = canvas(timeline_widget)
        .width(Length::Fill)
        .height(Length::Fixed(80.0))
        .into();

    let timeline_canvas = timeline_el.map(|msg| match msg {
        TimelineMessage::Seek(ms) | TimelineMessage::DragMove(ms) => {
            EditorMessage::TimelineSeek(ms)
        }
        TimelineMessage::DragStart | TimelineMessage::DragEnd => {
            EditorMessage::TimelineSeek(0)
        }
    });

    // --- Compose ---
    let layout = column![toolbar, preview_placeholder, timeline_canvas].spacing(0);

    container(layout)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_editor_message_variants() {
        let msgs: Vec<EditorMessage> = vec![
            EditorMessage::SelectTool(EditorTool::Select),
            EditorMessage::SelectTool(EditorTool::Trim),
            EditorMessage::SelectTool(EditorTool::Zoom),
            EditorMessage::SelectTool(EditorTool::Annotate),
            EditorMessage::TogglePlayback,
            EditorMessage::SetSpeed(1.0),
            EditorMessage::TimelineSeek(5000),
            EditorMessage::Export,
            EditorMessage::Back,
        ];
        // All variants constructable, Debug works
        for msg in &msgs {
            let _ = format!("{msg:?}");
        }
        assert_eq!(msgs.len(), 9);
    }

    #[test]
    fn test_speed_presets() {
        assert_eq!(SPEED_PRESETS.len(), 3);
        assert!((SPEED_PRESETS[0] - 0.5).abs() < f32::EPSILON);
        assert!((SPEED_PRESETS[1] - 1.0).abs() < f32::EPSILON);
        assert!((SPEED_PRESETS[2] - 2.0).abs() < f32::EPSILON);
    }
}
