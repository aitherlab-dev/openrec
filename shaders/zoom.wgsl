// Zoom shader — fullscreen quad + texture sampling with scale/translate transform.
// Bilinear filtering via textureSample.

struct ZoomUniforms {
    scale: f32,
    translate_x: f32,
    translate_y: f32,
    _padding: f32,
};

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var tex_sampler: sampler;
@group(0) @binding(2) var<uniform> params: ZoomUniforms;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Fullscreen triangle — 3 вершины покрывают весь экран
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;

    // Generate fullscreen triangle positions
    let x = f32(i32(vertex_index & 1u)) * 4.0 - 1.0;
    let y = f32(i32(vertex_index >> 1u)) * 4.0 - 1.0;

    out.position = vec4<f32>(x, y, 0.0, 1.0);
    // Flip Y for texture coordinates
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Apply zoom transform: scale around center + translate
    let center = vec2<f32>(0.5, 0.5);
    let inv_scale = 1.0 / params.scale;
    let offset = vec2<f32>(params.translate_x, params.translate_y);

    let uv = (in.uv - center) * inv_scale + center + offset;

    // Clamp to valid range
    let clamped_uv = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));

    return textureSample(input_texture, tex_sampler, clamped_uv);
}
