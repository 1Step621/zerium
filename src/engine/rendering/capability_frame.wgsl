@group(0) @binding(2)
var source: texture_2d<f32>;
@group(0) @binding(3)
var source_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corners = array<vec2<f32>, 3>(
        vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0),
    );
    let corner = corners[index];
    return VertexOutput(
        vec4(corner, 0.0, 1.0),
        corner * vec2(0.5, -0.5) + vec2(0.5),
    );
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(source, source_sampler, input.uv);
    return vec4(color.rgb * color.a, color.a);
}
