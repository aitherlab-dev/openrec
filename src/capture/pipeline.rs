use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use crossbeam_channel::{bounded, Receiver};

use crate::capture::pipewire_capture::{PipeWireCapture, PixelFormat, VideoFrame};
use crate::capture::portal::ScreencastSession;
use crate::config::{AppConfig, VideoCodec};
use crate::export::ffmpeg::{Codec, EncoderConfig, FfmpegEncoder};

#[derive(Debug, Clone, PartialEq)]
pub enum RecordingState {
    Idle,
    Starting,
    Recording,
    Stopping,
    Error(String),
}

pub struct RecordingPipeline {
    config: AppConfig,
    state: RecordingState,
    start_time: Option<Instant>,
    session: Option<ScreencastSession>,
    capture: Option<PipeWireCapture>,
    writer_thread: Option<thread::JoinHandle<Result<()>>>,
    writer_stop: Option<Arc<AtomicBool>>,
    output_path: Option<PathBuf>,
}

/// Генерирует имя файла вида `openrec_YYYY-MM-DD_HH-MM-SS.mp4`.
pub fn generate_output_filename(recordings_dir: &std::path::Path) -> PathBuf {
    let now = chrono::Local::now();
    let name = now.format("openrec_%Y-%m-%d_%H-%M-%S.mp4").to_string();
    recordings_dir.join(name)
}

fn codec_from_config(codec: &VideoCodec) -> Codec {
    match codec {
        VideoCodec::H264 => Codec::H264,
        VideoCodec::H265 => Codec::H265,
        VideoCodec::AV1 => Codec::AV1,
    }
}

fn pixel_format_string(fmt: PixelFormat) -> String {
    match fmt {
        PixelFormat::BGRA => "bgra".to_string(),
        PixelFormat::RGBA => "rgba".to_string(),
        PixelFormat::BGRx => "bgr0".to_string(),
    }
}

/// Поток-мост: читает фреймы из crossbeam receiver и пишет в ffmpeg.
fn writer_loop(
    receiver: Receiver<VideoFrame>,
    mut encoder: FfmpegEncoder,
    stop_flag: Arc<AtomicBool>,
) -> Result<()> {
    while !stop_flag.load(Ordering::Relaxed) {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(frame) => {
                encoder.write_frame(&frame.data)?;
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }

    // Дочитать оставшиеся фреймы из канала
    for frame in receiver.try_iter() {
        encoder.write_frame(&frame.data)?;
    }

    encoder.finish()?;
    Ok(())
}

impl RecordingPipeline {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            config: config.clone(),
            state: RecordingState::Idle,
            start_time: None,
            session: None,
            capture: None,
            writer_thread: None,
            writer_stop: None,
            output_path: None,
        }
    }

    pub fn state(&self) -> &RecordingState {
        &self.state
    }

    pub fn duration(&self) -> Duration {
        self.start_time
            .map(|t| t.elapsed())
            .unwrap_or(Duration::ZERO)
    }

    /// Запускает полный цикл записи:
    /// portal → select source → pipewire capture → ffmpeg encoder.
    pub async fn start(&mut self) -> Result<()> {
        if self.state != RecordingState::Idle {
            bail!("cannot start: pipeline is in {:?} state", self.state);
        }

        self.state = RecordingState::Starting;

        // 1. Portal session
        let session = ScreencastSession::new()
            .await
            .context("failed to create screencast session")?;

        // 2. Пользователь выбирает экран
        let source = session
            .select_source()
            .await
            .context("failed to select source")?;

        let (width, height) = source.size.unwrap_or((1920, 1080));
        let width = width as u32;
        let height = height as u32;

        log::info!(
            "Selected source: node_id={}, size={}x{}",
            source.node_id,
            width,
            height
        );

        // 3. PipeWire capture
        let mut capture = PipeWireCapture::new(source.node_id, self.config.video_fps)
            .context("failed to create PipeWire capture")?;

        let (frame_tx, frame_rx) = bounded::<VideoFrame>(4);
        capture
            .start(frame_tx)
            .context("failed to start PipeWire capture")?;

        // 4. Ждём первый фрейм чтобы узнать реальный формат
        let first_frame = frame_rx
            .recv_timeout(Duration::from_secs(5))
            .context("timeout waiting for first frame from PipeWire")?;

        let pixel_fmt = pixel_format_string(first_frame.format);

        // 5. Подготовить output path
        let recordings_dir = self
            .config
            .ensure_recordings_dir()
            .context("failed to create recordings dir")?;
        let output_path = generate_output_filename(recordings_dir);

        // 6. ffmpeg encoder
        let encoder_config = EncoderConfig {
            output_path: output_path.clone(),
            width: first_frame.width,
            height: first_frame.height,
            fps: self.config.video_fps,
            codec: codec_from_config(&self.config.video_codec),
            bitrate: None,
            pixel_format: pixel_fmt,
        };

        let mut encoder =
            FfmpegEncoder::new(encoder_config).context("failed to start ffmpeg encoder")?;

        // Записать первый фрейм
        encoder.write_frame(&first_frame.data)?;

        // 7. Writer thread
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = stop_flag.clone();

        let handle = thread::spawn(move || writer_loop(frame_rx, encoder, stop_clone));

        self.session = Some(session);
        self.capture = Some(capture);
        self.writer_thread = Some(handle);
        self.writer_stop = Some(stop_flag);
        self.output_path = Some(output_path);
        self.start_time = Some(Instant::now());
        self.state = RecordingState::Recording;

        log::info!("Recording started");
        Ok(())
    }

    /// Останавливает запись и возвращает путь к файлу.
    pub async fn stop(&mut self) -> Result<PathBuf> {
        if self.state != RecordingState::Recording {
            bail!("cannot stop: pipeline is in {:?} state", self.state);
        }

        self.state = RecordingState::Stopping;

        // 1. Остановить PipeWire capture (закроет sender → канал отвалится)
        if let Some(mut capture) = self.capture.take() {
            capture.stop();
        }

        // 2. Сигнал writer thread остановиться
        if let Some(flag) = self.writer_stop.take() {
            flag.store(true, Ordering::Relaxed);
        }

        // 3. Дождаться writer thread (он вызовет encoder.finish())
        if let Some(handle) = self.writer_thread.take() {
            match handle.join() {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    self.state = RecordingState::Error(e.to_string());
                    bail!("writer thread error: {e:#}");
                }
                Err(_) => {
                    self.state = RecordingState::Error("writer thread panicked".to_string());
                    bail!("writer thread panicked");
                }
            }
        }

        // 4. Закрыть portal session
        if let Some(mut session) = self.session.take() {
            if let Err(e) = session.close().await {
                log::warn!("failed to close screencast session: {e:#}");
            }
        }

        let output_path = self.output_path.take().context("no output path")?;
        self.start_time = None;
        self.state = RecordingState::Idle;

        log::info!("Recording saved to {}", output_path.display());
        Ok(output_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recording_state_transitions() {
        let idle = RecordingState::Idle;
        let starting = RecordingState::Starting;
        let recording = RecordingState::Recording;
        let stopping = RecordingState::Stopping;
        let error = RecordingState::Error("test error".to_string());

        assert_eq!(idle, RecordingState::Idle);
        assert_eq!(starting, RecordingState::Starting);
        assert_eq!(recording, RecordingState::Recording);
        assert_eq!(stopping, RecordingState::Stopping);
        assert_eq!(error, RecordingState::Error("test error".to_string()));
        assert_ne!(idle, recording);
    }

    #[test]
    fn test_pipeline_initial_state() {
        let config = AppConfig::default();
        let pipeline = RecordingPipeline::new(&config);

        assert_eq!(pipeline.state(), &RecordingState::Idle);
        assert_eq!(pipeline.duration(), Duration::ZERO);
        assert!(pipeline.output_path.is_none());
    }

    #[test]
    fn test_output_filename_format() {
        let dir = std::path::Path::new("/tmp/recordings");
        let path = generate_output_filename(dir);

        let filename = path.file_name().unwrap().to_str().unwrap();
        assert!(
            filename.starts_with("openrec_"),
            "filename should start with 'openrec_': {filename}"
        );
        assert!(
            filename.ends_with(".mp4"),
            "filename should end with '.mp4': {filename}"
        );
        // openrec_YYYY-MM-DD_HH-MM-SS.mp4
        // Проверяем формат через паттерн, не длину
        let parts: Vec<&str> = filename
            .trim_start_matches("openrec_")
            .trim_end_matches(".mp4")
            .split('_')
            .collect();
        assert_eq!(parts.len(), 2, "expected date_time parts: {filename}");
        assert_eq!(parts[0].len(), 10, "date part YYYY-MM-DD: {}", parts[0]);
        assert_eq!(parts[1].len(), 8, "time part HH-MM-SS: {}", parts[1]);
        assert_eq!(path.parent().unwrap(), dir);
    }

    #[test]
    fn test_codec_from_config() {
        assert!(matches!(codec_from_config(&VideoCodec::H264), Codec::H264));
        assert!(matches!(codec_from_config(&VideoCodec::H265), Codec::H265));
        assert!(matches!(codec_from_config(&VideoCodec::AV1), Codec::AV1));
    }

    #[test]
    fn test_pixel_format_string() {
        assert_eq!(pixel_format_string(PixelFormat::BGRA), "bgra");
        assert_eq!(pixel_format_string(PixelFormat::RGBA), "rgba");
        assert_eq!(pixel_format_string(PixelFormat::BGRx), "bgr0");
    }

    #[test]
    #[ignore] // requires D-Bus + PipeWire + portal dialog
    fn integration_full_recording_cycle() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let config = AppConfig::default();
            let mut pipeline = RecordingPipeline::new(&config);

            pipeline.start().await.unwrap();
            assert_eq!(pipeline.state(), &RecordingState::Recording);

            // Записать 2 секунды
            tokio::time::sleep(Duration::from_secs(2)).await;
            assert!(pipeline.duration() >= Duration::from_secs(1));

            let output = pipeline.stop().await.unwrap();
            assert!(output.exists(), "output file must exist");
            assert_eq!(pipeline.state(), &RecordingState::Idle);
        });
    }
}
