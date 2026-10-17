import package::generated::_internal::properties::{
    ZeriumRawProps,
    read_bool,
    read_f32,
    read_i32,
    read_u32
};
import package::generated::_internal::effect_state::pass_props;

struct ZeriumProps {
    color: vec4<f32>,
    use_luminance: bool,
}

fn props() -> ZeriumProps {
    let raw = pass_props();
    return ZeriumProps(
        vec4(read_f32(raw, 0u), read_f32(raw, 0u + 4u), read_f32(raw, 0u + 8u), read_f32(raw, 0u + 12u)),
        read_bool(raw, 16u),
    );
}
