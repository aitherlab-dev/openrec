// Motion blur — directional blur along a velocity vector.
// Samples along direction with configurable strength.

struct MotionBlurUniforms {
    direction: vec2<f32>,
    texel_size: vec2<f32>,
    strength: f32,
    _padding: vec3<f32>,
};

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var tex_sampler: sampler;
@group(0) @binding(2) var<uniform> params: MotionBlurUniforms;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;

    let x = f32(i32(vertex_index & 1u)) * 4.0 - 1.0;
    let y = f32(i32(vertex_index >> 1u)) * 4.0 - 1.0;

    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let samples = 16;
    let step = params.direction * params.texel_size * params.strength / f32(samples);

    var color = vec4<f32>(0.0);

    for (var i = 0; i < samples; i = i + 1) {
        let offset_factor = f32(i) - f32(samples) * 0.5;
        let sample_uv = in.uv + step * offset_factor;
        let clamped = clamp(sample_uv, vec2<f32>(0.0), vec2<f32>(1.0));
        color += textureSample(input_texture, tex_sampler, clamped);
    }

    return color / f32(samples);
}
