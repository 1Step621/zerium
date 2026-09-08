use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use cosmic_text::{
    Align, Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, Stretch, Style, SwashCache,
    Weight, Wrap, fontdb,
};

use crate::domain::parameter::ParameterValue;
use crate::domain::plugin::{ItemSchema, VisualCapability};
use crate::domain::timeline::{ItemId, TimelineItem};
use crate::engine::frame::RgbaFrame;

use super::{RenderError, RenderSize};

#[derive(Clone, Debug, PartialEq)]
struct TextSignature {
    content: String,
    font_families: Vec<String>,
    font_size: f32,
    color: [f32; 4],
    outline_width: f32,
    outline_color: [f32; 4],
    bold: bool,
    italic: bool,
    horizontal_alignment: u32,
    vertical_alignment: u32,
    box_size: [f32; 2],
    target_size: RenderSize,
    composition_size: RenderSize,
}

struct CachedTextFrame {
    signature: TextSignature,
    frame: Arc<RgbaFrame>,
    bytes: usize,
    last_used: u64,
}

pub(crate) struct TextFrameCache {
    font_system: FontSystem,
    swash_cache: SwashCache,
    frames: HashMap<ItemId, CachedTextFrame>,
    active: HashSet<ItemId>,
    byte_budget: usize,
    resident_bytes: usize,
    clock: u64,
}

impl TextFrameCache {
    pub(crate) const DEFAULT_BYTE_BUDGET: usize = 128 * 1024 * 1024;

    pub(crate) fn new() -> Self {
        Self::with_byte_budget(Self::DEFAULT_BYTE_BUDGET)
    }

    pub(crate) fn with_byte_budget(byte_budget: usize) -> Self {
        Self {
            font_system: FontSystem::new(),
            swash_cache: SwashCache::new(),
            frames: HashMap::new(),
            active: HashSet::new(),
            byte_budget,
            resident_bytes: 0,
            clock: 0,
        }
    }

    /// Pins only the text items participating in the current render graph.
    /// Call once before resolving a frame; unpinned entries remain reusable LRU entries.
    pub(crate) fn retain_active(&mut self, item_ids: impl IntoIterator<Item = ItemId>) {
        self.active.clear();
        self.active.extend(item_ids);
        self.evict_to_budget();
    }

    fn next_tick(&mut self) -> u64 {
        self.clock = self.clock.wrapping_add(1).max(1);
        self.clock
    }

    fn evict_to_budget(&mut self) {
        while self.resident_bytes > self.byte_budget {
            let candidate = self
                .frames
                .iter()
                .filter(|(id, cached)| {
                    !self.active.contains(id) && Arc::strong_count(&cached.frame) == 1
                })
                .min_by_key(|(_, cached)| cached.last_used)
                .map(|(id, _)| *id);
            let Some(candidate) = candidate else {
                break;
            };
            if let Some(evicted) = self.frames.remove(&candidate) {
                self.resident_bytes = self.resident_bytes.saturating_sub(evicted.bytes);
            }
        }
    }

    pub(crate) fn frame_for(
        &mut self,
        item: &TimelineItem,
        schema: &ItemSchema,
        target_size: RenderSize,
        composition_size: RenderSize,
    ) -> Result<Arc<RgbaFrame>, RenderError> {
        let signature = Self::signature(item, schema, target_size, composition_size)?;
        let tick = self.next_tick();
        if let Some(cached) = self.frames.get_mut(&item.id)
            && cached.signature == signature
        {
            cached.last_used = tick;
            return Ok(cached.frame.clone());
        }
        let frame = Arc::new(self.rasterize(&signature)?);
        let bytes = frame.rgba.len();
        if bytes > self.byte_budget {
            return Ok(frame);
        }
        if let Some(replaced) = self.frames.remove(&item.id) {
            self.resident_bytes = self.resident_bytes.saturating_sub(replaced.bytes);
        }
        self.resident_bytes = self.resident_bytes.saturating_add(bytes);
        self.frames.insert(
            item.id,
            CachedTextFrame {
                signature,
                frame: frame.clone(),
                bytes,
                last_used: tick,
            },
        );
        self.evict_to_budget();
        if self.resident_bytes > self.byte_budget
            && let Some(uncached) = self.frames.remove(&item.id)
        {
            self.resident_bytes = self.resident_bytes.saturating_sub(uncached.bytes);
        }
        Ok(frame)
    }

    fn named_parameter<'a>(
        item: &'a TimelineItem,
        schema: &ItemSchema,
        id: &str,
    ) -> Option<&'a ParameterValue> {
        schema.parameter(id)?;
        item.parameters.get(id)
    }

    fn signature(
        item: &TimelineItem,
        schema: &ItemSchema,
        target_size: RenderSize,
        composition_size: RenderSize,
    ) -> Result<TextSignature, RenderError> {
        let missing_binding = || {
            let item_label = item
                .intrinsic_label()
                .unwrap_or_else(|| schema.label().to_owned());
            RenderError::backend(format!(
                "text item '{item_label}' has no text parameter binding"
            ))
        };
        let Some(VisualCapability::Text {
            size,
            text,
            font_family,
            font_size,
            color,
            outline_width,
            outline_color,
            bold,
            italic,
            horizontal_alignment,
            vertical_alignment,
            ..
        }) = schema.visual()
        else {
            return Err(missing_binding());
        };
        let string = |id: &str| match Self::named_parameter(item, schema, id) {
            Some(ParameterValue::String(value)) => Some(value.clone()),
            _ => None,
        };
        let string_array = |id: &str| {
            let ParameterValue::Array(values) = Self::named_parameter(item, schema, id)? else {
                return None;
            };
            values
                .iter()
                .map(|value| match value {
                    ParameterValue::String(value) => Some(value.clone()),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()
        };
        let f32_value = |id: &str| match Self::named_parameter(item, schema, id) {
            Some(ParameterValue::F32(value)) => Some(*value),
            _ => None,
        };
        let bool_value = |id: &str| match Self::named_parameter(item, schema, id) {
            Some(ParameterValue::Bool(value)) => Some(*value),
            _ => None,
        };

        let vec4 = |id: &str| match Self::named_parameter(item, schema, id) {
            Some(ParameterValue::Color(value)) => Some(*value),
            _ => None,
        };
        let pair = |id: &str| {
            let value = Self::named_parameter(item, schema, id)?;
            Some([
                value.scalar_at(Some(0))?.numeric_scalar()? as f32,
                value.scalar_at(Some(1))?.numeric_scalar()? as f32,
            ])
        };
        let missing = || {
            let item_label = item
                .intrinsic_label()
                .unwrap_or_else(|| schema.label().to_owned());
            RenderError::backend(format!("text item '{item_label}' has invalid parameters"))
        };
        Ok(TextSignature {
            content: string(text).ok_or_else(missing)?,
            font_families: string_array(font_family).ok_or_else(missing)?,
            font_size: f32_value(font_size).ok_or_else(missing)?,
            color: vec4(color).ok_or_else(missing)?,
            outline_width: f32_value(outline_width).ok_or_else(missing)?,
            outline_color: vec4(outline_color).ok_or_else(missing)?,
            bold: bool_value(bold).ok_or_else(missing)?,
            italic: bool_value(italic).ok_or_else(missing)?,
            horizontal_alignment: match Self::named_parameter(item, schema, horizontal_alignment) {
                Some(ParameterValue::Enum(value)) => *value,
                _ => return Err(missing()),
            },
            vertical_alignment: match Self::named_parameter(item, schema, vertical_alignment) {
                Some(ParameterValue::Enum(value)) => *value,
                _ => return Err(missing()),
            },
            box_size: pair(size).ok_or_else(missing)?,
            target_size,
            composition_size,
        })
    }

    fn rasterize(&mut self, signature: &TextSignature) -> Result<RgbaFrame, RenderError> {
        let scale_x = signature.target_size.width as f32 / signature.composition_size.width as f32;
        let scale_y =
            signature.target_size.height as f32 / signature.composition_size.height as f32;
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
                    .is_some_and(|bytes| bytes <= self.byte_budget)
            })
            .ok_or_else(|| {
                RenderError::resource_limit(format!(
                    "text texture exceeds the {} byte cache working-set limit",
                    self.byte_budget
                ))
            })?;
        let font_size = (signature.font_size * uniform_scale).max(1.);
        let metrics = Metrics::new(font_size, font_size * 1.2);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        let primary_family = signature
            .font_families
            .first()
            .map(String::as_str)
            .unwrap_or_default();
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
        let attrs = Self::attrs_for_family(&self.font_system, primary_family, weight, style);
        let font_runs = Self::font_runs(
            &mut self.font_system,
            &signature.content,
            &signature.font_families,
            weight,
            style,
        );
        let run_attrs = font_runs
            .iter()
            .map(|(_, family_index)| {
                Self::attrs_for_family(
                    &self.font_system,
                    signature.font_families[*family_index].as_str(),
                    weight,
                    style,
                )
            })
            .collect::<Vec<_>>();
        let alignment = match signature.horizontal_alignment {
            1 => Align::Center,
            2 => Align::Right,
            _ => Align::Left,
        };
        let mut coverage = vec![0_u8; pixel_count];
        {
            let mut borrowed = buffer.borrow_with(&mut self.font_system);
            borrowed.set_size(Some(width as f32), Some(height as f32));
            borrowed.set_wrap(Wrap::WordOrGlyph);
            if font_runs.is_empty() {
                borrowed.set_text(
                    &signature.content,
                    &attrs,
                    Shaping::Advanced,
                    Some(alignment),
                );
            } else {
                borrowed.set_rich_text(
                    font_runs.iter().zip(&run_attrs).map(|((range, _), attrs)| {
                        (&signature.content[range.clone()], attrs.clone())
                    }),
                    &attrs,
                    Shaping::Advanced,
                    Some(alignment),
                );
            }
            borrowed.shape_until_scroll(false);
            borrowed.draw(
                &mut self.swash_cache,
                Color::rgb(255, 255, 255),
                |x, y, glyph_width, glyph_height, color| {
                    for offset_y in 0..glyph_height as i32 {
                        let pixel_y = y + offset_y;
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
        Self::align_vertically(
            &mut coverage,
            width as usize,
            height as usize,
            signature.vertical_alignment,
        );
        let outline_radius = (signature.outline_width.max(0.) * uniform_scale).min(64.);
        let outline =
            Self::outline_from_distance(&coverage, width as usize, height as usize, outline_radius);
        let rgba = Self::colorize(
            &coverage,
            &outline,
            signature.color,
            signature.outline_color,
        );
        Ok(RgbaFrame {
            width,
            height,
            rgba: rgba.into(),
        })
    }

    fn family(name: &str) -> Family<'_> {
        match name.trim().to_ascii_lowercase().as_str() {
            "" | "sans-serif" | "sans serif" => Family::SansSerif,
            "serif" => Family::Serif,
            "monospace" => Family::Monospace,
            _ => Family::Name(name.trim()),
        }
    }

    fn attrs_for_family<'a>(
        font_system: &FontSystem,
        name: &'a str,
        requested_weight: Weight,
        style: Style,
    ) -> Attrs<'a> {
        let family = Self::family(name);
        let resolved_weight = font_system
            .db()
            .query(&fontdb::Query {
                families: &[family],
                weight: requested_weight,
                stretch: Stretch::Normal,
                style,
            })
            .and_then(|id| font_system.db().face(id))
            .map_or(requested_weight, |face| face.weight);
        Attrs::new()
            .family(family)
            .weight(resolved_weight)
            .style(style)
    }

    fn font_runs(
        font_system: &mut FontSystem,
        text: &str,
        families: &[String],
        weight: Weight,
        style: Style,
    ) -> Vec<(std::ops::Range<usize>, usize)> {
        if text.is_empty() || families.len() < 2 {
            return Vec::new();
        }
        let mut runs = Vec::new();
        let mut current_family = 0;
        let mut run_start = 0;
        for (index, character) in text.char_indices() {
            let family = families
                .iter()
                .position(|family| {
                    Self::family_supports(font_system, family, character, weight, style)
                })
                .unwrap_or(0);
            if index > run_start && family != current_family {
                runs.push((run_start..index, current_family));
                run_start = index;
            }
            current_family = family;
        }
        runs.push((run_start..text.len(), current_family));
        runs
    }

    fn family_supports(
        font_system: &mut FontSystem,
        family_name: &str,
        character: char,
        weight: Weight,
        style: Style,
    ) -> bool {
        if character.is_whitespace() {
            return true;
        }
        let family = Self::family(family_name);
        let Some(font_id) = font_system.db().query(&fontdb::Query {
            families: &[family],
            weight,
            stretch: Stretch::Normal,
            style,
        }) else {
            return false;
        };
        font_system
            .get_font(font_id, weight)
            .is_some_and(|font| font.as_swash().charmap().map(character) != 0)
    }

    fn align_vertically(coverage: &mut [u8], width: usize, height: usize, alignment: u32) {
        let Some(first) = (0..height).find(|y| {
            coverage[y * width..(y + 1) * width]
                .iter()
                .any(|alpha| *alpha > 0)
        }) else {
            return;
        };
        let last = (first..height)
            .rev()
            .find(|y| {
                coverage[y * width..(y + 1) * width]
                    .iter()
                    .any(|alpha| *alpha > 0)
            })
            .unwrap_or(first);
        let content_height = last - first + 1;
        let target = match alignment {
            1 => height.saturating_sub(content_height) / 2,
            2 => height.saturating_sub(content_height),
            _ => 0,
        };
        if target == first {
            return;
        }
        let original = coverage.to_vec();
        coverage.fill(0);
        for source_y in first..=last {
            let destination_y = target + source_y - first;
            if destination_y >= height {
                break;
            }
            coverage[destination_y * width..(destination_y + 1) * width]
                .copy_from_slice(&original[source_y * width..(source_y + 1) * width]);
        }
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
        let mut intermediate = vec![INFINITY; source.len()];
        let scratch_len = width.max(height);
        let mut line = vec![INFINITY; scratch_len];
        let mut transformed = vec![INFINITY; scratch_len];
        let mut sites = vec![0_usize; scratch_len];
        let mut boundaries = vec![0_f32; scratch_len + 1];

        for x in 0..width {
            for y in 0..height {
                line[y] = distances[y * width + x];
            }
            Self::distance_transform_1d(
                &line[..height],
                &mut transformed[..height],
                &mut sites[..height],
                &mut boundaries[..=height],
            );
            for y in 0..height {
                intermediate[y * width + x] = transformed[y];
            }
        }
        for y in 0..height {
            let row = y * width..(y + 1) * width;
            Self::distance_transform_1d(
                &intermediate[row.clone()],
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
}
