use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use anyhow::{Context, Result};
use crossbeam_channel::Sender;
use pipewire as pw;
use pw::spa;
use pw::stream::StreamFlags;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioSource {
    Microphone,
    SystemAudio,
}

#[derive(Debug, Clone)]
pub struct AudioConfig {
    pub source: AudioSource,
    pub device_name: Option<String>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            source: AudioSource::Microphone,
            device_name: None,
            sample_rate: 48000,
            channels: 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AudioFrame {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone)]
pub struct AudioDevice {
    pub name: String,
    pub description: String,
    pub source_type: AudioSource,
}

struct CaptureUserData {
    format: spa::param::audio::AudioInfoRaw,
    sender: Sender<AudioFrame>,
    sample_rate: u32,
    channels: u16,
    start_time: Instant,
}

pub struct AudioCapture {
    config: AudioConfig,
    running: Arc<AtomicBool>,
    quit_sender: Option<pw::channel::Sender<()>>,
    thread_handle: Option<thread::JoinHandle<()>>,
}

impl AudioCapture {
    pub fn new(config: AudioConfig) -> Result<Self> {
        Ok(Self {
            config,
            running: Arc::new(AtomicBool::new(false)),
            quit_sender: None,
            thread_handle: None,
        })
    }

    pub fn start(&mut self, sender: Sender<AudioFrame>) -> Result<()> {
        if self.running.load(Ordering::Relaxed) {
            anyhow::bail!("audio capture already running");
        }

        let config = self.config.clone();
        let running = self.running.clone();
        let (quit_tx, quit_rx) = pw::channel::channel::<()>();

        self.quit_sender = Some(quit_tx);
        self.running.store(true, Ordering::Relaxed);

        let handle = thread::Builder::new()
            .name("audio-capture".into())
            .spawn(move || {
                if let Err(e) = run_capture_loop(config, sender, quit_rx, running.clone()) {
                    log::error!("audio capture error: {e:#}");
                }
                running.store(false, Ordering::Relaxed);
            })
            .context("failed to spawn audio capture thread")?;

        self.thread_handle = Some(handle);
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.quit_sender.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
        self.running.store(false, Ordering::Relaxed);
    }

    pub fn list_devices() -> Result<Vec<AudioDevice>> {
        // PipeWire device enumeration requires a running loop and registry.
        // For now return an empty list — full enumeration will be added
        // when the registry listener infrastructure is in place.
        Ok(Vec::new())
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_capture_loop(
    config: AudioConfig,
    sender: Sender<AudioFrame>,
    quit_rx: pw::channel::Receiver<()>,
    _running: Arc<AtomicBool>,
) -> Result<()> {
    pw::init();

    let mainloop = pw::main_loop::MainLoopRc::new(None)
        .context("failed to create PipeWire main loop")?;

    let context = pw::context::ContextRc::new(&mainloop, None)
        .context("failed to create PipeWire context")?;

    let core = context
        .connect_rc(None)
        .context("failed to connect to PipeWire")?;

    let props = build_stream_properties(&config);

    let stream = pw::stream::StreamBox::new(&core, "openrec-audio", props)
        .context("failed to create audio stream")?;

    let user_data = CaptureUserData {
        format: spa::param::audio::AudioInfoRaw::default(),
        sender,
        sample_rate: config.sample_rate,
        channels: config.channels,
        start_time: Instant::now(),
    };

    let _listener = stream
        .add_local_listener_with_user_data(user_data)
        .state_changed(|_stream, _data, old, new| {
            log::debug!("audio stream state: {old:?} → {new:?}");
        })
        .param_changed(|_stream, data, id, param| {
            if let Some(param) = param {
                if id == spa::param::ParamType::Format.as_raw()
                    && data.format.parse(param).is_ok()
                {
                    data.sample_rate = data.format.rate();
                    data.channels = data.format.channels() as u16;
                    log::info!(
                        "audio format negotiated: {}Hz, {}ch",
                        data.sample_rate,
                        data.channels
                    );
                }
            }
        })
        .process(|stream: &pw::stream::Stream, data| {
            if let Some(mut buffer) = stream.dequeue_buffer() {
                let datas = buffer.datas_mut();
                if let Some(d) = datas.first_mut() {
                    let offset = d.chunk().offset() as usize;
                    let size = d.chunk().size() as usize;

                    if let Some(slice) = d.data() {
                        if offset + size <= slice.len() {
                            let audio_bytes = &slice[offset..offset + size];
                            let samples: Vec<f32> = audio_bytes
                                .chunks_exact(4)
                                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                                .collect();

                            let frame = AudioFrame {
                                samples,
                                sample_rate: data.sample_rate,
                                channels: data.channels,
                                timestamp_ms: data.start_time.elapsed().as_millis() as u64,
                            };

                            let _ = data.sender.try_send(frame);
                        }
                    }
                }
            }
        })
        .register()
        .context("failed to register stream listener")?;

    let pod = build_audio_format_pod(&config)?;
    let mut params = [spa::pod::Pod::from_bytes(&pod)
        .context("failed to create audio format pod")?];

    stream
        .connect(
            spa::utils::Direction::Input,
            None,
            StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS,
            &mut params,
        )
        .context("failed to connect audio stream")?;

    let _quit = quit_rx.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        move |_| mainloop.quit()
    });

    mainloop.run();

    stream.disconnect().ok();

    Ok(())
}

fn build_stream_properties(config: &AudioConfig) -> pw::properties::PropertiesBox {
    let mut props = pw::properties::properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Screen",
    };

    if config.source == AudioSource::SystemAudio {
        props.insert(*pw::keys::STREAM_CAPTURE_SINK, "true");
    }

    if let Some(ref device) = config.device_name {
        props.insert("target.object", device.as_str());
    }

    props
}

fn build_audio_format_pod(config: &AudioConfig) -> Result<Vec<u8>> {
    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
    audio_info.set_rate(config.sample_rate);
    audio_info.set_channels(config.channels as u32);

    let obj = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };

    let values: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .context("failed to serialize audio format")?
    .0
    .into_inner();

    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_config_default() {
        let config = AudioConfig::default();
        assert_eq!(config.source, AudioSource::Microphone);
        assert!(config.device_name.is_none());
        assert_eq!(config.sample_rate, 48000);
        assert_eq!(config.channels, 2);
    }

    #[test]
    fn test_audio_frame_creation() {
        let frame = AudioFrame {
            samples: vec![0.0, 0.5, -0.5, 1.0],
            sample_rate: 44100,
            channels: 1,
            timestamp_ms: 500,
        };
        assert_eq!(frame.samples.len(), 4);
        assert_eq!(frame.sample_rate, 44100);
        assert_eq!(frame.channels, 1);
        assert_eq!(frame.timestamp_ms, 500);
    }

    #[test]
    fn test_audio_source_variants() {
        let mic = AudioSource::Microphone;
        let sys = AudioSource::SystemAudio;
        assert_ne!(mic, sys);
        assert_eq!(mic, AudioSource::Microphone);
        assert_eq!(sys, AudioSource::SystemAudio);
    }

    #[test]
    fn test_audio_device_struct() {
        let device = AudioDevice {
            name: "alsa_input.pci-0000_00_1f.3.analog-stereo".into(),
            description: "Built-in Audio Analog Stereo".into(),
            source_type: AudioSource::Microphone,
        };
        assert_eq!(device.source_type, AudioSource::Microphone);
        assert!(!device.name.is_empty());
        assert!(!device.description.is_empty());
    }

    #[test]
    fn test_audio_capture_new() {
        let config = AudioConfig::default();
        let capture = AudioCapture::new(config);
        assert!(capture.is_ok());
    }

    #[test]
    fn test_list_devices() {
        let devices = AudioCapture::list_devices();
        assert!(devices.is_ok());
    }

    #[test]
    #[ignore]
    fn integration_start_stop_capture() {
        // Требует работающий PipeWire daemon
        let config = AudioConfig::default();
        let mut capture = AudioCapture::new(config).unwrap();
        let (tx, rx) = crossbeam_channel::bounded(64);

        capture.start(tx).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Проверяем что хотя бы что-то пришло (если есть микрофон)
        let count = rx.try_iter().count();
        log::info!("received {count} audio frames");

        capture.stop();
    }

    #[test]
    #[ignore]
    fn integration_system_audio_capture() {
        // Требует работающий PipeWire с активным аудио-выходом
        let config = AudioConfig {
            source: AudioSource::SystemAudio,
            ..Default::default()
        };
        let mut capture = AudioCapture::new(config).unwrap();
        let (tx, _rx) = crossbeam_channel::bounded(64);

        capture.start(tx).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
        capture.stop();
    }
}
