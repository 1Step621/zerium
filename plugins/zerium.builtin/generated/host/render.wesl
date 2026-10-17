struct ZeriumVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

fn vertex(vertex_index: u32) -> ZeriumVertex {
    let positions = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    let point = positions[vertex_index];
    return ZeriumVertex(vec4(point, 0.0, 1.0), point * vec2(0.5, -0.5) + vec2(0.5));
}
