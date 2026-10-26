import package::helpers::surface::{SurfaceVertex, surface_vertex, surface_position};
import package::generated::pattern::props;
import package::generated::host::item::placement_center;
import package::generated::support::util::{rotate, DEG2RAD, scene_color};
import package::helpers::geometry::distance_coverage;

@vertex
fn vertex_main(@builtin(vertex_index) index: u32, @builtin(instance_index) instance: u32) -> SurfaceVertex {
    return surface_vertex(index, instance);
}

@fragment
fn fragment_main(input: SurfaceVertex) -> @location(0) vec4<f32> {
    let p = props(input.instance_index);
    let position = surface_position(input);
    let center = placement_center(
        vec2(p.position.v0, p.position.v1),
        vec2(p.size.v0, p.size.v1),
        vec2(p.origin.v0, p.origin.v1),
    );
    let local = position - center;
    let size = vec2(p.size.v0, p.size.v1);
    let edge = max(abs(local.x) - size.x * 0.5, abs(local.y) - size.y * 0.5);
    let point = rotate(local - vec2(p.offset.v0, p.offset.v1), -p.angle * DEG2RAD);
    let cell = floor(point / p.spacing);
    let delta = point - (cell + vec2(0.5)) * p.spacing;
    let width = min(p.width, p.spacing);
    var distance = min(abs(delta.x), abs(delta.y)) - width * 0.5;
    var coverage = 0.0;
    if p.mode == 1u {
        let parity = f32(((i32(cell.x) + i32(cell.y)) % 2 + 2) % 2);
        let fraction = fract(point / p.spacing);
        let nearest = min(fraction, vec2(1.0) - fraction) * p.spacing;
        let footprint = max(max(fwidth(point.x), fwidth(point.y)), 0.0001);
        coverage = mix(0.5, parity, smoothstep(0.0, footprint * 0.5, min(nearest.x, nearest.y)));
    } else {
        if p.mode == 2u {
            distance = abs(delta.y) - width * 0.5;
        }
        if p.mode == 3u {
            distance = length(delta) - width * 0.5;
        }
        coverage = distance_coverage(distance, 0.001);
    }
    return mix(scene_color(p.background), scene_color(p.color), coverage) * distance_coverage(edge, 0.001);
}
