import package::generated::_internal::properties::ZeriumRawProps;

// Internal GPU ABI. Plugin shaders use the typed generated property API.
struct _ZeriumItem {
    property_offset: u32,
    property_size: u32,
}

@group(0) @binding(0)
var<storage, read> _items: array<_ZeriumItem>;

struct _ZeriumSurface {
    output_size: vec2<f32>,
    composition_size: vec2<f32>,
    surface_min: vec2<f32>,
    surface_size: vec2<f32>,
}

@group(0) @binding(2)
var<uniform> _surface: _ZeriumSurface;

fn item_props(instance_index: u32) -> ZeriumRawProps {
    let item = _items[instance_index];
    return ZeriumRawProps(item.property_offset, item.property_size);
}
