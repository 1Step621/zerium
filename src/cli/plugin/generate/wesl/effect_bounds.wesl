struct EffectBounds {
    from_right: u32,
    from_bottom: u32,
    max_x: u32,
    max_y: u32,
};

// Importing this binding requests the source alpha bounds for the effect.
@group(2) @binding(0)
var<storage, read> effect_bounds: EffectBounds;

fn contains(position: vec2<u32>, size: vec2<u32>, expansion: f32) -> bool {
    if effect_bounds.max_x == 0u {
        return false;
    }
    let margin = u32(ceil(max(expansion, 0.0)));
    let left = size.x - effect_bounds.from_right;
    let top = size.y - effect_bounds.from_bottom;
    return position.x + margin >= left
        && position.y + margin >= top
        && position.x < effect_bounds.max_x + margin
        && position.y < effect_bounds.max_y + margin;
}
