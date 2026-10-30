use cosmic_text::{
    Align, Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, Stretch, Style, SwashCache,
    Weight, Wrap, fontdb,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::engine::input::VisualFrame;

use super::{RenderError, TextSignature};

struct ResolvedFont<'a> {
    id: Option<fontdb::ID>,
    attrs: Attrs<'a>,
}

pub(super) fn rasterize(
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    signature: &TextSignature,
    byte_budget: usize,
) -> Result<VisualFrame, RenderError> {
    let scale_x = signature.target_size.width as f32 / signature.composition_size.width as f32;
    let scale_y = signature.target_size.height as f32 / signature.composition_size.height as f32;
    let uniform_scale = scale_x.min(scale_y);
    let width = (signature.box_size[0].max(1.) * scale_x).ceil() as u32;
    let height = (signature.box_size[1].max(1.) * scale_y).ceil() as u32;
    let pixel_count = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .filter(|pixels| {
            pixels
                .checked_mul(4)
                .is_some_and(|bytes| bytes <= byte_budget)
        })
        .ok_or_else(|| {
            RenderError::resource_limit(format!(
                "text texture exceeds the {} byte cache working-set limit",
                byte_budget
            ))
        })?;
    let font_size = (signature.font_size * uniform_scale).max(1.);
    let line_height = (font_size * 1.2 + signature.line_spacing * uniform_scale).max(1.);
    let extra_line_height = line_height - font_size * 1.2;
    let letter_spacing_pixels = signature.letter_spacing * uniform_scale;
    let letter_spacing = letter_spacing_pixels / font_size;
    let metrics = Metrics::new(font_size, line_height);
    let mut buffer = Buffer::new(font_system, metrics);
    let weight = if signature.bold {
        Weight::BOLD
    } else {
        Weight::NORMAL
    };
    let style = if signature.italic {
        Style::Italic
    } else {
        Style::Normal
    };
    let fonts = signature
        .font_families
        .iter()
        .map(String::as_str)
        .chain(std::iter::once("sans-serif"))
        .map(|family| resolve_font(font_system, family, weight, style, letter_spacing))
        .collect::<Vec<_>>();
    let attrs = &fonts[0].attrs;
    let font_runs = font_runs(font_system, &signature.content, &fonts, weight);
    let alignment = match signature.horizontal_alignment {
        1 => Align::Center,
        2 => Align::Right,
        _ => Align::Left,
    };
    let mut coverage = vec![0_u8; pixel_count];
    {
        let mut borrowed = buffer.borrow_with(font_system);
        // cosmic-text counts one trailing letter space in each line's width.
        borrowed.set_size(Some((width as f32 + letter_spacing_pixels).max(1.)), None);
        borrowed.set_wrap(Wrap::WordOrGlyph);
        if font_runs.is_empty() {
            borrowed.set_text(
                &signature.content,
                attrs,
                Shaping::Advanced,
                Some(alignment),
            );
        } else {
            borrowed.set_rich_text(
                font_runs.iter().map(|(range, family)| {
                    (
                        &signature.content[range.clone()],
                        fonts[*family].attrs.clone(),
                    )
                }),
                attrs,
                Shaping::Advanced,
                Some(alignment),
            );
        }
        borrowed.shape_until_scroll(false);
        // Extra leading belongs between lines, not above or below the text block.
        let content_height = borrowed
            .layout_runs()
            .last()
            .map_or(0., |run| run.line_top + run.line_height - extra_line_height);
        let remaining_height = (height as f32 - content_height).max(0.);
        let offset_y = (match signature.vertical_alignment {
            1 => remaining_height / 2.,
            2 => remaining_height,
            _ => 0.,
        } - extra_line_height / 2.)
            .round() as i32;
        borrowed.draw(
            swash_cache,
            Color::rgb(255, 255, 255),
            |x, y, glyph_width, glyph_height, color| {
                for row in 0..glyph_height as i32 {
                    let pixel_y = y + row + offset_y;
                    if !(0..height as i32).contains(&pixel_y) {
                        continue;
                    }
                    for offset_x in 0..glyph_width as i32 {
                        let pixel_x = x + offset_x;
                        if !(0..width as i32).contains(&pixel_x) {
                            continue;
                        }
                        let index = pixel_y as usize * width as usize + pixel_x as usize;
                        coverage[index] = coverage[index].max(color.a());
                    }
                }
            },
        );
    }
    let outline_radius = (signature.outline_width.max(0.) * uniform_scale).min(64.);
    let outline = outline_from_distance(&coverage, width as usize, height as usize, outline_radius);
    let rgba = colorize(
        &coverage,
        &outline,
        signature.color,
        signature.outline_color,
    );
    Ok(VisualFrame::rgba(width, height, rgba))
}

fn font_family(name: &str) -> Family<'_> {
    match name.trim().to_ascii_lowercase().as_str() {
        "" | "sans-serif" | "sans serif" => Family::SansSerif,
        "serif" => Family::Serif,
        "monospace" => Family::Monospace,
        _ => Family::Name(name.trim()),
    }
}

fn resolve_font<'a>(
    font_system: &FontSystem,
    name: &'a str,
    requested_weight: Weight,
    style: Style,
    letter_spacing: f32,
) -> ResolvedFont<'a> {
    let family = font_family(name);
    let id = font_system.db().query(&fontdb::Query {
        families: &[family],
        weight: requested_weight,
        stretch: Stretch::Normal,
        style,
    });
    let resolved_weight = id
        .and_then(|id| font_system.db().face(id))
        .map_or(requested_weight, |face| face.weight);
    ResolvedFont {
        id,
        attrs: Attrs::new()
            .family(family)
            .weight(resolved_weight)
            .style(style)
            .letter_spacing(letter_spacing),
    }
}

fn font_runs(
    font_system: &mut FontSystem,
    text: &str,
    families: &[ResolvedFont<'_>],
    weight: Weight,
) -> Vec<(std::ops::Range<usize>, usize)> {
    if text.is_empty() || families.len() < 2 {
        return Vec::new();
    }
    let fonts = families
        .iter()
        .map(|family| family.id.and_then(|id| font_system.get_font(id, weight)))
        .collect::<Vec<_>>();
    let mut runs = Vec::new();
    let mut current_family = 0;
    let mut run_start = 0;
    for (index, grapheme) in text.grapheme_indices(true) {
        let family = fonts
            .iter()
            .position(|font| {
                font.as_ref()
                    .is_some_and(|font| font_supports(font, grapheme))
            })
            .unwrap_or(families.len() - 1);
        if index > run_start && family != current_family {
            runs.push((run_start..index, current_family));
            run_start = index;
        }
        current_family = family;
    }
    runs.push((run_start..text.len(), current_family));
    runs
}

fn font_supports(font: &cosmic_text::Font, grapheme: &str) -> bool {
    let charmap = font.as_swash().charmap();
    grapheme.chars().all(|character| {
        character.is_control()
            || matches!(character as u32, 0x200c..=0x200d | 0xfe00..=0xfe0f | 0xe0100..=0xe01ef)
            || charmap.map(character) != 0
    })
}

fn outline_from_distance(source: &[u8], width: usize, height: usize, radius: f32) -> Vec<u8> {
    if radius <= 0. || !source.iter().any(|alpha| *alpha > 0) {
        return vec![0; source.len()];
    }

    const INFINITY: f32 = 1.0e20;
    let mut distances = source
        .iter()
        .map(|alpha| {
            if *alpha == 0 {
                INFINITY
            } else {
                let edge_offset = 1. - f32::from(*alpha) / 255.;
                edge_offset * edge_offset
            }
        })
        .collect::<Vec<_>>();
    let scratch_len = width.max(height);
    let mut line = vec![INFINITY; scratch_len];
    let mut transformed = vec![INFINITY; scratch_len];
    let mut sites = vec![0_usize; scratch_len];
    let mut boundaries = vec![0_f32; scratch_len + 1];

    for x in 0..width {
        for y in 0..height {
            line[y] = distances[y * width + x];
        }
        distance_transform_1d(
            &line[..height],
            &mut transformed[..height],
            &mut sites[..height],
            &mut boundaries[..=height],
        );
        for y in 0..height {
            distances[y * width + x] = transformed[y];
        }
    }
    for y in 0..height {
        let row = y * width..(y + 1) * width;
        line[..width].copy_from_slice(&distances[row.clone()]);
        distance_transform_1d(
            &line[..width],
            &mut distances[row],
            &mut sites[..width],
            &mut boundaries[..=width],
        );
    }

    distances
        .into_iter()
        .zip(source)
        .map(|(distance_squared, source_alpha)| {
            let distance = distance_squared.sqrt();
            let coverage = (radius + 1. - distance).clamp(0., 1.);
            ((coverage * 255.).round() as u8).max(*source_alpha)
        })
        .collect()
}

fn distance_transform_1d(
    source: &[f32],
    output: &mut [f32],
    sites: &mut [usize],
    boundaries: &mut [f32],
) {
    const INFINITY: f32 = 1.0e20;
    let Some(first) = source.iter().position(|value| *value < INFINITY) else {
        output.fill(INFINITY);
        return;
    };

    let mut envelope_end = 0;
    sites[0] = first;
    boundaries[0] = f32::NEG_INFINITY;
    boundaries[1] = f32::INFINITY;

    for site in first + 1..source.len() {
        if source[site] >= INFINITY {
            continue;
        }
        let mut intersection;
        loop {
            let previous = sites[envelope_end];
            intersection = ((source[site] + (site * site) as f32)
                - (source[previous] + (previous * previous) as f32))
                / (2. * (site - previous) as f32);
            if intersection > boundaries[envelope_end] || envelope_end == 0 {
                break;
            }
            envelope_end -= 1;
        }
        envelope_end += 1;
        sites[envelope_end] = site;
        boundaries[envelope_end] = intersection;
        boundaries[envelope_end + 1] = f32::INFINITY;
    }

    let mut envelope = 0;
    for (position, output) in output.iter_mut().enumerate() {
        while boundaries[envelope + 1] < position as f32 {
            envelope += 1;
        }
        let delta = position.abs_diff(sites[envelope]);
        *output = (delta * delta) as f32 + source[sites[envelope]];
    }
}

fn colorize(fill: &[u8], outline: &[u8], color: [f32; 4], outline_color: [f32; 4]) -> Vec<u8> {
    let srgb_to_linear = |value: f32| {
        let value = value.clamp(0., 1.);
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let linear_to_srgb = |value: f32| {
        let value = value.max(0.);
        if value <= 0.0031308 {
            value * 12.92
        } else {
            1.055 * value.powf(1. / 2.4) - 0.055
        }
    };
    let fill_rgb = [
        srgb_to_linear(color[0]),
        srgb_to_linear(color[1]),
        srgb_to_linear(color[2]),
    ];
    let fill_opacity = color[3].clamp(0., 1.);
    let outline_rgb = [
        srgb_to_linear(outline_color[0]),
        srgb_to_linear(outline_color[1]),
        srgb_to_linear(outline_color[2]),
    ];
    let outline_opacity = outline_color[3].clamp(0., 1.);
    let mut rgba = Vec::with_capacity(fill.len() * 4);
    for (&fill, &outline) in fill.iter().zip(outline) {
        let fill_alpha = fill as f32 / 255. * fill_opacity;
        let outline_alpha = outline as f32 / 255. * outline_opacity;
        let behind_alpha = outline_alpha * (1. - fill_alpha);
        let alpha = fill_alpha + behind_alpha;
        let rgb = if alpha > 0. {
            [
                (fill_rgb[0] * fill_alpha + outline_rgb[0] * behind_alpha) / alpha,
                (fill_rgb[1] * fill_alpha + outline_rgb[1] * behind_alpha) / alpha,
                (fill_rgb[2] * fill_alpha + outline_rgb[2] * behind_alpha) / alpha,
            ]
        } else {
            [0.; 3]
        };
        rgba.extend(
            rgb.map(|component| (linear_to_srgb(component).clamp(0., 1.) * 255.).round() as u8),
        );
        rgba.push((alpha * 255.).round() as u8);
    }
    rgba
}
