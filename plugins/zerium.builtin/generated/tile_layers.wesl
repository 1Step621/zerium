import package::generated::host::_effect::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumPropsTuple3 {
    v0: f32,
    v1: f32,
};

struct ZeriumPropsTuple4 {
    v0: f32,
    v1: f32,
};

struct ZeriumProps {
    start_offset: u32,
    end_offset: u32,
    hide_original: bool,
    tile_size: ZeriumPropsTuple3,
    offset: ZeriumPropsTuple4,
    angle: f32,
    mirror: bool,
    clip_to_input: bool,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(read_u32(raw, 0u), read_u32(raw, 4u), read_bool(raw, 8u), ZeriumPropsTuple3(read_f32(raw, 12u), read_f32(raw, 12u + 4u)), ZeriumPropsTuple4(read_f32(raw, 20u), read_f32(raw, 20u + 4u)), read_f32(raw, 28u), read_bool(raw, 32u), read_bool(raw, 36u));
}

@group(1) @binding(0)
var behind: texture_2d<f32>;

@group(1) @binding(8)
var capability_sampler: sampler;
