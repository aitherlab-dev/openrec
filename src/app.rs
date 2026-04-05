use iced::widget::{button, column, container, text};
use iced::{Element, Length, Task};

enum AppMode {
    Idle,
    Recording,
    Editor,
}

pub struct App {
    mode: AppMode,
    recording_duration: Option<std::time::Duration>,
}

#[derive(Debug, Clone)]
pub enum Message {
    StartRecording,
    StopRecording,
    OpenEditor,
    BackToIdle,
    Quit,
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let app = Self {
            mode: AppMode::Idle,
            recording_duration: None,
        };
        (app, Task::none())
    }

    pub fn title(&self) -> String {
        let suffix = match self.mode {
            AppMode::Idle => "",
            AppMode::Recording => " — Запись",
            AppMode::Editor => " — Редактор",
        };
        format!("OpenRec{suffix}")
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::StartRecording => {
                self.mode = AppMode::Recording;
                self.recording_duration = Some(std::time::Duration::ZERO);
                Task::none()
            }
            Message::StopRecording => {
                self.mode = AppMode::Idle;
                self.recording_duration = None;
                Task::none()
            }
            Message::OpenEditor => {
                self.mode = AppMode::Editor;
                Task::none()
            }
            Message::BackToIdle => {
                self.mode = AppMode::Idle;
                Task::none()
            }
            Message::Quit => iced::exit(),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let content = match self.mode {
            AppMode::Idle => column![
                text("OpenRec — Готов к записи").size(24),
                button("Начать запись").on_press(Message::StartRecording),
                button("Открыть редактор").on_press(Message::OpenEditor),
            ],
            AppMode::Recording => column![
                text("Идёт запись...").size(24),
                button("Остановить").on_press(Message::StopRecording),
            ],
            AppMode::Editor => column![
                text("Редактор (в разработке)").size(24),
                button("Назад").on_press(Message::BackToIdle),
                button("Выход").on_press(Message::Quit),
            ],
        };

        container(content.spacing(20).align_x(iced::Alignment::Center))
            .center(Length::Fill)
            .into()
    }
}
