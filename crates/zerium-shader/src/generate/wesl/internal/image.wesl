// Shared image contract: logical bounds, raster coverage, and sampling density.
struct ZeriumImage {
    bounds_min: vec2<f32>,
    bounds_size: vec2<f32>,
    raster_min: vec2<f32>,
    raster_size: vec2<f32>,
    density: vec2<f32>,
    _padding: vec2<f32>,
}

fn _image_position(image: ZeriumImage, uv: vec2<f32>) -> vec2<f32> {
    return image.bounds_min + uv * image.bounds_size;
}

fn _sample_image(
    texture: texture_2d<f32>,
    image: ZeriumImage,
    image_sampler: sampler,
    position: vec2<f32>,
) -> vec4<f32> {
    if any(image.bounds_size <= vec2(0.0)) || any(image.raster_size <= vec2(0.0)) {
        return vec4(0.0);
    }

    let size = vec2<f32>(textureDimensions(texture));
    let pixel = (position - image.raster_min) * image.density;
    let texel = floor(pixel);

    if all(abs(pixel - texel - vec2(0.5)) < vec2(0.0001)) {
        if any(texel < vec2(0.0)) || any(texel >= size) {
            return vec4(0.0);
        }

        return textureLoad(texture, vec2<i32>(texel), 0);
    }

    let coverage = clamp(pixel + vec2(0.5), vec2(0.0), vec2(1.0))
        * clamp(size + vec2(0.5) - pixel, vec2(0.0), vec2(1.0));
    return textureSampleLevel(texture, image_sampler, pixel / size, 0.0) * coverage.x * coverage.y;
}

fn _load_image(texture: texture_2d<f32>, image: ZeriumImage, position: vec2<f32>) -> vec4<f32> {
    if any(image.bounds_size <= vec2(0.0)) || any(image.raster_size <= vec2(0.0)) {
        return vec4(0.0);
    }

    let pixel = vec2<i32>(floor((position - image.raster_min) * image.density));
    let size = vec2<i32>(textureDimensions(texture));

    if any(pixel < vec2(0)) || any(pixel >= size) {
        return vec4(0.0);
    }

    return textureLoad(texture, pixel, 0);
}
