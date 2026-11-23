struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@group(0) @binding(0) var coverage_texture: texture_2d<f32>;
@group(0) @binding(1) var coverage_sampler: sampler;
@group(0) @binding(2) var color_texture: texture_2d<f32>;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    let position = positions[index];
    let uv = vec2<f32>((position.x + 1.0) * 0.5, (1.0 - position.y) * 0.5);
    return VertexOutput(vec4<f32>(position, 0.0, 1.0), uv);
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    // R8Unorm is a linear coverage mask. The color texture is sRGB, so sampling it yields
    // linear RGB. Premultiply only here, immediately before premultiplied-alpha blending.
    let coverage = textureSample(coverage_texture, coverage_sampler, input.uv).r;
    let color = textureSample(color_texture, coverage_sampler, vec2<f32>(0.5, 0.5));
    let alpha = coverage * color.a;
    return vec4<f32>(color.rgb * alpha, alpha);
}
