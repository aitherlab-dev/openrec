use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Позиция курсора с временной меткой.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CursorPosition {
    pub x: f64,
    pub y: f64,
    pub timestamp_ms: u64,
}

/// Сервис записи позиции курсора для авто-зума.
pub struct CursorTelemetry {
    poll_interval: Duration,
    positions: Arc<Mutex<Vec<CursorPosition>>>,
    stop_tx: Option<crossbeam_channel::Sender<()>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl CursorTelemetry {
    pub fn new(poll_interval_ms: u64) -> Self {
        Self {
            poll_interval: Duration::from_millis(poll_interval_ms),
            positions: Arc::new(Mutex::new(Vec::new())),
            stop_tx: None,
            handle: None,
        }
    }

    /// Запускает поток опроса позиции курсора.
    pub fn start(&mut self) -> Result<()> {
        if self.is_running() {
            anyhow::bail!("cursor telemetry is already running");
        }

        let (stop_tx, stop_rx) = crossbeam_channel::bounded(1);
        let positions = Arc::clone(&self.positions);
        let interval = self.poll_interval;

        // Очищаем предыдущие данные
        positions.lock().unwrap_or_else(|e| e.into_inner()).clear();

        let start_time = Instant::now();

        let handle = thread::Builder::new()
            .name("cursor-telemetry".into())
            .spawn(move || {
                poll_loop(stop_rx, positions, interval, start_time);
            })
            .context("failed to spawn cursor telemetry thread")?;

        self.stop_tx = Some(stop_tx);
        self.handle = Some(handle);
        log::info!("Cursor telemetry started (interval: {}ms)", interval.as_millis());
        Ok(())
    }

    /// Останавливает запись и возвращает все позиции.
    pub fn stop(&mut self) -> Vec<CursorPosition> {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        let positions = std::mem::take(&mut *self.positions.lock().unwrap_or_else(|e| e.into_inner()));
        log::info!("Cursor telemetry stopped, {} positions recorded", positions.len());
        positions
    }

    pub fn is_running(&self) -> bool {
        self.handle
            .as_ref()
            .is_some_and(|h| !h.is_finished())
    }

    /// Количество записанных позиций (для диагностики).
    pub fn positions_count(&self) -> usize {
        self.positions.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

impl Default for CursorTelemetry {
    fn default() -> Self {
        Self::new(16) // ~60Hz
    }
}

impl Drop for CursorTelemetry {
    fn drop(&mut self) {
        if self.is_running() {
            log::warn!("CursorTelemetry dropped while running, stopping");
            self.stop();
        }
    }
}

fn poll_loop(
    stop_rx: crossbeam_channel::Receiver<()>,
    positions: Arc<Mutex<Vec<CursorPosition>>>,
    interval: Duration,
    start_time: Instant,
) {
    loop {
        match stop_rx.recv_timeout(interval) {
            Ok(()) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }

        let timestamp_ms = start_time.elapsed().as_millis() as u64;

        match query_cursor_position() {
            Ok((x, y)) => {
                let pos = CursorPosition { x, y, timestamp_ms };
                positions.lock().unwrap_or_else(|e| e.into_inner()).push(pos);
            }
            Err(e) => {
                log::debug!("failed to query cursor position: {e}");
            }
        }
    }
}

/// Получает позицию курсора через hyprctl (Hyprland).
/// Формат вывода: "X, Y"
// TODO: Перейти на Hyprland IPC socket вместо fork/exec для производительности
fn query_cursor_position() -> Result<(f64, f64)> {
    let output = Command::new("hyprctl")
        .arg("cursorpos")
        .output()
        .context("failed to run hyprctl cursorpos")?;

    if !output.status.success() {
        anyhow::bail!(
            "hyprctl cursorpos failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let text = String::from_utf8_lossy(&output.stdout);
    parse_hyprctl_cursorpos(text.trim())
}

fn parse_hyprctl_cursorpos(s: &str) -> Result<(f64, f64)> {
    let (x_str, y_str) = s
        .split_once(',')
        .context("expected 'X, Y' format from hyprctl cursorpos")?;

    let x: f64 = x_str.trim().parse().context("failed to parse cursor X")?;
    let y: f64 = y_str.trim().parse().context("failed to parse cursor Y")?;
    Ok((x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_position_creation() {
        let pos = CursorPosition {
            x: 100.5,
            y: 200.0,
            timestamp_ms: 1234,
        };
        assert_eq!(pos.x, 100.5);
        assert_eq!(pos.y, 200.0);
        assert_eq!(pos.timestamp_ms, 1234);
    }

    #[test]
    fn test_cursor_telemetry_default_interval() {
        let telemetry = CursorTelemetry::default();
        assert_eq!(telemetry.poll_interval, Duration::from_millis(16));
        assert!(!telemetry.is_running());
    }

    #[test]
    fn test_positions_storage() {
        let mut telemetry = CursorTelemetry::new(16);
        // Добавляем позиции напрямую через внутренний mutex
        {
            let mut guard = telemetry.positions.lock().unwrap();
            guard.push(CursorPosition { x: 10.0, y: 20.0, timestamp_ms: 0 });
            guard.push(CursorPosition { x: 30.0, y: 40.0, timestamp_ms: 16 });
            guard.push(CursorPosition { x: 50.0, y: 60.0, timestamp_ms: 32 });
        }
        assert_eq!(telemetry.positions_count(), 3);
        // stop() забирает данные через take
        let taken = telemetry.stop();
        assert_eq!(taken.len(), 3);
        assert_eq!(taken[0].x, 10.0);
        assert_eq!(taken[2].timestamp_ms, 32);
        // После take данные пусты
        assert_eq!(telemetry.positions_count(), 0);
    }

    #[test]
    fn test_parse_hyprctl_cursorpos() {
        let (x, y) = parse_hyprctl_cursorpos("960, 540").unwrap();
        assert_eq!(x, 960.0);
        assert_eq!(y, 540.0);
    }

    #[test]
    fn test_parse_hyprctl_cursorpos_no_spaces() {
        let (x, y) = parse_hyprctl_cursorpos("1920,1080").unwrap();
        assert_eq!(x, 1920.0);
        assert_eq!(y, 1080.0);
    }

    #[test]
    fn test_parse_hyprctl_cursorpos_fractional() {
        let (x, y) = parse_hyprctl_cursorpos("960.5, 540.75").unwrap();
        assert_eq!(x, 960.5);
        assert_eq!(y, 540.75);
    }

    #[test]
    fn test_parse_hyprctl_cursorpos_invalid() {
        assert!(parse_hyprctl_cursorpos("invalid").is_err());
        assert!(parse_hyprctl_cursorpos("abc, def").is_err());
    }

    #[test]
    fn test_stop_without_start() {
        let mut telemetry = CursorTelemetry::new(16);
        let positions = telemetry.stop();
        assert!(positions.is_empty());
    }

    #[test]
    fn test_parse_negative_coordinates() {
        let (x, y) = parse_hyprctl_cursorpos("-100, -50").unwrap();
        assert_eq!(x, -100.0);
        assert_eq!(y, -50.0);
    }

    #[test]
    fn test_parse_large_coordinates() {
        let (x, y) = parse_hyprctl_cursorpos("7680, 4320").unwrap();
        assert_eq!(x, 7680.0);
        assert_eq!(y, 4320.0);
    }

    #[test]
    fn test_cursor_telemetry_custom_interval() {
        let telemetry = CursorTelemetry::new(32);
        assert_eq!(telemetry.poll_interval, Duration::from_millis(32));
        assert!(!telemetry.is_running());
        assert_eq!(telemetry.positions_count(), 0);
    }

    #[test]
    #[ignore]
    fn integration_start_stop() {
        // Требует Hyprland с hyprctl
        let mut telemetry = CursorTelemetry::new(50);
        telemetry.start().unwrap();
        assert!(telemetry.is_running());
        thread::sleep(Duration::from_millis(200));
        let positions = telemetry.stop();
        assert!(!telemetry.is_running());
        assert!(!positions.is_empty());
    }
}
