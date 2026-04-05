use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iced::widget::{button, column, container, text};
use iced::{Element, Length, Subscription, Task};
use tokio::sync::{mpsc, oneshot, Mutex as TokioMutex};

use crate::capture::cursor::CursorTelemetry;
use crate::capture::pipeline::RecordingPipeline;
use crate::config::AppConfig;
use crate::tray::{TrayCommand, TrayService};
use crate::ui::recorder_hud::{recorder_idle_view, recorder_recording_view};

#[derive(Debug, PartialEq)]
pub enum AppMode {
    Idle,
    Recording,
    Editor,
}

/// Handle для управления pipeline.
/// Pipeline живёт в отдельном std::thread; stop отправляется через oneshot.
pub struct PipelineHandle {
    stop_tx: Option<oneshot::Sender<()>>,
    result_rx: Option<oneshot::Receiver<Result<PathBuf, String>>>,
}

pub struct App {
    pub(crate) mode: AppMode,
    pub(crate) recording_duration: Option<Duration>,
    config: AppConfig,
    pipeline_handle: Arc<Mutex<Option<PipelineHandle>>>,
    cursor_telemetry: Option<CursorTelemetry>,
    recording_start: Option<Instant>,
    #[allow(dead_code)] // held to keep Arc alive; accessed via TRAY_RX static
    tray_receiver: Arc<TokioMutex<mpsc::Receiver<TrayCommand>>>,
    recording_flag: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
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

/// Запускает pipeline в отдельном std::thread (pipeline не Send).
/// Возвращает через oneshot когда pipeline.start() завершился.
/// PipelineHandle сохраняется в shared slot.
fn launch_pipeline(
    config: AppConfig,
    handle_slot: Arc<Mutex<Option<PipelineHandle>>>,
    cancelled: Arc<AtomicBool>,
) -> oneshot::Receiver<Result<(), String>> {
    let (started_tx, started_rx) = oneshot::channel();
    let (stop_tx, stop_rx) = oneshot::channel();
    let (result_tx, result_rx) = oneshot::channel();

    std::thread::Builder::new()
        .name("recording-pipeline".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to create tokio runtime for pipeline");

            rt.block_on(async {
                let mut pipeline = RecordingPipeline::new(&config);
                match pipeline.start().await {
                    Ok(()) => {
                        // Сохранить handle в shared slot
                        {
                            let mut slot = handle_slot.lock().unwrap();
                            *slot = Some(PipelineHandle {
                                stop_tx: Some(stop_tx),
                                result_rx: Some(result_rx),
                            });
                        }
                        // Сообщить что pipeline запущен
                        let _ = started_tx.send(Ok(()));

                        // Ждём сигнал stop
                        let _ = stop_rx.await;
                        let result = pipeline.stop().await.map_err(|e| format!("{e:#}"));
                        let _ = result_tx.send(result);
                    }
                    Err(e) => {
                        let _ = started_tx.send(Err(format!("{e:#}")));
                    }
                }
            });
        })
        .expect("failed to spawn pipeline thread");

    let _ = cancelled; // будет использоваться в RecordingStarted

    started_rx
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
            pipeline_handle: Arc::new(Mutex::new(None)),
            cursor_telemetry: None,
            recording_start: None,
            tray_receiver,
            recording_flag,
            cancelled: Arc::new(AtomicBool::new(false)),
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
                self.cancelled.store(false, Ordering::Relaxed);

                // Запустить cursor telemetry
                let mut cursor = CursorTelemetry::default();
                if let Err(e) = cursor.start() {
                    log::warn!("Failed to start cursor telemetry: {e}");
                }
                self.cursor_telemetry = Some(cursor);

                // Запустить pipeline в отдельном потоке
                let handle_slot = self.pipeline_handle.clone();
                let cancelled = self.cancelled.clone();
                let started_rx = launch_pipeline(self.config.clone(), handle_slot, cancelled);

                Task::perform(
                    async move {
                        started_rx.await.unwrap_or(Err("pipeline channel closed".into()))
                    },
                    Message::RecordingStarted,
                )
            }
            Message::StopRecording => {
                if self.mode != AppMode::Recording {
                    return Task::none();
                }
                self.mode = AppMode::Idle;
                self.recording_flag.store(false, Ordering::Relaxed);
                self.cancelled.store(true, Ordering::Relaxed);

                // Остановить cursor telemetry
                if let Some(mut cursor) = self.cursor_telemetry.take() {
                    let positions = cursor.stop();
                    log::info!("Cursor positions recorded: {}", positions.len());
                }

                // Остановить pipeline через handle
                let mut handle_opt = self.pipeline_handle.lock().unwrap().take();
                if let Some(ref mut handle) = handle_opt {
                    if let Some(tx) = handle.stop_tx.take() {
                        let _ = tx.send(());
                    }
                    if let Some(rx) = handle.result_rx.take() {
                        self.recording_start = None;
                        self.recording_duration = None;
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
                        // Если запись была отменена пока pipeline запускался
                        if self.cancelled.load(Ordering::Relaxed)
                            || self.mode != AppMode::Recording
                        {
                            log::info!("Recording cancelled during startup, stopping pipeline");
                            let mut handle_opt = self.pipeline_handle.lock().unwrap().take();
                            if let Some(ref mut handle) = handle_opt {
                                if let Some(tx) = handle.stop_tx.take() {
                                    let _ = tx.send(());
                                }
                            }
                            return Task::none();
                        }
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
        match self.mode {
            AppMode::Idle => recorder_idle_view(),
            AppMode::Recording => {
                recorder_recording_view(self.recording_duration.unwrap_or(Duration::ZERO))
            }
            AppMode::Editor => {
                let content = column![
                    text("Редактор (в разработке)").size(24),
                    button("Назад").on_press(Message::BackToIdle),
                    button("Выход").on_press(Message::Quit),
                ];
                container(content.spacing(20).align_x(iced::Alignment::Center))
                    .center(Length::Fill)
                    .into()
            }
        }
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
        let (_tx, rx) = mpsc::channel(1);
        App {
            mode: AppMode::Idle,
            recording_duration: None,
            config: AppConfig::default(),
            pipeline_handle: Arc::new(Mutex::new(None)),
            cursor_telemetry: None,
            recording_start: None,
            tray_receiver: Arc::new(TokioMutex::new(rx)),
            recording_flag: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn test_boot_loads_config() {
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

        let _ = app.update(Message::StartRecording);
        assert!(app.recording_flag.load(Ordering::Relaxed));
        assert_eq!(app.mode, AppMode::Recording);

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

        let _ = app.update(Message::StartRecording);
        assert_eq!(app.mode, AppMode::Recording);
    }

    #[test]
    fn test_stop_when_not_recording() {
        let mut app = new_app();
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
    fn test_cancellation_during_start() {
        let mut app = new_app();
        app.mode = AppMode::Recording;
        app.cancelled.store(true, Ordering::Relaxed);

        // Pipeline started OK but mode was cancelled
        let _ = app.update(Message::RecordingStarted(Ok(())));
        // Pipeline handle should be cleaned up (taken from slot)
        assert!(app.pipeline_handle.lock().unwrap().is_none());
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
