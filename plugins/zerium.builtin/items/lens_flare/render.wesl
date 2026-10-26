import package::helpers::surface::{SurfaceVertex, surface_vertex, surface_position};
import package::generated::lens_flare::props;
import package::generated::support::util::scene_color;
import package::helpers::noise::hash;

@vertex
fn vertex_main(@builtin(vertex_index) index: u32, @builtin(instance_index) instance: u32) -> SurfaceVertex {
    return surface_vertex(index, instance);
}

@fragment
fn fragment_main(input: SurfaceVertex) -> @location(0) vec4<f32> {
    let p = props(input.instance_index);
    let position = surface_position(input);
    if p.radius <= 0.0 || p.intensity <= 0.0 {
        return vec4(0.0);
    }
    let light_position = vec2(p.position.v0, p.position.v1);
    let local = position - light_position;
    let r = length(local) / p.radius;
    let core = exp(-r * r * 80.0) * 2.0;
    let glow = exp(-r * 4.0) * 0.5;
    let ring = exp(-pow((r - 0.75) * 35.0, 2.0)) * p.halo / 200.0;
    let streak = exp(-abs(local.y) / max(
        p.radius * 0.015,
        0.5,
    )) * exp(-abs(local.x) / max(p.streak, 0.001)) * select(0.0, 1.0, p.streak > 0.0);
    let tint = scene_color(p.color);
    var rgb = tint.rgb * (core + glow + ring + streak * 0.5);
    var energy = core + glow + ring + streak * 0.5;
    // Reflections follow the optical axis through the viewport center.
    for (var index = 0u; index < p.ghosts; index += 1u) {
        let t = f32(index + 1u) / f32(p.ghosts + 1u);
        let ghost_center = mix(light_position, -light_position, t * 1.8);
        let radius = p.radius * mix(0.12, 0.45, hash(index + 71u));
        let d = length(position - ghost_center) / max(radius, 0.001);
        let ghost = (1.0 - smoothstep(0.85, 1.0, d)) * 0.08;
        let ghost_color = vec3(
            0.3 + 0.7 * hash(index + 1u),
            0.3 + 0.7 * hash(index + 2u),
            0.3 + 0.7 * hash(index + 3u),
        );
        rgb += ghost_color * ghost;
        energy += ghost;
    }
    let strength = p.intensity / 100.0;
    let alpha = (1.0 - exp(-energy * strength)) * tint.a;
    return vec4(rgb * strength * tint.a, alpha);
}
