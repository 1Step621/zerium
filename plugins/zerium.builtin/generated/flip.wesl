import package::generated::host::_effect::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumProps {
    horizontal: bool,
    vertical: bool,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(read_bool(raw, 0u), read_bool(raw, 4u));
}
