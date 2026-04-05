use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::widget::{button, column, container, text};
use iced::{Element, Length, Subscription, Task};
use tokio::sync::{mpsc, oneshot, Mutex as TokioMutex};

use crate::capture::cursor::CursorTelemetry;
use crate::capture::pipeline::RecordingPipeline;
use crate::config::AppConfig;
use crate::tray::{TrayCommand, TrayService};

#[derive(Debug, PartialEq)]
pub enum AppMode {
    Idle,
    Recording,
    Editor,
}

/// Handle для управления pipeline из async контекста.
/// Pipeline живёт в отдельной tokio task; stop отправляется через oneshot.
pub struct PipelineHandle {
    stop_tx: Option<oneshot::Sender<()>>,
    result_rx: Option<oneshot::Receiver<Result<PathBuf, String>>>,
}

pub struct App {
    pub(crate) mode: AppMode,
    pub(crate) recording_duration: Option<Duration>,
    config: AppConfig,
    pipeline_handle: Option<PipelineHandle>,
    cursor_telemetry: Option<CursorTelemetry>,
    recording_start: Option<Instant>,
    #[allow(dead_code)] // held to keep Arc alive; accessed via TRAY_RX static
    tray_receiver: Arc<TokioMutex<mpsc::Receiver<TrayCommand>>>,
    recording_flag: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
pub enum Message {
    StartRecording,
    StopRecording,
    OpenEditor,
    BackToIdle,
    Quit,
    RecordingStarted(Result<(), String>),
    RecordingStopped(Result<PathBuf, String>),
    TrayCommand(TrayCommand),
    TimerTick,
}

/// Маппинг TrayCommand → Message.
pub fn tray_command_to_message(cmd: TrayCommand) -> Message {
    match cmd {
        TrayCommand::StartRecording => Message::StartRecording,
        TrayCommand::StopRecording => Message::StopRecording,
        TrayCommand::OpenEditor => Message::OpenEditor,
        TrayCommand::Quit => Message::Quit,
    }
}

/// Запускает pipeline в отдельной tokio task. Возвращает handle для управления.
async fn start_pipeline(config: AppConfig) -> Result<PipelineHandle, String> {
    let (stop_tx, stop_rx) = oneshot::channel();
    let (result_tx, result_rx) = oneshot::channel();

    tokio::task::spawn_local(async move {
        let mut pipeline = RecordingPipeline::new(&config);
        match pipeline.start().await {
            Ok(()) => {
                // Ждём сигнал stop
                let _ = stop_rx.await;
                let result = pipeline
                    .stop()
                    .await
                    .map_err(|e| format!("{e:#}"));
                let _ = result_tx.send(result);
            }
            Err(e) => {
                let _ = result_tx.send(Err(format!("{e:#}")));
            }
        }
    });

    // Даём task время запуститься
    tokio::task::yield_now().await;

    Ok(PipelineHandle {
        stop_tx: Some(stop_tx),
        result_rx: Some(result_rx),
    })
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let config = AppConfig::load().unwrap_or_else(|e| {
            log::warn!("Failed to load config, using defaults: {e}");
            AppConfig::default()
        });

        let (tray_tx, tray_rx) = mpsc::channel(32);
        let recording_flag = Arc::new(AtomicBool::new(false));

        let tray_receiver = Arc::new(TokioMutex::new(tray_rx));
        let _ = TRAY_RX.set(tray_receiver.clone());

        TrayService::spawn(tray_tx, recording_flag.clone());

        let app = Self {
            mode: AppMode::Idle,
            recording_duration: None,
            config,
            pipeline_handle: None,
            cursor_telemetry: None,
            recording_start: None,
            tray_receiver,
            recording_flag,
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

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subs = Vec::new();

        // Таймер — тикает каждую секунду при записи
        if self.mode == AppMode::Recording {
            subs.push(
                iced::time::every(Duration::from_secs(1)).map(|_| Message::TimerTick),
            );
        }

        // Tray commands
        subs.push(Subscription::run_with(0u8, tray_subscription));

        Subscription::batch(subs)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::StartRecording => {
                if self.mode != AppMode::Idle {
                    return Task::none();
                }
                self.mode = AppMode::Recording;
                self.recording_start = Some(Instant::now());
                self.recording_duration = Some(Duration::ZERO);
                self.recording_flag.store(true, Ordering::Relaxed);

                // Запустить cursor telemetry
                let mut cursor = CursorTelemetry::default();
                if let Err(e) = cursor.start() {
                    log::warn!("Failed to start cursor telemetry: {e}");
                }
                self.cursor_telemetry = Some(cursor);

                // Запустить pipeline async
                let config = self.config.clone();
                Task::perform(
                    async move {
                        start_pipeline(config)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    |result| match result {
                        Ok(_handle) => Message::RecordingStarted(Ok(())),
                        Err(e) => Message::RecordingStarted(Err(e)),
                    },
                )
            }
            Message::StopRecording => {
                if self.mode != AppMode::Recording {
                    return Task::none();
                }
                self.mode = AppMode::Idle;
                self.recording_flag.store(false, Ordering::Relaxed);

                // Остановить cursor telemetry
                if let Some(mut cursor) = self.cursor_telemetry.take() {
                    let positions = cursor.stop();
                    log::info!("Cursor positions recorded: {}", positions.len());
                }

                // Остановить pipeline
                if let Some(mut handle) = self.pipeline_handle.take() {
                    if let Some(tx) = handle.stop_tx.take() {
                        let _ = tx.send(());
                    }
                    if let Some(rx) = handle.result_rx.take() {
                        return Task::perform(
                            async move { rx.await.unwrap_or(Err("channel closed".into())) },
                            Message::RecordingStopped,
                        );
                    }
                }

                self.recording_start = None;
                self.recording_duration = None;
                Task::none()
            }
            Message::RecordingStarted(result) => {
                match result {
                    Ok(()) => {
                        log::info!("Recording pipeline started successfully");
                    }
                    Err(e) => {
                        log::error!("Failed to start recording: {e}");
                        self.mode = AppMode::Idle;
                        self.recording_flag.store(false, Ordering::Relaxed);
                        self.recording_start = None;
                        self.recording_duration = None;
                        if let Some(mut cursor) = self.cursor_telemetry.take() {
                            cursor.stop();
                        }
                    }
                }
                Task::none()
            }
            Message::RecordingStopped(result) => {
                self.recording_start = None;
                self.recording_duration = None;
                match result {
                    Ok(path) => {
                        log::info!("Recording saved: {}", path.display());
                    }
                    Err(e) => {
                        log::error!("Recording failed: {e}");
                    }
                }
                Task::none()
            }
            Message::TrayCommand(cmd) => {
                let msg = tray_command_to_message(cmd);
                self.update(msg)
            }
            Message::TimerTick => {
                if let Some(start) = self.recording_start {
                    self.recording_duration = Some(start.elapsed());
                }
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
            AppMode::Recording => {
                let duration_text = match self.recording_duration {
                    Some(d) => {
                        let secs = d.as_secs();
                        format!("Идёт запись... {:02}:{:02}", secs / 60, secs % 60)
                    }
                    None => "Идёт запись...".to_string(),
                };
                column![
                    text(duration_text).size(24),
                    button("Остановить").on_press(Message::StopRecording),
                ]
            }
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

type TrayReceiver = Arc<TokioMutex<mpsc::Receiver<TrayCommand>>>;

static TRAY_RX: std::sync::OnceLock<TrayReceiver> = std::sync::OnceLock::new();

/// Subscription stream для чтения TrayCommand из mpsc канала.
fn tray_subscription(_id: &u8) -> impl futures::Stream<Item = Message> {
    async_stream::stream! {
        let Some(rx) = TRAY_RX.get() else { return };
        while let Some(cmd) = rx.lock().await.recv().await {
            yield Message::TrayCommand(cmd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_app() -> App {
        // Для тестов не запускаем трей — создаём dummy канал
        let (_tx, rx) = mpsc::channel(1);
        App {
            mode: AppMode::Idle,
            recording_duration: None,
            config: AppConfig::default(),
            pipeline_handle: None,
            cursor_telemetry: None,
            recording_start: None,
            tray_receiver: Arc::new(TokioMutex::new(rx)),
            recording_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn test_boot_loads_config() {
        // boot() запускает TrayService и требует tokio runtime
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (app, _task) = rt.block_on(async { App::boot() });
        assert_eq!(app.mode, AppMode::Idle);
        assert!(app.config.video_fps > 0);
        assert!(!app.recording_flag.load(Ordering::Relaxed));
    }

    #[test]
    fn test_recording_flag_sync() {
        let mut app = new_app();
        assert!(!app.recording_flag.load(Ordering::Relaxed));

        // StartRecording sets flag
        let _ = app.update(Message::StartRecording);
        assert!(app.recording_flag.load(Ordering::Relaxed));
        assert_eq!(app.mode, AppMode::Recording);

        // StopRecording clears flag
        let _ = app.update(Message::StopRecording);
        assert!(!app.recording_flag.load(Ordering::Relaxed));
        assert_eq!(app.mode, AppMode::Idle);
    }

    #[test]
    fn test_tray_command_mapping() {
        assert!(matches!(
            tray_command_to_message(TrayCommand::StartRecording),
            Message::StartRecording
        ));
        assert!(matches!(
            tray_command_to_message(TrayCommand::StopRecording),
            Message::StopRecording
        ));
        assert!(matches!(
            tray_command_to_message(TrayCommand::OpenEditor),
            Message::OpenEditor
        ));
        assert!(matches!(
            tray_command_to_message(TrayCommand::Quit),
            Message::Quit
        ));
    }

    #[test]
    fn test_timer_tick_updates_duration() {
        let mut app = new_app();
        app.mode = AppMode::Recording;
        app.recording_start = Some(Instant::now() - Duration::from_secs(5));
        app.recording_duration = Some(Duration::ZERO);

        let _ = app.update(Message::TimerTick);
        let dur = app.recording_duration.unwrap();
        assert!(dur.as_secs() >= 4);
    }

    #[test]
    fn test_start_recording_idempotent() {
        let mut app = new_app();
        let _ = app.update(Message::StartRecording);
        assert_eq!(app.mode, AppMode::Recording);

        // Second start should be ignored
        let _ = app.update(Message::StartRecording);
        assert_eq!(app.mode, AppMode::Recording);
    }

    #[test]
    fn test_stop_when_not_recording() {
        let mut app = new_app();
        // Stop when idle — no change
        let _ = app.update(Message::StopRecording);
        assert_eq!(app.mode, AppMode::Idle);
    }

    #[test]
    fn test_recording_started_error_resets_state() {
        let mut app = new_app();
        app.mode = AppMode::Recording;
        app.recording_flag.store(true, Ordering::Relaxed);
        app.recording_start = Some(Instant::now());

        let _ = app.update(Message::RecordingStarted(Err("test error".into())));
        assert_eq!(app.mode, AppMode::Idle);
        assert!(!app.recording_flag.load(Ordering::Relaxed));
        assert!(app.recording_start.is_none());
    }

    #[test]
    fn test_title_idle() {
        let app = new_app();
        assert_eq!(app.title(), "OpenRec");
    }

    #[test]
    fn test_title_recording() {
        let mut app = new_app();
        let _ = app.update(Message::StartRecording);
        assert!(app.title().contains("Запись"));
    }

    #[test]
    fn test_title_editor() {
        let mut app = new_app();
        let _ = app.update(Message::OpenEditor);
        assert!(app.title().contains("Редактор"));
    }
}
