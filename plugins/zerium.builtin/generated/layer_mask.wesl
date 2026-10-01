import package::generated::host::_props::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumProps {
    start_offset: u32,
    end_offset: u32,
    hide_original: bool,
    invert_mask: bool,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(read_u32(raw, 0u), read_u32(raw, 4u), read_bool(raw, 8u), read_bool(raw, 12u));
}

@group(1) @binding(0)
var behind: texture_2d<f32>;

@group(1) @binding(8)
var capability_sampler: sampler;
