use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::capture::cursor::CursorPosition;
use crate::editor::preview::extract_frame;
use crate::export::ffmpeg::{Codec, EncoderConfig, FfmpegEncoder};
use crate::export::gif::{GifConfig, GifEncoder};
use crate::project::persistence::{Annotation, Project, TrimSegment, ZoomRegion};

/// Параметры zoom-трансформации для одного кадра.
#[derive(Debug, Clone)]
pub struct ZoomTransform {
    pub scale: f32,
    pub focus_x: f32,
    pub focus_y: f32,
}

impl ZoomTransform {
    pub fn identity() -> Self {
        Self {
            scale: 1.0,
            focus_x: 0.5,
            focus_y: 0.5,
        }
    }
}

pub struct ExportRenderer {
    video_path: PathBuf,
    width: u32,
    height: u32,
    fps: u32,
    trim_segments: Vec<TrimSegment>,
    zoom_regions: Vec<ZoomRegion>,
    annotations: Vec<Annotation>,
    cursor_data: Vec<CursorPosition>,
    duration_ms: u64,
}

impl ExportRenderer {
    pub fn new(project: &Project, width: u32, height: u32, fps: u32) -> Self {
        Self {
            video_path: project.source_video.clone(),
            width,
            height,
            fps,
            trim_segments: project.trim_segments.clone(),
            zoom_regions: project.zoom_regions.clone(),
            annotations: project.annotations.clone(),
            cursor_data: project.cursor_data.clone(),
            duration_ms: project.duration_ms,
        }
    }

    pub fn render_to_mp4(&self, output: &Path) -> Result<()> {
        let config = EncoderConfig {
            output_path: output.to_path_buf(),
            width: self.width,
            height: self.height,
            fps: self.fps,
            codec: Codec::H264,
            bitrate: None,
            pixel_format: "rgba".to_string(),
        };

        let mut encoder = FfmpegEncoder::new(config)
            .context("failed to create MP4 encoder")?;

        self.render_frames(|frame_data| encoder.write_frame(frame_data))?;

        encoder.finish().context("failed to finalize MP4")?;
        Ok(())
    }

    pub fn render_to_gif(&self, output: &Path, gif_fps: u32) -> Result<()> {
        let config = GifConfig {
            output_path: output.to_path_buf(),
            width: self.width,
            height: self.height,
            fps: gif_fps,
            pixel_format: "rgba".to_string(),
            ..Default::default()
        };

        let mut encoder = GifEncoder::new(config)
            .context("failed to create GIF encoder")?;

        let frame_interval_ms = 1000 / gif_fps as u64;
        let mut position_ms = 0u64;

        while position_ms < self.duration_ms {
            if !should_skip_frame(position_ms, &self.trim_segments) {
                let frame = extract_frame(
                    &self.video_path,
                    position_ms,
                    self.width,
                    self.height,
                )
                .with_context(|| format!("failed to extract frame at {position_ms}ms"))?;

                let transform = self.zoom_at(position_ms);
                let output_data = apply_zoom_transform(
                    &frame.data,
                    self.width,
                    self.height,
                    &transform,
                );

                encoder.write_frame(&output_data)?;
            }
            position_ms += frame_interval_ms;
        }

        encoder.finish().context("failed to finalize GIF")?;
        Ok(())
    }

    fn render_frames(&self, mut write_fn: impl FnMut(&[u8]) -> Result<()>) -> Result<()> {
        let frame_interval_ms = 1000 / self.fps as u64;
        let mut position_ms = 0u64;

        while position_ms < self.duration_ms {
            if !should_skip_frame(position_ms, &self.trim_segments) {
                let frame = extract_frame(
                    &self.video_path,
                    position_ms,
                    self.width,
                    self.height,
                )
                .with_context(|| format!("failed to extract frame at {position_ms}ms"))?;

                let transform = self.zoom_at(position_ms);
                let output_data = apply_zoom_transform(
                    &frame.data,
                    self.width,
                    self.height,
                    &transform,
                );

                write_fn(&output_data)?;
            }
            position_ms += frame_interval_ms;
        }

        Ok(())
    }

    fn zoom_at(&self, position_ms: u64) -> ZoomTransform {
        for region in &self.zoom_regions {
            if position_ms >= region.start_ms && position_ms < region.end_ms {
                return ZoomTransform {
                    scale: region.level,
                    focus_x: region.focus_x,
                    focus_y: region.focus_y,
                };
            }
        }
        ZoomTransform::identity()
    }
}

/// Проверяет, попадает ли кадр в обрезанный сегмент.
pub fn should_skip_frame(position_ms: u64, trim_segments: &[TrimSegment]) -> bool {
    trim_segments
        .iter()
        .any(|seg| position_ms >= seg.start_ms && position_ms < seg.end_ms)
}

/// CPU-based zoom: crop вокруг фокусной точки, nearest-neighbor upscale.
/// scale == 1 → passthrough, scale > 1 → увеличение (обрезка + масштаб).
pub fn apply_zoom_transform(
    frame: &[u8],
    width: u32,
    height: u32,
    transform: &ZoomTransform,
) -> Vec<u8> {
    if (transform.scale - 1.0).abs() < f32::EPSILON {
        return frame.to_vec();
    }

    let scale = transform.scale.max(1.0);
    let src_w = (width as f32 / scale) as u32;
    let src_h = (height as f32 / scale) as u32;

    // Вычисляем crop origin, clamp к границам
    let max_x = width.saturating_sub(src_w);
    let max_y = height.saturating_sub(src_h);
    let src_x = ((transform.focus_x * width as f32 - src_w as f32 / 2.0) as u32).min(max_x);
    let src_y = ((transform.focus_y * height as f32 - src_h as f32 / 2.0) as u32).min(max_y);

    let mut output = vec![0u8; (width * height * 4) as usize];

    // Nearest-neighbor upscale
    for dst_y in 0..height {
        for dst_x in 0..width {
            let sample_x = src_x + (dst_x * src_w / width);
            let sample_y = src_y + (dst_y * src_h / height);

            let src_idx = ((sample_y * width + sample_x) * 4) as usize;
            let dst_idx = ((dst_y * width + dst_x) * 4) as usize;

            if src_idx + 3 < frame.len() && dst_idx + 3 < output.len() {
                output[dst_idx..dst_idx + 4].copy_from_slice(&frame[src_idx..src_idx + 4]);
            }
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_skip_frame() {
        let trims = vec![
            TrimSegment {
                start_ms: 0,
                end_ms: 500,
            },
            TrimSegment {
                start_ms: 2000,
                end_ms: 3000,
            },
        ];

        // Внутри trim — пропускаем
        assert!(should_skip_frame(0, &trims));
        assert!(should_skip_frame(250, &trims));
        assert!(should_skip_frame(499, &trims));
        assert!(should_skip_frame(2000, &trims));
        assert!(should_skip_frame(2500, &trims));

        // Вне trim — не пропускаем
        assert!(!should_skip_frame(500, &trims));
        assert!(!should_skip_frame(1000, &trims));
        assert!(!should_skip_frame(3000, &trims));
        assert!(!should_skip_frame(5000, &trims));
    }

    #[test]
    fn test_should_skip_frame_no_trims() {
        let trims: Vec<TrimSegment> = vec![];
        assert!(!should_skip_frame(0, &trims));
        assert!(!should_skip_frame(1000, &trims));
        assert!(!should_skip_frame(999999, &trims));
    }

    #[test]
    fn test_apply_zoom_identity() {
        let w = 4u32;
        let h = 4u32;
        let frame: Vec<u8> = (0..w * h * 4).map(|i| (i % 256) as u8).collect();
        let transform = ZoomTransform::identity();

        let result = apply_zoom_transform(&frame, w, h, &transform);
        assert_eq!(result, frame);
    }

    #[test]
    fn test_apply_zoom_scale() {
        let w = 8u32;
        let h = 8u32;
        // Создаём кадр где каждый пиксель уникален
        let mut frame = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let idx = ((y * w + x) * 4) as usize;
                frame[idx] = x as u8;     // R
                frame[idx + 1] = y as u8; // G
                frame[idx + 2] = 0;       // B
                frame[idx + 3] = 255;     // A
            }
        }

        let transform = ZoomTransform {
            scale: 2.0,
            focus_x: 0.5,
            focus_y: 0.5,
        };

        let result = apply_zoom_transform(&frame, w, h, &transform);
        assert_eq!(result.len(), frame.len());
        // При zoom 2x результат НЕ должен совпадать с оригиналом
        assert_ne!(result, frame);
    }

    #[test]
    fn test_export_renderer_new() {
        let project = Project::new("Test", PathBuf::from("/tmp/test.mp4"), 5000);
        let renderer = ExportRenderer::new(&project, 1920, 1080, 60);

        assert_eq!(renderer.width, 1920);
        assert_eq!(renderer.height, 1080);
        assert_eq!(renderer.fps, 60);
        assert_eq!(renderer.duration_ms, 5000);
        assert!(renderer.trim_segments.is_empty());
        assert!(renderer.zoom_regions.is_empty());
    }

    #[test]
    fn test_zoom_transform_identity() {
        let t = ZoomTransform::identity();
        assert_eq!(t.scale, 1.0);
        assert_eq!(t.focus_x, 0.5);
        assert_eq!(t.focus_y, 0.5);
    }

    #[test]
    #[ignore]
    fn test_render_to_mp4() {
        // Требует реальный видеофайл
        let path = Path::new("/tmp/openrec_test.mp4");
        if !path.exists() {
            return;
        }

        let mut project = Project::new("Render Test", path.to_path_buf(), 2000);
        project.trim_segments.push(TrimSegment {
            start_ms: 0,
            end_ms: 200,
        });

        let renderer = ExportRenderer::new(&project, 320, 240, 10);
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("rendered.mp4");

        renderer.render_to_mp4(&output).unwrap();
        assert!(output.exists());
        assert!(std::fs::metadata(&output).unwrap().len() > 0);
    }
}
