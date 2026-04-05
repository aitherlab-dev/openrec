use std::time::Duration;

use iced::widget::{button, checkbox, column, container, row, text};
use iced::{Alignment, Color, Element, Length};

use crate::app::Message;

/// Форматирует Duration в "MM:SS" (или "H:MM:SS" для >= 1 часа).
pub fn format_duration(d: Duration) -> String {
    let total_secs = d.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

/// Экран ожидания — idle HUD.
pub fn recorder_idle_view<'a>() -> Element<'a, Message> {
    let title = text("OpenRec").size(32);

    let record_btn = button(
        text("Начать запись").size(18).center(),
    )
    .padding([12, 32])
    .on_press(Message::StartRecording);

    let editor_btn = button("Открыть редактор")
        .on_press(Message::OpenEditor);

    let settings_btn = button("Настройки");

    let mic_label = text("Микрофон: по умолчанию").size(14);

    let system_audio_cb = checkbox(true).label("Системный звук");

    let webcam_cb = checkbox(false).label("Веб-камера");

    let controls = column![
        title,
        record_btn,
        editor_btn,
        settings_btn,
        mic_label,
        system_audio_cb,
        webcam_cb,
    ]
    .spacing(16)
    .align_x(Alignment::Center);

    container(controls)
        .center(Length::Fill)
        .into()
}

/// Экран записи — recording HUD.
pub fn recorder_recording_view(duration: Duration) -> Element<'static, Message> {
    let rec_indicator = text("● REC")
        .size(20)
        .color(Color::from_rgb(0.9, 0.1, 0.1));

    let timer = text(format_duration(duration)).size(28);

    let top_row = row![rec_indicator, timer]
        .spacing(12)
        .align_y(Alignment::Center);

    let pause_btn = button("Пауза");

    let stop_btn = button(
        text("Остановить").size(16).center(),
    )
    .padding([10, 28])
    .on_press(Message::StopRecording);

    let button_row = row![pause_btn, stop_btn]
        .spacing(12)
        .align_y(Alignment::Center);

    let controls = column![top_row, button_row]
        .spacing(20)
        .align_x(Alignment::Center);

    container(controls)
        .center(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_zero() {
        assert_eq!(format_duration(Duration::ZERO), "00:00");
    }

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration(Duration::from_secs(45)), "00:45");
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(Duration::from_secs(125)), "02:05");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(Duration::from_secs(3661)), "1:01:01");
    }
}
