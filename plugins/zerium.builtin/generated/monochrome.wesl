import package::generated::host::_effect::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumProps {
    color: vec4<f32>,
    use_luminance: bool,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(vec4(read_f32(raw, 0u), read_f32(raw, 0u + 4u), read_f32(raw, 0u + 8u), read_f32(raw, 0u + 12u)), read_bool(raw, 16u));
}
