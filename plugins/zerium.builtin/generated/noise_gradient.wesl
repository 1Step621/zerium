import package::generated::host::_effect::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumPropsTuple0 {
    v0: f32,
    v1: vec4<f32>,
};

struct ZeriumProps {
    _raw: ZeriumRawProps,
    stops_len: u32,
    angle: f32,
    grain_size: f32,
    width: f32,
    offset: f32,
    phase: f32,
    seed: u32,
    amount: f32,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(raw, read_u32(raw, 4u), read_f32(raw, 8u), read_f32(raw, 12u), read_f32(raw, 16u), read_f32(raw, 20u), read_f32(raw, 24u), read_u32(raw, 28u), read_f32(raw, 32u));
}

fn get_stops(properties: ZeriumProps, index: u32) -> ZeriumPropsTuple0 {
    if index >= properties.stops_len { return ZeriumPropsTuple0(0.0, vec4(0.0)); }
    let byte_offset = read_u32(properties._raw, 0u) + index * 20u;
    return ZeriumPropsTuple0(read_f32(properties._raw, byte_offset), vec4(read_f32(properties._raw, byte_offset + 4u), read_f32(properties._raw, byte_offset + 4u + 4u), read_f32(properties._raw, byte_offset + 4u + 8u), read_f32(properties._raw, byte_offset + 4u + 12u)));
}
