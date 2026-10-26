import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, input_position, sample_input};
import package::generated::corner_pin::props;
import package::helpers::geometry::cross_2d;

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> ZeriumVertex {
    return vertex(index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let p = props();
    // Keep the homography local to avoid cancellation far from the scene origin.
    let center = input_position(vec2(0.5));
    let position = uv_to_position(input.uv) - center;
    let a = input_position(vec2(0.0)) - center + vec2(p.top_left.v0, p.top_left.v1);
    let b = input_position(vec2(1.0, 0.0)) - center + vec2(p.top_right.v0, p.top_right.v1);
    let c = input_position(vec2(1.0)) - center + vec2(p.bottom_right.v0, p.bottom_right.v1);
    let d = input_position(vec2(0.0, 1.0)) - center + vec2(p.bottom_left.v0, p.bottom_left.v1);
    // A projective image requires a nondegenerate convex destination quad.
    let turns = vec4(
        cross_2d(b - a, c - b),
        cross_2d(c - b, d - c),
        cross_2d(d - c, a - d),
        cross_2d(a - d, b - a),
    );
    if !(all(turns > vec4(1e-5)) || all(turns < vec4(-1e-5))) {
        return vec4(0.0);
    }
    let e = b - c;
    let f = d - c;
    let delta = a - b + c - d;
    let denominator = cross_2d(e, f);
    if abs(denominator) < 1e-5 {
        return vec4(0.0);
    }
    let projective = vec2(cross_2d(delta, f), cross_2d(e, delta)) / denominator;
    let h0 = vec3(b - a + projective.x * b, projective.x);
    let h1 = vec3(d - a + projective.y * d, projective.y);
    let h2 = vec3(a, 1.0);
    let destination = vec3(position, 1.0);
    let q = vec3(
        dot(cross(h1, h2), destination),
        dot(cross(h2, h0), destination),
        dot(cross(h0, h1), destination),
    );
    if abs(q.z) < 1e-5 {
        return vec4(0.0);
    }
    let uv = q.xy / q.z;
    if any(uv < vec2(0.0)) || any(uv > vec2(1.0)) {
        return vec4(0.0);
    }
    return sample_input(input_position(uv));
}
