import package::generated::item::{ZeriumRawProps, item_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumProps {
    _raw: ZeriumRawProps,
    start_offset: u32,
    end_offset: u32,
    hide_original: bool,
};

fn props(instance_index: u32) -> ZeriumProps {
    let raw = item_props(instance_index);
    return ZeriumProps(raw, read_u32(raw, 0u), read_u32(raw, 4u), read_bool(raw, 8u));
}
