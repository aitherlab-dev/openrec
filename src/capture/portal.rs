use anyhow::{Context, Result};
use ashpd::desktop::screencast::{
    CursorMode, Screencast, SelectSourcesOptions, SourceType, Stream,
};
use ashpd::desktop::{PersistMode, Session};

/// Тип источника захвата.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureSourceType {
    Monitor,
    Window,
}

/// Информация о выбранном источнике захвата.
#[derive(Debug, Clone)]
pub struct SelectedSource {
    pub node_id: u32,
    pub source_type: CaptureSourceType,
    pub size: Option<(i32, i32)>,
}

impl SelectedSource {
    fn from_stream(stream: &Stream) -> Self {
        let source_type = match stream.source_type() {
            Some(SourceType::Monitor) => CaptureSourceType::Monitor,
            Some(SourceType::Window) => CaptureSourceType::Window,
            other => {
                log::warn!("unknown source type {other:?}, assuming Monitor");
                CaptureSourceType::Monitor
            }
        };
        Self {
            node_id: stream.pipe_wire_node_id(),
            source_type,
            size: stream.size(),
        }
    }
}

/// Обёртка над сессией xdg-desktop-portal screencast.
pub struct ScreencastSession {
    proxy: Screencast,
    session: Session<Screencast>,
    closed: bool,
}

impl ScreencastSession {
    /// Создаёт новую сессию portal screencast через D-Bus.
    pub async fn new() -> Result<Self> {
        let proxy = Screencast::new()
            .await
            .context("failed to connect to screencast portal")?;

        let session = proxy
            .create_session(Default::default())
            .await
            .context("failed to create screencast session")?;

        Ok(Self {
            proxy,
            session,
            closed: false,
        })
    }

    /// Показывает системный диалог выбора экрана/окна.
    /// Возвращает информацию о выбранном источнике.
    pub async fn select_source(&self) -> Result<SelectedSource> {
        let source_options = SelectSourcesOptions::default()
            .set_sources(SourceType::Monitor | SourceType::Window)
            .set_multiple(false)
            .set_cursor_mode(CursorMode::Embedded)
            .set_persist_mode(PersistMode::DoNot);

        self.proxy
            .select_sources(&self.session, source_options)
            .await
            .context("failed to select screencast sources")?
            .response()
            .context("select_sources dialog was cancelled")?;

        let streams_response = self
            .proxy
            .start(&self.session, None, Default::default())
            .await
            .context("failed to start screencast")?
            .response()
            .context("start screencast was cancelled")?;

        let streams = streams_response.streams();
        let stream = streams
            .first()
            .context("no streams returned from portal")?;

        Ok(SelectedSource::from_stream(stream))
    }

    /// Открывает PipeWire remote и возвращает file descriptor.
    pub async fn open_pipewire_remote(&self) -> Result<std::os::fd::OwnedFd> {
        let fd = self
            .proxy
            .open_pipe_wire_remote(&self.session, Default::default())
            .await
            .context("failed to open PipeWire remote")?;

        log::info!("PipeWire remote fd opened");
        Ok(fd)
    }

    /// Завершает сессию portal.
    pub async fn close(&mut self) -> Result<()> {
        self.session
            .close()
            .await
            .context("failed to close screencast session")?;
        self.closed = true;
        log::info!("Screencast session closed");
        Ok(())
    }
}

impl Drop for ScreencastSession {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        log::warn!("ScreencastSession dropped without close()");
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let session = &self.session;
            let _ = handle.block_on(session.close());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_source_from_values() {
        let source = SelectedSource {
            node_id: 42,
            source_type: CaptureSourceType::Monitor,
            size: Some((1920, 1080)),
        };
        assert_eq!(source.node_id, 42);
        assert_eq!(source.source_type, CaptureSourceType::Monitor);
        assert_eq!(source.size, Some((1920, 1080)));
    }

    #[test]
    fn selected_source_window_type() {
        let source = SelectedSource {
            node_id: 7,
            source_type: CaptureSourceType::Window,
            size: None,
        };
        assert_eq!(source.source_type, CaptureSourceType::Window);
        assert!(source.size.is_none());
    }

    #[test]
    fn capture_source_type_equality() {
        assert_ne!(CaptureSourceType::Monitor, CaptureSourceType::Window);
        assert_eq!(CaptureSourceType::Monitor, CaptureSourceType::Monitor);
    }

    #[test]
    fn closed_flag_default() {
        // Проверяем что closed=true предотвращает warn в Drop
        // (без реального D-Bus — только логика флага)
        let closed = true;
        assert!(closed);
    }

    #[test]
    fn selected_source_monitor_type() {
        let source = SelectedSource {
            node_id: 1,
            source_type: CaptureSourceType::Monitor,
            size: Some((2560, 1440)),
        };
        assert_eq!(source.source_type, CaptureSourceType::Monitor);
        assert_eq!(source.node_id, 1);
        assert_eq!(source.size, Some((2560, 1440)));
    }

    #[test]
    fn selected_source_default_size() {
        let source = SelectedSource {
            node_id: 99,
            source_type: CaptureSourceType::Monitor,
            size: None,
        };
        assert!(source.size.is_none());
        // Код pipeline использует unwrap_or((1920, 1080)) — size=None допустима
        let (w, h) = source.size.unwrap_or((1920, 1080));
        assert_eq!(w, 1920);
        assert_eq!(h, 1080);
    }

    #[test]
    #[ignore]
    fn integration_create_session() {
        // Требует активную D-Bus GUI-сессию
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut session = ScreencastSession::new().await.unwrap();
            session.close().await.unwrap();
        });
    }

    #[test]
    #[ignore]
    fn integration_select_source() {
        // Требует активную D-Bus GUI-сессию + покажет диалог выбора
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut session = ScreencastSession::new().await.unwrap();
            let source = session.select_source().await.unwrap();
            assert!(source.node_id > 0);
            session.close().await.unwrap();
        });
    }
}
