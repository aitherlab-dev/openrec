use anyhow::{Context, Result};
use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use ashpd::desktop::Session;
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::config::HotkeyConfig;

const SHORTCUT_TOGGLE: &str = "toggle-recording";
const SHORTCUT_CANCEL: &str = "cancel-recording";

/// Действие горячей клавиши.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    ToggleRecording,
    CancelRecording,
}

/// Описание привязки клавиши (результат парсинга).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    pub modifiers: Vec<String>,
    pub key: String,
}

/// Парсит строку типа "Super+Shift+R" в модификаторы + ключ.
pub fn parse_keybinding(s: &str) -> KeyBinding {
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    if parts.len() <= 1 {
        return KeyBinding {
            modifiers: Vec::new(),
            key: s.trim().to_string(),
        };
    }
    let (mods, key) = parts.split_at(parts.len() - 1);
    KeyBinding {
        modifiers: mods.iter().map(|m| m.to_string()).collect(),
        key: key[0].to_string(),
    }
}

/// Сервис глобальных горячих клавиш через xdg-desktop-portal.
pub struct HotkeyService {
    shortcuts: Vec<NewShortcut>,
}

impl HotkeyService {
    pub fn new(config: &HotkeyConfig) -> Self {
        let shortcuts = vec![
            NewShortcut::new(SHORTCUT_TOGGLE, "Начать/остановить запись")
                .preferred_trigger(Some(config.toggle_recording.as_str())),
            NewShortcut::new(SHORTCUT_CANCEL, "Отменить запись")
                .preferred_trigger(Some(config.cancel_recording.as_str())),
        ];
        Self { shortcuts }
    }

    /// Запускает слушатель горячих клавиш в отдельной tokio-задаче.
    pub fn spawn(self, sender: mpsc::Sender<HotkeyAction>) -> JoinHandle<()> {
        tokio::spawn(async move {
            if let Err(e) = run_hotkey_listener(self.shortcuts, sender).await {
                log::error!("Hotkey listener failed: {e}");
            }
        })
    }
}

async fn run_hotkey_listener(
    shortcuts: Vec<NewShortcut>,
    sender: mpsc::Sender<HotkeyAction>,
) -> Result<()> {
    let portal = GlobalShortcuts::new()
        .await
        .context("failed to connect to GlobalShortcuts portal")?;

    let session: Session<GlobalShortcuts> = portal
        .create_session(Default::default())
        .await
        .context("failed to create GlobalShortcuts session")?;

    portal
        .bind_shortcuts(&session, &shortcuts, None, Default::default())
        .await
        .context("failed to bind shortcuts")?
        .response()
        .context("bind_shortcuts was rejected")?;

    log::info!("Global shortcuts registered");

    let mut activated = portal
        .receive_activated()
        .await
        .context("failed to subscribe to shortcut activations")?;

    while let Some(event) = activated.next().await {
        let action = match event.shortcut_id() {
            SHORTCUT_TOGGLE => Some(HotkeyAction::ToggleRecording),
            SHORTCUT_CANCEL => Some(HotkeyAction::CancelRecording),
            id => {
                log::debug!("unknown shortcut activated: {id}");
                None
            }
        };
        if let Some(action) = action {
            log::info!("Hotkey action: {action:?}");
            if sender.send(action).await.is_err() {
                log::warn!("Hotkey receiver dropped, stopping listener");
                break;
            }
        }
    }

    let _ = session.close().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hotkey_action_variants() {
        assert_ne!(HotkeyAction::ToggleRecording, HotkeyAction::CancelRecording);
        assert_eq!(HotkeyAction::ToggleRecording, HotkeyAction::ToggleRecording);
    }

    #[test]
    fn test_hotkey_config_parsing_with_modifiers() {
        let kb = parse_keybinding("Super+Shift+R");
        assert_eq!(kb.modifiers, vec!["Super", "Shift"]);
        assert_eq!(kb.key, "R");
    }

    #[test]
    fn test_hotkey_config_parsing_single_key() {
        let kb = parse_keybinding("Escape");
        assert!(kb.modifiers.is_empty());
        assert_eq!(kb.key, "Escape");
    }

    #[test]
    fn test_hotkey_config_parsing_two_parts() {
        let kb = parse_keybinding("Ctrl+C");
        assert_eq!(kb.modifiers, vec!["Ctrl"]);
        assert_eq!(kb.key, "C");
    }

    #[test]
    fn test_hotkey_service_creates_shortcuts() {
        let config = HotkeyConfig::default();
        let service = HotkeyService::new(&config);
        assert_eq!(service.shortcuts.len(), 2);
    }

    #[test]
    #[ignore]
    fn integration_register_shortcuts() {
        // Требует активную D-Bus GUI-сессию с поддержкой GlobalShortcuts
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let config = HotkeyConfig::default();
            let service = HotkeyService::new(&config);
            let (tx, _rx) = mpsc::channel(16);
            let handle = service.spawn(tx);
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            handle.abort();
        });
    }
}
