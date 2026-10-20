use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use cosmic_text::{FontSystem, SwashCache};

use crate::engine::frame::VisualFrame;
use zerium_core::plugin::{MAX_DECIMAL_PLACES, TextStyle, TextureInput};
use zerium_core::property::{PropertyValue, PropertyValues};
use zerium_core::timeline::{EffectInstanceId, ItemId, TimelineItem};

use super::{RenderError, RenderSize};

mod raster;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TextSourceId {
    pub item_id: ItemId,
    pub effect_id: Option<EffectInstanceId>,
    pub capability_index: usize,
}

pub(crate) struct TextFrameRequest<'a> {
    pub id: TextSourceId,
    pub capability: &'a TextureInput,
    pub properties: &'a PropertyValues,
    pub label: &'a str,
    pub target_size: RenderSize,
}

#[derive(Clone, Debug, PartialEq)]
struct TextSignature {
    content: String,
    font_families: Vec<String>,
    font_size: f32,
    letter_spacing: f32,
    line_spacing: f32,
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
    frame: Arc<VisualFrame>,
    last_used: u64,
}

pub(super) struct TextFrameCache {
    font_system: FontSystem,
    swash_cache: SwashCache,
    frames: HashMap<TextSourceId, CachedTextFrame>,
    active: HashSet<TextSourceId>,
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
            font_system: gpui::new_font_system_with_locale(crate::i18n::locale()),
            swash_cache: SwashCache::new(),
            frames: HashMap::new(),
            active: HashSet::new(),
            byte_budget,
            resident_bytes: 0,
            clock: 0,
        }
    }

    /// Pins text sources on the active items and their effects.
    /// Call once before resolving a frame; unpinned entries remain reusable LRU entries.
    pub(crate) fn retain_active<'a>(&mut self, items: impl IntoIterator<Item = &'a TimelineItem>) {
        self.active.clear();
        for item in items {
            let owners = item
                .schema()
                .map(|schema| (None, schema.inputs()))
                .into_iter()
                .chain(
                    item.effects
                        .iter()
                        .map(|effect| (Some(effect.id), effect.schema().inputs())),
                );
            for (effect_id, capabilities) in owners {
                for (capability_index, capability) in capabilities.iter().enumerate() {
                    if capability.text_style().is_some() {
                        self.active.insert(TextSourceId {
                            item_id: item.id,
                            effect_id,
                            capability_index,
                        });
                    }
                }
            }
        }
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
                self.resident_bytes = self.resident_bytes.saturating_sub(evicted.frame.byte_len());
            }
        }
    }

    pub(crate) fn frame_for(
        &mut self,
        request: TextFrameRequest<'_>,
        composition_size: RenderSize,
    ) -> Result<Arc<VisualFrame>, RenderError> {
        let signature = TextSignature::from_request(&request, composition_size)?;
        let tick = self.next_tick();
        if let Some(cached) = self.frames.get_mut(&request.id)
            && cached.signature == signature
        {
            cached.last_used = tick;
            return Ok(cached.frame.clone());
        }
        let frame = Arc::new(raster::rasterize(
            &mut self.font_system,
            &mut self.swash_cache,
            &signature,
            self.byte_budget,
        )?);
        let bytes = frame.byte_len();
        if let Some(replaced) = self.frames.remove(&request.id) {
            self.resident_bytes = self
                .resident_bytes
                .saturating_sub(replaced.frame.byte_len());
        }
        self.resident_bytes = self.resident_bytes.saturating_add(bytes);
        self.frames.insert(
            request.id,
            CachedTextFrame {
                signature,
                frame: frame.clone(),
                last_used: tick,
            },
        );
        self.evict_to_budget();
        if self.resident_bytes > self.byte_budget
            && let Some(uncached) = self.frames.remove(&request.id)
        {
            self.resident_bytes = self
                .resident_bytes
                .saturating_sub(uncached.frame.byte_len());
        }
        Ok(frame)
    }
}

impl TextSignature {
    fn from_request(
        request: &TextFrameRequest<'_>,
        composition_size: RenderSize,
    ) -> Result<Self, RenderError> {
        let missing = || {
            RenderError::backend(format!(
                "text source '{}' has invalid properties",
                request.label
            ))
        };
        let TextStyle {
            size,
            font_family,
            font_size,
            letter_spacing,
            line_spacing,
            color,
            outline_width,
            outline_color,
            bold,
            italic,
            horizontal_alignment,
            vertical_alignment,
        } = request.capability.text_style().ok_or_else(missing)?;
        let property = |id: &str| request.properties.property(id);
        let string_array = |id: &str| {
            property(id)?
                .as_array()?
                .iter()
                .map(|element| element.value().as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
        };
        let pair = |id: &str| {
            let value = property(id)?;
            Some([
                value.scalar_at(Some(0))?.numeric_scalar()? as f32,
                value.scalar_at(Some(1))?.numeric_scalar()? as f32,
            ])
        };
        let content = match request.capability {
            TextureInput::Text { text, .. } => property(text)
                .and_then(PropertyValue::as_str)
                .ok_or_else(missing)?
                .to_owned(),
            TextureInput::Number {
                value,
                decimal_places,
                ..
            } => {
                let value = property(value)
                    .and_then(PropertyValue::numeric_scalar)
                    .ok_or_else(missing)?;
                let precision = property(decimal_places)
                    .and_then(PropertyValue::as_u32)
                    .filter(|precision| *precision <= MAX_DECIMAL_PLACES)
                    .ok_or_else(missing)? as usize;
                format!("{value:.precision$}")
            }
            _ => return Err(missing()),
        };
        Ok(Self {
            content,
            font_families: string_array(font_family).ok_or_else(missing)?,
            font_size: property(font_size)
                .and_then(PropertyValue::as_f32)
                .ok_or_else(missing)?,
            letter_spacing: property(letter_spacing)
                .and_then(PropertyValue::as_f32)
                .ok_or_else(missing)?,
            line_spacing: property(line_spacing)
                .and_then(PropertyValue::as_f32)
                .ok_or_else(missing)?,
            color: property(color)
                .and_then(PropertyValue::as_color)
                .ok_or_else(missing)?,
            outline_width: property(outline_width)
                .and_then(PropertyValue::as_f32)
                .ok_or_else(missing)?,
            outline_color: property(outline_color)
                .and_then(PropertyValue::as_color)
                .ok_or_else(missing)?,
            bold: property(bold)
                .and_then(PropertyValue::as_bool)
                .ok_or_else(missing)?,
            italic: property(italic)
                .and_then(PropertyValue::as_bool)
                .ok_or_else(missing)?,
            horizontal_alignment: property(horizontal_alignment)
                .and_then(PropertyValue::as_enum)
                .ok_or_else(missing)?,
            vertical_alignment: property(vertical_alignment)
                .and_then(PropertyValue::as_enum)
                .ok_or_else(missing)?,
            box_size: pair(size).ok_or_else(missing)?,
            target_size: request.target_size,
            composition_size,
        })
    }
}
