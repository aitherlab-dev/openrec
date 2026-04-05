// Gaussian blur — separable 13-tap kernel.
// Run twice: horizontal (direction = vec2(1,0)) then vertical (direction = vec2(0,1)).

struct BlurUniforms {
    direction: vec2<f32>,
    texel_size: vec2<f32>,
    radius: f32,
    _padding: vec3<f32>,
};

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var tex_sampler: sampler;
@group(0) @binding(2) var<uniform> params: BlurUniforms;

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
    // 13-tap Gaussian weights (sigma ~= 4)
    let offsets = array<f32, 7>(0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0);
    let weights = array<f32, 7>(
        0.1964825501511404,
        0.2969069646728344,
        0.09447039785044732,
        0.010381362401148057,
        0.0,
        0.0,
        0.0,
    );

    let step = params.direction * params.texel_size * params.radius;

    var color = textureSample(input_texture, tex_sampler, in.uv) * weights[0];

    for (var i = 1; i < 7; i = i + 1) {
        if weights[i] <= 0.0 {
            break;
        }
        let offset = step * offsets[i];
        color += textureSample(input_texture, tex_sampler, in.uv + offset) * weights[i];
        color += textureSample(input_texture, tex_sampler, in.uv - offset) * weights[i];
    }

    return color;
}
