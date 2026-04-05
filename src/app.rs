use iced::widget::text;
use iced::{Element, Task};

pub struct App;

#[derive(Debug, Clone)]
pub enum Message {}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        (Self, Task::none())
    }

    pub fn update(&mut self, _message: Message) -> Task<Message> {
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        text("OpenRec").size(24).into()
    }
}
