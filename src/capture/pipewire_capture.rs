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

pub struct PipeWireCapture {
    node_id: u32,
    fps: u32,
    running: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl PipeWireCapture {
    pub fn new(node_id: u32, fps: u32) -> Result<Self> {
        pw::init();
        Ok(Self {
            node_id,
            fps,
            running: Arc::new(AtomicBool::new(false)),
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

        let handle = thread::spawn(move || {
            if let Err(e) = run_pipewire_loop(node_id, fps, frame_sender, running.clone()) {
                log::error!("PipeWire capture error: {e:#}");
            }
            running.store(false, Ordering::SeqCst);
        });

        self.thread = Some(handle);
        Ok(())
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
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
) -> Result<()> {
    let mainloop =
        pw::main_loop::MainLoopRc::new(None).context("Failed to create PipeWire main loop")?;
    let context = pw::context::ContextRc::new(&mainloop, None)
        .context("Failed to create PipeWire context")?;
    let core = context
        .connect_rc(None)
        .context("Failed to connect to PipeWire")?;

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
            let format = PixelFormat::from_spa(spa_fmt).unwrap_or(PixelFormat::BGRx);

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

            let _ = data.frame_sender.try_send(frame);
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
