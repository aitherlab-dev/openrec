use iced::widget::{button, column, container, text};
use iced::{Element, Length, Task};

#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
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
    SwitchMode(AppMode),
    StartRecording,
    StopRecording,
    OpenEditor,
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
            Message::SwitchMode(mode) => {
                self.mode = mode;
                Task::none()
            }
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
            Message::Quit => iced::exit(),
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let content = match self.mode {
            AppMode::Idle => column![
                text("OpenRec — Готов к записи").size(24),
                button("Начать запись").on_press(Message::StartRecording),
            ],
            AppMode::Recording => column![
                text("Идёт запись...").size(24),
                button("Остановить").on_press(Message::StopRecording),
            ],
            AppMode::Editor => column![
                text("Редактор (в разработке)").size(24),
                button("Назад").on_press(Message::SwitchMode(AppMode::Idle)),
            ],
        };

        container(content.spacing(20).align_x(iced::Alignment::Center))
            .center(Length::Fill)
            .into()
    }
}
