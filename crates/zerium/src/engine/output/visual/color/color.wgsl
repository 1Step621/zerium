@group(0) @binding(0) var image: texture_2d<f32>;

override COLOR_SPACE: u32 = 0u; // BT.709, BT.2020 SDR, BT.2100 PQ, BT.2100 HLG.
override FULL_RANGE: bool = false;
override TEN_BIT: bool = false;

@vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let p = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(p[index], 0.0, 1.0);
}

fn transfer(value: f32) -> f32 {
    let x = max(value, 0.0);
    switch COLOR_SPACE {
        case 2u: {
            // Scene white is 203 cd/m²; PQ is absolute, normalized to 10000 cd/m².
            let p = pow(clamp(x * 203.0 / 10000.0, 0.0, 1.0), 2610.0 / 16384.0);
            return pow((3424.0 / 4096.0 + 2413.0 / 128.0 * p)
                / (1.0 + 2392.0 / 128.0 * p), 2523.0 / 32.0);
        }
        case 3u: {
            if x <= 1.0 / 12.0 { return sqrt(3.0 * x); }
            return 0.17883277 * log(12.0 * x - 0.28466892) + 0.55991073;
        }
        default: {
            let alpha = select(1.0992968268, 1.099, COLOR_SPACE == 0u);
            let beta = select(0.0180539685, 0.018, COLOR_SPACE == 0u);
            if x < beta { return 4.5 * x; }
            return alpha * pow(x, 0.45) - (alpha - 1.0);
        }
    }
}

fn encoded_rgb(position: vec2<i32>) -> vec3<f32> {
    // The scene is linear BT.709 RGB, premultiplied over the output's black background.
    var rgb = textureLoad(image, position, 0).rgb;
    if COLOR_SPACE != 0u {
        rgb = vec3(dot(rgb, vec3(0.6274039, 0.3292830, 0.0433131)),
            dot(rgb, vec3(0.0690973, 0.9195404, 0.0113623)),
            dot(rgb, vec3(0.0163914, 0.0880133, 0.8955953)));
    }
    rgb = max(rgb, vec3(0.0));
    if COLOR_SPACE == 3u {
        // Invert the reference HLG display OOTF (1000 cd/m², system gamma 1.2).
        let display = rgb * (203.0 / 1000.0);
        let luminance = dot(display, vec3(0.2627, 0.6780, 0.0593));
        rgb = display * pow(max(luminance, 0.0000001), 1.0 / 1.2 - 1.0);
    }
    return clamp(vec3(transfer(rgb.r), transfer(rgb.g), transfer(rgb.b)), vec3(0.0), vec3(1.0));
}

fn yuv(rgb: vec3<f32>) -> vec3<f32> {
    let weights = select(vec3(0.2627, 0.6780, 0.0593), vec3(0.2126, 0.7152, 0.0722), COLOR_SPACE == 0u);
    let y = dot(rgb, weights);
    let uv = 0.5 * (rgb.br - vec2(y)) / (vec2(1.0) - weights.br);
    let maximum = select(255.0, 1023.0, TEN_BIT);
    if FULL_RANGE { return vec3(y, uv + vec2(select(128.0, 512.0, TEN_BIT) / maximum)); }
    let scale = select(1.0, 4.0, TEN_BIT) / maximum;
    return vec3((16.0 + 219.0 * y) * scale, (vec2(128.0) + 224.0 * uv) * scale);
}

fn chroma_value(position: vec2<f32>) -> vec2<f32> {
    let base = vec2<i32>(position) * 2;
    let rgb = (encoded_rgb(base) + encoded_rgb(base + vec2(1, 0))
        + encoded_rgb(base + vec2(0, 1)) + encoded_rgb(base + vec2(1, 1))) * 0.25;
    return clamp(yuv(rgb).yz, vec2(0.0), vec2(1.0));
}

@fragment fn luma(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4(clamp(yuv(encoded_rgb(vec2<i32>(p.xy))).x, 0.0, 1.0), 0.0, 0.0, 1.0);
}

@fragment fn chroma(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4(chroma_value(p.xy), 0.0, 1.0);
}

// P010 stores each 10-bit sample in the most significant bits of a 16-bit word.
@fragment fn luma10(@builtin(position) p: vec4<f32>) -> @location(0) vec4<u32> {
    let y = clamp(yuv(encoded_rgb(vec2<i32>(p.xy))).x, 0.0, 1.0);
    return vec4(u32(round(y * 1023.0)) << 6u, 0u, 0u, 0u);
}

@fragment fn chroma10(@builtin(position) p: vec4<f32>) -> @location(0) vec4<u32> {
    return vec4(vec2<u32>(round(chroma_value(p.xy) * 1023.0)) << vec2(6u), 0u, 0u);
}

// The sRGB attachment applies the transfer function after unpremultiplication.
@fragment fn rgba(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let color = textureLoad(image, vec2<i32>(p.xy), 0);
    if color.a <= 0.00001 { return vec4(0.0); }
    return vec4(clamp(color.rgb / color.a, vec3(0.0), vec3(1.0)), clamp(color.a, 0.0, 1.0));
}
