import package::generated::_internal::properties::ZeriumRawProps;

// Internal pass ABI, shared by render, compute, and temporal reducers.
struct _ZeriumPass {
    property_offset: u32,
    property_size: u32,
    sample_index: u32,
    sample_count: u32,
    frame_offset: f32,
    sample_progress: f32,
    composition_size: vec2<f32>,
    surface_min: vec2<f32>,
    surface_size: vec2<f32>,
    output_size: vec2<u32>,
    bounds_min: vec2<f32>,
    bounds_size: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> _pass_state: _ZeriumPass;

fn pass_props() -> ZeriumRawProps {
    return ZeriumRawProps(_pass_state.property_offset, _pass_state.property_size);
}
