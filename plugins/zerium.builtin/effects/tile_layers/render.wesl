import package::generated::host::effect::{
    ZeriumVertex,
    effect_input,
    effect_sampler,
    uv_to_position,
    vertex,
};
import package::generated::host::util::{DEG2RAD, rotate};
import package::generated::tile_layers::{behind, capability_sampler, props};

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let position = rotate(uv_to_position(input.uv) - vec2(properties.offset.v0, properties.offset.v1), -properties.angle * DEG2RAD);
    let cell = position / vec2(properties.tile_size.v0, properties.tile_size.v1) + vec2(0.5);
    var uv = fract(cell);
    if properties.mirror {
        let parity = vec2<u32>(bitcast<vec2<u32>>(vec2<i32>(floor(cell)))) % vec2(2u);
        uv = select(uv, vec2(1.0) - uv, parity == vec2(1u));
    }
    let source = textureSample(effect_input, effect_sampler, input.uv);
    let layers = textureSample(behind, capability_sampler, uv);
    return layers * select(1.0, source.a, properties.clip_to_input);
}
