use ksni::menu::{StandardItem, MenuItem};
use ksni::{Tray, TrayMethods};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

#[derive(Debug, Clone)]
pub enum TrayCommand {
    StartRecording,
    StopRecording,
    OpenEditor,
    Quit,
}

struct OpenRecTray {
    recording: bool,
    sender: mpsc::Sender<TrayCommand>,
}

impl Tray for OpenRecTray {
    fn id(&self) -> String {
        "openrec".into()
    }

    fn title(&self) -> String {
        "OpenRec".into()
    }

    fn icon_name(&self) -> String {
        "media-record".into()
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let start_sender = self.sender.clone();
        let stop_sender = self.sender.clone();
        let editor_sender = self.sender.clone();
        let quit_sender = self.sender.clone();

        vec![
            StandardItem {
                label: "Начать запись".into(),
                enabled: !self.recording,
                activate: Box::new(move |_| {
                    let _ = start_sender.try_send(TrayCommand::StartRecording);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Остановить запись".into(),
                enabled: self.recording,
                activate: Box::new(move |_| {
                    let _ = stop_sender.try_send(TrayCommand::StopRecording);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Открыть редактор".into(),
                activate: Box::new(move |_| {
                    let _ = editor_sender.try_send(TrayCommand::OpenEditor);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Выход".into(),
                activate: Box::new(move |_| {
                    let _ = quit_sender.try_send(TrayCommand::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub struct TrayService;

impl TrayService {
    pub fn spawn(sender: mpsc::Sender<TrayCommand>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let tray = OpenRecTray {
                recording: false,
                sender,
            };
            match tray.spawn().await {
                Ok(handle) => {
                    log::info!("System tray started");
                    // Keep handle alive until the tray service shuts down
                    while !handle.is_closed() {
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                }
                Err(e) => {
                    log::error!("Failed to start system tray: {e}");
                }
            }
        })
    }
}
