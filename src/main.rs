mod app;
mod camera;
mod capture;
mod config;
mod editor;
mod export;
mod project;
#[allow(dead_code)]
mod tray;
mod ui;

use app::App;

fn main() -> iced::Result {
    env_logger::init();

    iced::application(App::boot, App::update, App::view)
        .title("OpenRec")
        .centered()
        .run()
}
