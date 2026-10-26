// Deterministic hashes and smooth fractal noise in logical coordinates.
fn hash(value: u32) -> f32 {
    var bits = value;
    bits = (bits ^ (bits >> 16u)) * 0x7feb352du;
    bits = (bits ^ (bits >> 15u)) * 0x846ca68bu;
    bits = bits ^ (bits >> 16u);
    return f32(bits >> 8u) / 16777216.0;
}

fn cell_hash(cell: vec2<i32>, seed: u32) -> f32 {
    let bits = bitcast<vec2<u32>>(cell);
    return hash(bits.x * 0x9e3779b9u + bits.y * 0x85ebca6bu + seed);
}

fn value_noise(position: vec2<f32>, seed: u32) -> f32 {
    let cell = vec2<i32>(floor(position));
    let fraction = fract(position);
    let weight = fraction * fraction * (vec2(3.0) - 2.0 * fraction);
    return mix(
        mix(cell_hash(cell, seed), cell_hash(cell + vec2(1, 0), seed), weight.x),
        mix(cell_hash(cell + vec2(0, 1), seed), cell_hash(cell + vec2(1, 1), seed), weight.x),
        weight.y,
    );
}

fn fractal_noise(position: vec2<f32>, phase: f32, octaves: u32, seed: u32) -> f32 {
    var point = position + phase * vec2(0.37, 0.53);
    var amplitude = 0.5;
    var sum = 0.0;
    var weight = 0.0;

    for(var octave = 0u; octave < octaves; octave += 1u) {
        sum += value_noise(point, seed + octave * 1013u) * amplitude;
        weight += amplitude;
        point = vec2(point.x * 1.6 - point.y * 1.2, point.x * 1.2 + point.y * 1.6)
            + vec2(17.0, 31.0);
        amplitude *= 0.5;
    }

    return sum / max(weight, 0.00001);
}

fn cellular_noise(position: vec2<f32>, seed: u32) -> f32 {
    let cell = vec2<i32>(floor(position));
    var nearest = 2.0;
    for(var y = -1; y <= 1; y += 1) {
        for(var x = -1; x <= 1; x += 1) {
            let neighbor = cell + vec2(x, y);
            let offset = vec2(cell_hash(neighbor, seed), cell_hash(neighbor, seed + 7919u));
            nearest = min(nearest, distance(position, vec2<f32>(neighbor) + offset));
        }
    }
    return clamp(nearest, 0.0, 1.0);
}
