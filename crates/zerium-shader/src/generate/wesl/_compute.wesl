// Internal GPU ABI. Plugin shaders use the typed generated property API.

struct _ZeriumCompute {
    property_offset: u32,
    property_size: u32,
    width: u32,
    height: u32,
    composition_size: vec2<f32>,
    surface_min: vec2<f32>,
    surface_size: vec2<f32>,
};

@group(0) @binding(2)
var<uniform> _compute_info: _ZeriumCompute;

@group(0) @binding(3)
var<storage, read> _property_words: array<u32>;

fn effect_props() -> ZeriumRawProps {
    return ZeriumRawProps(
        _compute_info.property_offset,
        _compute_info.property_size,
    );
}
