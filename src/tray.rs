use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ksni::menu::{MenuItem, StandardItem};
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
    recording: Arc<AtomicBool>,
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
        if self.recording.load(Ordering::Relaxed) {
            "media-playback-stop".into()
        } else {
            "media-record".into()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let is_recording = self.recording.load(Ordering::Relaxed);
        let start_sender = self.sender.clone();
        let stop_sender = self.sender.clone();
        let editor_sender = self.sender.clone();
        let quit_sender = self.sender.clone();

        vec![
            StandardItem {
                label: "Начать запись".into(),
                enabled: !is_recording,
                activate: Box::new(move |_| {
                    let _ = start_sender.try_send(TrayCommand::StartRecording);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Остановить запись".into(),
                enabled: is_recording,
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

// Note: OpenRecTray is private and requires a running D-Bus session,
// so integration tests for tray menu are not feasible in unit tests.

impl TrayService {
    pub fn spawn(
        sender: mpsc::Sender<TrayCommand>,
        recording: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let tray = OpenRecTray { recording, sender };
            match tray.spawn().await {
                Ok(handle) => {
                    log::info!("System tray started");
                    // ksni Handle has no shutdown signal to await;
                    // pending() keeps the task alive without polling
                    std::future::pending::<()>().await;
                    drop(handle);
                }
                Err(e) => {
                    log::error!("Failed to start system tray: {e}");
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tray_command_variants() {
        let commands = vec![
            TrayCommand::StartRecording,
            TrayCommand::StopRecording,
            TrayCommand::OpenEditor,
            TrayCommand::Quit,
        ];

        for cmd in &commands {
            let cloned = cmd.clone();
            // Debug format works
            let _ = format!("{:?}", cloned);
        }

        assert_eq!(commands.len(), 4);
    }

    #[test]
    fn test_recording_state_toggle() {
        let recording = Arc::new(AtomicBool::new(false));
        assert!(!recording.load(Ordering::Relaxed));

        recording.store(true, Ordering::Relaxed);
        assert!(recording.load(Ordering::Relaxed));

        recording.store(false, Ordering::Relaxed);
        assert!(!recording.load(Ordering::Relaxed));
    }
}
