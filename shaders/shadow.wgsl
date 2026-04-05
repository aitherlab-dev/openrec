// Drop shadow shader — renders shadow beneath content.
// Samples alpha from offset position, applies blur approximation, composites.

struct ShadowUniforms {
    offset: vec2<f32>,
    texel_size: vec2<f32>,
    blur_radius: f32,
    shadow_color: vec3<f32>,
};

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var tex_sampler: sampler;
@group(0) @binding(2) var<uniform> params: ShadowUniforms;

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

// Box blur approximation for shadow softness
fn sample_shadow_alpha(uv: vec2<f32>) -> f32 {
    let shadow_uv = uv - params.offset * params.texel_size;
    var alpha = 0.0;
    let steps = 5;
    var total_weight = 0.0;

    for (var dx = -steps; dx <= steps; dx = dx + 1) {
        for (var dy = -steps; dy <= steps; dy = dy + 1) {
            let sample_offset = vec2<f32>(f32(dx), f32(dy)) * params.texel_size * params.blur_radius * 0.2;
            let sampled = textureSample(input_texture, tex_sampler, shadow_uv + sample_offset);
            alpha += sampled.a;
            total_weight += 1.0;
        }
    }

    return alpha / total_weight;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let shadow_alpha = sample_shadow_alpha(in.uv);
    let shadow = vec4<f32>(params.shadow_color, shadow_alpha * 0.6);

    let foreground = textureSample(input_texture, tex_sampler, in.uv);

    // Composite: shadow behind foreground (premultiplied alpha)
    let blended = shadow * (1.0 - foreground.a) + foreground;

    return blended;
}
