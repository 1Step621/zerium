//! Triangle resampling in premultiplied, linear sRGB without a full float image.
use crate::engine::{
    color::{linear_to_srgb, srgb_to_linear},
    input::MediaError,
};
use image::{GenericImageView, Rgba, RgbaImage};
use std::sync::{
    LazyLock,
    atomic::{AtomicBool, Ordering},
};

static LINEAR: LazyLock<[f32; 256]> =
    LazyLock::new(|| std::array::from_fn(|index| srgb_to_linear(index as f32 / 255.)));

struct Weights {
    start: u32,
    values: Vec<f32>,
}

fn weights(source: u32, destination: u32) -> Vec<Weights> {
    let ratio = source as f64 / destination as f64;
    let support = ratio.max(1.);
    (0..destination)
        .map(|index| {
            let center = (index as f64 + 0.5) * ratio;
            let start = ((center - support).floor() as i64).clamp(0, i64::from(source) - 1) as u32;
            let end = ((center + support).ceil() as u32).min(source);
            let mut values: Vec<_> = (start..end)
                .map(|position| {
                    (1. - ((position as f64 + 0.5 - center) / support).abs()).max(0.) as f32
                })
                .collect();
            let sum: f32 = values.iter().sum();
            for value in &mut values {
                *value /= sum;
            }
            Weights { start, values }
        })
        .collect()
}

pub(super) fn resize(
    source: &impl GenericImageView<Pixel = Rgba<u8>>,
    width: u32,
    height: u32,
    cancelled: &AtomicBool,
) -> Result<RgbaImage, MediaError> {
    let horizontal = weights(source.width(), width);
    let vertical = weights(source.height(), height);
    let mut row = vec![[0.; 4]; source.width() as usize];
    let mut output = RgbaImage::new(width, height);
    for (y, weights) in vertical.iter().enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            return Err(MediaError::Cancelled);
        }
        row.fill([0.; 4]);
        for (offset, weight) in weights.values.iter().copied().enumerate() {
            if weight == 0. {
                continue;
            }
            let source_y = weights.start + offset as u32;
            for (x, sum) in row.iter_mut().enumerate() {
                let Rgba([r, g, b, a]) = source.get_pixel(x as u32, source_y);
                let alpha = f32::from(a) / 255. * weight;
                sum[0] += LINEAR[r as usize] * alpha;
                sum[1] += LINEAR[g as usize] * alpha;
                sum[2] += LINEAR[b as usize] * alpha;
                sum[3] += alpha;
            }
        }
        for (x, weights) in horizontal.iter().enumerate() {
            let mut sum = [0.; 4];
            for (offset, weight) in weights.values.iter().copied().enumerate() {
                let pixel = row[weights.start as usize + offset];
                for channel in 0..4 {
                    sum[channel] += pixel[channel] * weight;
                }
            }
            let mut pixel = [0; 4];
            if sum[3] > 0. {
                for channel in 0..3 {
                    let encoded = linear_to_srgb(sum[channel] / sum[3]);
                    pixel[channel] = (encoded.clamp(0., 1.) * 255.).round() as u8;
                }
                pixel[3] = (sum[3].clamp(0., 1.) * 255.).round() as u8;
            }
            output.put_pixel(x as u32, y as u32, Rgba(pixel));
        }
    }
    Ok(output)
}
