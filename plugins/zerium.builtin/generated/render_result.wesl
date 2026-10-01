import package::generated::host::_props::{ZeriumRawProps, item_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumProps {
    start_offset: u32,
    end_offset: u32,
    hide_original: bool,
};

fn props(instance_index: u32) -> ZeriumProps {
    let raw = item_props(instance_index);
    return ZeriumProps(read_u32(raw, 0u), read_u32(raw, 4u), read_bool(raw, 8u));
}

@group(1) @binding(0)
var render_result: texture_2d<f32>;

@group(1) @binding(8)
var capability_sampler: sampler;
