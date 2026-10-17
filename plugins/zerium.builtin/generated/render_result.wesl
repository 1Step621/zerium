import package::generated::_internal::properties::{
    ZeriumRawProps,
    read_bool,
    read_f32,
    read_i32,
    read_u32
};
import package::generated::_internal::item::item_props;

struct ZeriumProps {
    start_offset: u32,
    end_offset: u32,
    hide_original: bool,
}

fn props(instance_index: u32) -> ZeriumProps {
    let raw = item_props(instance_index);
    return ZeriumProps(
        read_u32(raw, 0u),
        read_u32(raw, 4u),
        read_bool(raw, 8u),
    );
}
