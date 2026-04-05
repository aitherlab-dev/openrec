use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use anyhow::{Context, Result};
use crossbeam_channel::Sender;
use pipewire as pw;
use pw::spa;
use pw::spa::pod::Pod;
use pw::stream::{StreamFlags, StreamState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::upper_case_acronyms)]
pub enum PixelFormat {
    BGRA,
    RGBA,
    BGRx,
}

impl PixelFormat {
    fn from_spa(raw: spa::param::video::VideoFormat) -> Option<Self> {
        if raw == spa::param::video::VideoFormat::BGRA {
            Some(Self::BGRA)
        } else if raw == spa::param::video::VideoFormat::RGBA {
            Some(Self::RGBA)
        } else if raw == spa::param::video::VideoFormat::BGRx {
            Some(Self::BGRx)
        } else {
            None
        }
    }

    pub fn bytes_per_pixel(self) -> u32 {
        4
    }
}

#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: PixelFormat,
    pub data: Vec<u8>,
    pub timestamp_ms: u64,
}

impl VideoFrame {
    pub fn expected_size(&self) -> usize {
        self.stride as usize * self.height as usize
    }
}

struct CaptureState {
    format: spa::param::video::VideoInfoRaw,
    frame_sender: Sender<VideoFrame>,
}

struct QuitSignal;

pub struct PipeWireCapture {
    node_id: u32,
    fps: u32,
    running: Arc<AtomicBool>,
    quit_sender: Option<pw::channel::Sender<QuitSignal>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl PipeWireCapture {
    // TODO: вынести pw::init() в main() при интеграции с приложением
    pub fn new(node_id: u32, fps: u32) -> Result<Self> {
        pw::init();
        Ok(Self {
            node_id,
            fps,
            running: Arc::new(AtomicBool::new(false)),
            quit_sender: None,
            thread: None,
        })
    }

    pub fn start(&mut self, frame_sender: Sender<VideoFrame>) -> Result<()> {
        anyhow::ensure!(
            !self.running.load(Ordering::SeqCst),
            "Capture already running"
        );

        self.running.store(true, Ordering::SeqCst);
        let running = self.running.clone();
        let node_id = self.node_id;
        let fps = self.fps;

        let (quit_tx, quit_rx) = pw::channel::channel::<QuitSignal>();
        self.quit_sender = Some(quit_tx);

        let handle = thread::spawn(move || {
            if let Err(e) = run_pipewire_loop(node_id, fps, frame_sender, running.clone(), quit_rx)
            {
                log::error!("PipeWire capture error: {e:#}");
            }
            running.store(false, Ordering::SeqCst);
        });

        self.thread = Some(handle);
        Ok(())
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(sender) = self.quit_sender.take() {
            let _ = sender.send(QuitSignal);
        }
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl Drop for PipeWireCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_pipewire_loop(
    node_id: u32,
    fps: u32,
    frame_sender: Sender<VideoFrame>,
    running: Arc<AtomicBool>,
    quit_rx: pw::channel::Receiver<QuitSignal>,
) -> Result<()> {
    let mainloop =
        pw::main_loop::MainLoopRc::new(None).context("Failed to create PipeWire main loop")?;
    let context = pw::context::ContextRc::new(&mainloop, None)
        .context("Failed to create PipeWire context")?;
    let core = context
        .connect_rc(None)
        .context("Failed to connect to PipeWire")?;

    // Attach quit channel to mainloop — allows stop() from another thread
    let mainloop_for_quit = mainloop.clone();
    let _quit_receiver = quit_rx.attach(mainloop.loop_(), move |_| {
        log::info!("Quit signal received, stopping PipeWire loop");
        mainloop_for_quit.quit();
    });

    let stream = pw::stream::StreamBox::new(
        &core,
        "openrec-screen-capture",
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .context("Failed to create PipeWire stream")?;

    let state = CaptureState {
        format: Default::default(),
        frame_sender,
    };

    let mainloop_quit = mainloop.clone();
    let running_cb = running.clone();

    let _listener = stream
        .add_local_listener_with_user_data(state)
        .state_changed(move |_stream, _data, old, new| {
            log::debug!("PipeWire stream state: {old:?} -> {new:?}");
            match new {
                StreamState::Error(_) => {
                    running_cb.store(false, Ordering::SeqCst);
                    mainloop_quit.quit();
                }
                StreamState::Unconnected => {
                    mainloop_quit.quit();
                }
                _ => {}
            }
        })
        .param_changed(|_stream, data, id, param| {
            let Some(param) = param else { return };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }

            let (media_type, media_subtype) = match spa::param::format_utils::parse_format(param) {
                Ok(v) => v,
                Err(_) => return,
            };

            if media_type != spa::param::format::MediaType::Video
                || media_subtype != spa::param::format::MediaSubtype::Raw
            {
                return;
            }

            if let Err(e) = data.format.parse(param) {
                log::error!("Failed to parse video format: {e}");
                return;
            }

            let size = data.format.size();
            let fmt = data.format.format();
            log::info!(
                "Negotiated format: {:?} {}x{} @ {}/{}fps",
                fmt,
                size.width,
                size.height,
                data.format.framerate().num,
                data.format.framerate().denom,
            );
        })
        .process(|stream, data| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                log::trace!("No buffer available");
                return;
            };

            let datas = buffer.datas_mut();
            if datas.is_empty() {
                return;
            }

            let d = &mut datas[0];
            let chunk = d.chunk();
            let chunk_size = chunk.size() as usize;
            let chunk_offset = chunk.offset() as usize;
            let stride = chunk.stride() as u32;

            if chunk_size == 0 {
                return;
            }

            let Some(mapped) = d.data() else {
                return;
            };

            if chunk_offset + chunk_size > mapped.len() {
                return;
            }

            let pixel_data = &mapped[chunk_offset..chunk_offset + chunk_size];

            let size = data.format.size();
            let spa_fmt = data.format.format();
            let Some(format) = PixelFormat::from_spa(spa_fmt) else {
                log::warn!("Unsupported pixel format: {spa_fmt:?}, skipping frame");
                return;
            };

            let timestamp_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);

            let frame = VideoFrame {
                width: size.width,
                height: size.height,
                stride: if stride > 0 {
                    stride
                } else {
                    size.width * format.bytes_per_pixel()
                },
                format,
                data: pixel_data.to_vec(),
                timestamp_ms,
            };

            if data.frame_sender.try_send(frame).is_err() {
                log::debug!("Frame dropped: channel full");
            }
        })
        .register()
        .context("Failed to register stream listener")?;

    // Build format params: prefer BGRx, also accept BGRA/RGBA
    let obj = pw::spa::pod::object!(
        pw::spa::utils::SpaTypes::ObjectParamFormat,
        pw::spa::param::ParamType::EnumFormat,
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::MediaType,
            Id,
            pw::spa::param::format::MediaType::Video
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::MediaSubtype,
            Id,
            pw::spa::param::format::MediaSubtype::Raw
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            pw::spa::param::video::VideoFormat::BGRx,
            pw::spa::param::video::VideoFormat::BGRx,
            pw::spa::param::video::VideoFormat::BGRA,
            pw::spa::param::video::VideoFormat::RGBA
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            pw::spa::utils::Rectangle {
                width: 1920,
                height: 1080
            },
            pw::spa::utils::Rectangle {
                width: 1,
                height: 1
            },
            pw::spa::utils::Rectangle {
                width: 4096,
                height: 4096
            }
        ),
        pw::spa::pod::property!(
            pw::spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            pw::spa::utils::Fraction { num: fps, denom: 1 },
            pw::spa::utils::Fraction { num: 0, denom: 1 },
            pw::spa::utils::Fraction {
                num: 1000,
                denom: 1
            }
        ),
    );

    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .context("Failed to serialize stream params")?
    .0
    .into_inner();

    let mut params = [Pod::from_bytes(&values).unwrap()];

    stream.connect(
        spa::utils::Direction::Input,
        Some(node_id),
        StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS,
        &mut params,
    )
    .context("Failed to connect PipeWire stream")?;

    log::info!("PipeWire stream connected to node {node_id}");

    mainloop.run();

    let _ = stream.disconnect();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_frame_expected_size() {
        let frame = VideoFrame {
            width: 1920,
            height: 1080,
            stride: 1920 * 4,
            format: PixelFormat::BGRx,
            data: vec![0u8; 1920 * 4 * 1080],
            timestamp_ms: 12345,
        };
        assert_eq!(frame.expected_size(), 1920 * 4 * 1080);
        assert_eq!(frame.data.len(), frame.expected_size());
    }

    #[test]
    fn video_frame_with_padding_stride() {
        let stride = 1920 * 4 + 64;
        let frame = VideoFrame {
            width: 1920,
            height: 1080,
            stride,
            format: PixelFormat::BGRA,
            data: vec![0u8; stride as usize * 1080],
            timestamp_ms: 0,
        };
        assert_eq!(frame.expected_size(), stride as usize * 1080);
    }

    #[test]
    fn pixel_format_bytes_per_pixel() {
        assert_eq!(PixelFormat::BGRA.bytes_per_pixel(), 4);
        assert_eq!(PixelFormat::RGBA.bytes_per_pixel(), 4);
        assert_eq!(PixelFormat::BGRx.bytes_per_pixel(), 4);
    }

    #[test]
    fn pixel_format_from_spa() {
        assert_eq!(
            PixelFormat::from_spa(spa::param::video::VideoFormat::BGRx),
            Some(PixelFormat::BGRx)
        );
        assert_eq!(
            PixelFormat::from_spa(spa::param::video::VideoFormat::BGRA),
            Some(PixelFormat::BGRA)
        );
        assert_eq!(
            PixelFormat::from_spa(spa::param::video::VideoFormat::RGBA),
            Some(PixelFormat::RGBA)
        );
        assert_eq!(
            PixelFormat::from_spa(spa::param::video::VideoFormat::RGB),
            None
        );
    }

    #[test]
    fn video_frame_data_integrity() {
        let width: u32 = 640;
        let height: u32 = 480;
        let stride = width * 4;
        let data = vec![0xABu8; stride as usize * height as usize];

        let frame = VideoFrame {
            width,
            height,
            stride,
            format: PixelFormat::RGBA,
            data: data.clone(),
            timestamp_ms: 999,
        };

        assert_eq!(frame.data.len(), (stride * height) as usize);
        assert_eq!(frame.data.len(), frame.expected_size());
        assert!(frame.data.iter().all(|&b| b == 0xAB));
    }

    #[test]
    fn pixel_format_debug() {
        // All variants produce non-empty Debug output
        let bgra = format!("{:?}", PixelFormat::BGRA);
        let rgba = format!("{:?}", PixelFormat::RGBA);
        let bgrx = format!("{:?}", PixelFormat::BGRx);

        assert!(bgra.contains("BGRA"), "got: {bgra}");
        assert!(rgba.contains("RGBA"), "got: {rgba}");
        assert!(bgrx.contains("BGRx"), "got: {bgrx}");
    }

    #[test]
    fn video_frame_timestamp() {
        let ts = 1_700_000_000_000u64; // realistic millisecond timestamp
        let frame = VideoFrame {
            width: 100,
            height: 100,
            stride: 400,
            format: PixelFormat::BGRx,
            data: vec![0u8; 400 * 100],
            timestamp_ms: ts,
        };
        assert_eq!(frame.timestamp_ms, ts);

        // Zero timestamp is also valid (e.g. relative timing)
        let frame_zero = VideoFrame {
            width: 1,
            height: 1,
            stride: 4,
            format: PixelFormat::BGRA,
            data: vec![0u8; 4],
            timestamp_ms: 0,
        };
        assert_eq!(frame_zero.timestamp_ms, 0);
    }

    #[test]
    #[ignore] // requires running PipeWire daemon
    fn capture_create_and_stop() {
        pw::init();
        let capture = PipeWireCapture::new(0, 30);
        assert!(capture.is_ok());
        let mut capture = capture.unwrap();
        assert!(!capture.is_running());
        capture.stop();
    }
}
