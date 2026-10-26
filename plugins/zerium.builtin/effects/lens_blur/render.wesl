import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, sample_input};
import package::generated::lens_blur::props;
import package::helpers::geometry::PI;
import package::generated::support::util::DEG2RAD;

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> ZeriumVertex {
    return vertex(index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let p = props();
    let position = uv_to_position(input.uv);
    let source = sample_input(position);
    if p.radius <= 0.0 {
        return source;
    }
    var sum = vec4(0.0);
    for (var index = 0u; index < p.samples; index += 1u) {
        let angle = f32(index) * 2.39996323;
        let r = sqrt((f32(index) + 0.5) / f32(p.samples));
        var aperture = 1.0;
        if p.blades >= 3u {
            let half_sector = PI / f32(p.blades);
            let local_angle = angle - p.angle * DEG2RAD;
            let sector_angle = local_angle - floor((local_angle + half_sector) / (2.0 * half_sector)) * 2.0 * half_sector;
            aperture = cos(half_sector) / cos(sector_angle);
        }
        sum += sample_input(position + vec2(cos(angle), sin(angle)) * (p.radius * r * aperture));
    }
    return sum / f32(p.samples);
}
