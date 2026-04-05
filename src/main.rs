mod app;
mod camera;
#[allow(dead_code)]
mod capture;
#[allow(dead_code)]
mod config;
#[allow(dead_code)]
mod editor;
#[allow(dead_code)]
mod export;
#[allow(dead_code)]
mod project;
#[allow(dead_code)]
mod tray;
#[allow(dead_code)]
mod ui;

use app::App;

fn main() -> iced::Result {
    env_logger::init();

    iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .centered()
        .run()
}
