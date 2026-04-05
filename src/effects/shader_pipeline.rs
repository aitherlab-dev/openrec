use std::path::Path;

use anyhow::{Context, Result};

/// Пути к WGSL шейдерам относительно корня проекта.
pub const ZOOM_SHADER: &str = "shaders/zoom.wgsl";
pub const BLUR_SHADER: &str = "shaders/blur.wgsl";
pub const SHADOW_SHADER: &str = "shaders/shadow.wgsl";
pub const MOTION_BLUR_SHADER: &str = "shaders/motion_blur.wgsl";

/// Параметры zoom-эффекта для GPU.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ZoomParams {
    pub scale: f32,
    pub translate_x: f32,
    pub translate_y: f32,
    pub _padding: f32,
}

/// Параметры blur-эффекта для GPU.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct BlurParams {
    pub direction: [f32; 2],
    pub texel_size: [f32; 2],
    pub radius: f32,
    pub _padding: [f32; 3],
}

/// Параметры shadow-эффекта для GPU.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ShadowParams {
    pub offset: [f32; 2],
    pub texel_size: [f32; 2],
    pub blur_radius: f32,
    pub shadow_color: [f32; 3],
}

/// Параметры motion blur для GPU.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct MotionBlurParams {
    pub direction: [f32; 2],
    pub texel_size: [f32; 2],
    pub strength: f32,
    pub _padding: [f32; 3],
}

/// GPU pipeline для эффектов.
/// Загружает и компилирует WGSL шейдеры, создаёт render pipelines.
pub struct ShaderPipeline {
    // TODO: wgpu device, queue, render pipelines
    _placeholder: (),
}

impl ShaderPipeline {
    /// Создаёт pipeline, компилируя все шейдеры.
    pub fn new(_device: &wgpu::Device) -> Result<Self> {
        // TODO: создать ShaderModule для каждого шейдера
        // TODO: создать bind group layouts, pipeline layouts, render pipelines
        Ok(Self {
            _placeholder: (),
        })
    }

    /// Применяет zoom-трансформацию к текстуре.
    pub fn apply_zoom(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _input: &wgpu::Texture,
        _params: &ZoomParams,
    ) -> Result<()> {
        // TODO: render pass с zoom.wgsl
        Ok(())
    }

    /// Применяет гауссово размытие (два прохода).
    pub fn apply_blur(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _input: &wgpu::Texture,
        _radius: f32,
    ) -> Result<()> {
        // TODO: горизонтальный проход + вертикальный проход blur.wgsl
        Ok(())
    }

    /// Применяет drop shadow.
    pub fn apply_shadow(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _input: &wgpu::Texture,
        _params: &ShadowParams,
    ) -> Result<()> {
        // TODO: render pass с shadow.wgsl
        Ok(())
    }

    /// Применяет motion blur.
    pub fn apply_motion_blur(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _input: &wgpu::Texture,
        _params: &MotionBlurParams,
    ) -> Result<()> {
        // TODO: render pass с motion_blur.wgsl
        Ok(())
    }
}

/// Загружает WGSL шейдер из файла.
pub fn load_shader_source(path: &Path) -> Result<String> {
    std::fs::read_to_string(path)
        .with_context(|| format!("failed to read shader: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shader_dir() -> std::path::PathBuf {
        // Cargo запускает тесты из корня проекта
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("shaders")
    }

    #[test]
    fn test_shader_files_exist() {
        let dir = shader_dir();
        assert!(dir.join("zoom.wgsl").exists(), "zoom.wgsl missing");
        assert!(dir.join("blur.wgsl").exists(), "blur.wgsl missing");
        assert!(dir.join("shadow.wgsl").exists(), "shadow.wgsl missing");
        assert!(
            dir.join("motion_blur.wgsl").exists(),
            "motion_blur.wgsl missing"
        );
    }

    #[test]
    fn test_load_shader_source() {
        let dir = shader_dir();
        let source = load_shader_source(&dir.join("zoom.wgsl")).unwrap();
        assert!(source.contains("vs_main"));
        assert!(source.contains("fs_main"));
        assert!(source.contains("ZoomUniforms"));
    }

    fn validate_wgsl(name: &str, source: &str) {
        let result = naga::front::wgsl::parse_str(source);
        match result {
            Ok(_module) => {}
            Err(err) => panic!("{name} shader failed WGSL validation:\n{err}"),
        }
    }

    #[test]
    fn test_zoom_shader_syntax() {
        let source = load_shader_source(&shader_dir().join("zoom.wgsl")).unwrap();
        validate_wgsl("zoom", &source);
    }

    #[test]
    fn test_blur_shader_syntax() {
        let source = load_shader_source(&shader_dir().join("blur.wgsl")).unwrap();
        validate_wgsl("blur", &source);
    }

    #[test]
    fn test_shadow_shader_syntax() {
        let source = load_shader_source(&shader_dir().join("shadow.wgsl")).unwrap();
        validate_wgsl("shadow", &source);
    }

    #[test]
    fn test_motion_blur_shader_syntax() {
        let source = load_shader_source(&shader_dir().join("motion_blur.wgsl")).unwrap();
        validate_wgsl("motion_blur", &source);
    }

    #[test]
    fn test_zoom_params_layout() {
        assert_eq!(std::mem::size_of::<ZoomParams>(), 16);
    }

    #[test]
    fn test_blur_params_layout() {
        assert_eq!(std::mem::size_of::<BlurParams>(), 32);
    }

    #[test]
    fn test_shadow_params_layout() {
        assert_eq!(std::mem::size_of::<ShadowParams>(), 32);
    }

    #[test]
    fn test_motion_blur_params_layout() {
        assert_eq!(std::mem::size_of::<MotionBlurParams>(), 32);
    }

    #[test]
    #[ignore]
    fn test_shader_pipeline_creation() {
        // Требует GPU
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let instance = wgpu::Instance::default();
            let adapter = instance
                .request_adapter(&Default::default())
                .await
                .unwrap();
            let (device, _queue) = adapter
                .request_device(&Default::default())
                .await
                .unwrap();
            let pipeline = ShaderPipeline::new(&device);
            assert!(pipeline.is_ok());
        });
    }
}
