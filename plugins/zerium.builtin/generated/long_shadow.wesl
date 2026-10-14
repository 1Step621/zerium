import package::generated::host::_effect::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumProps {
    length: f32,
    angle: f32,
    color: vec4<f32>,
    fade: bool,
    samples: u32,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(read_f32(raw, 0u), read_f32(raw, 4u), vec4(read_f32(raw, 8u), read_f32(raw, 8u + 4u), read_f32(raw, 8u + 8u), read_f32(raw, 8u + 12u)), read_bool(raw, 24u), read_u32(raw, 28u));
}
