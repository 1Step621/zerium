// Packed 16-bit pixel coordinates. All-zero storage denotes a missing site.
fn encode_site(site: vec2<u32>) -> vec4<f32> {
    let x = site.x + 1u;
    let y = site.y + 1u;
    return vec4(
        f32(x & 255u),
        f32((x >> 8u) & 255u),
        f32(y & 255u),
        f32((y >> 8u) & 255u),
    ) / 255.0;
}

fn decode_site(value: vec4<f32>) -> vec2<i32> {
    let bytes = vec4<u32>(round(clamp(value, vec4(0.0), vec4(1.0)) * 255.0));
    let x = bytes.x | (bytes.y << 8u);
    let y = bytes.z | (bytes.w << 8u);
    if x == 0u || y == 0u {
        return vec2(-1);
    }
    return vec2(i32(x) - 1, i32(y) - 1);
}
