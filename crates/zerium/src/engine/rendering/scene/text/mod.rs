use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};

use cosmic_text::{FontSystem, SwashCache};

use crate::engine::frame::{FrameCache, RASTER_CACHE_BYTES, VisualFrame};
use zerium_core::plugin::{MAX_DECIMAL_PLACES, TextStyle, TextureInput};
use zerium_core::property::{PropertyValue, PropertyValues};

use super::{RenderError, RenderSize};

mod raster;

pub(crate) struct TextFrameRequest<'a> {
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

pub(super) struct TextFrameCache {
    font_system: FontSystem,
    swash_cache: SwashCache,
    // Pixel identity is determined entirely by content, style and raster size.
    frames: FrameCache<TextSignature>,
}

impl TextFrameCache {
    pub(crate) fn new() -> Self {
        Self::with_byte_budget(RASTER_CACHE_BYTES)
    }

    pub(crate) fn with_byte_budget(byte_budget: usize) -> Self {
        Self {
            font_system: gpui::new_font_system_with_locale(crate::i18n::locale()),
            swash_cache: SwashCache::new(),
            frames: FrameCache::new(byte_budget),
        }
    }

    pub(crate) fn frame_for(
        &mut self,
        request: TextFrameRequest<'_>,
        composition_size: RenderSize,
    ) -> Result<Arc<VisualFrame>, RenderError> {
        let signature = TextSignature::from_request(&request, composition_size)?;
        if let Some(frame) = self.frames.get(&signature) {
            return Ok(frame);
        }
        let frame = Arc::new(raster::rasterize(
            &mut self.font_system,
            &mut self.swash_cache,
            &signature,
            self.frames.byte_budget(),
        )?);
        self.frames.insert(signature, frame.clone());
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
        let signature = Self {
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
        };
        if signature
            .scalars()
            .into_iter()
            .any(|value| !value.is_finite())
        {
            return Err(missing());
        }
        Ok(signature)
    }

    fn scalars(&self) -> [f32; 14] {
        [
            self.font_size,
            self.letter_spacing,
            self.line_spacing,
            self.outline_width,
            self.color[0],
            self.color[1],
            self.color[2],
            self.color[3],
            self.outline_color[0],
            self.outline_color[1],
            self.outline_color[2],
            self.outline_color[3],
            self.box_size[0],
            self.box_size[1],
        ]
    }
}

// Construction rejects nonfinite values. Hashing canonicalizes signed zero,
// matching the numeric equality used by the raster signature.
impl Eq for TextSignature {}

impl Hash for TextSignature {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let Self {
            content,
            font_families,
            font_size: _,
            letter_spacing: _,
            line_spacing: _,
            color: _,
            outline_width: _,
            outline_color: _,
            bold,
            italic,
            horizontal_alignment,
            vertical_alignment,
            box_size: _,
            target_size,
            composition_size,
        } = self;
        content.hash(state);
        font_families.hash(state);
        for value in self.scalars() {
            (if value == 0. { 0 } else { value.to_bits() }).hash(state);
        }
        bold.hash(state);
        italic.hash(state);
        horizontal_alignment.hash(state);
        vertical_alignment.hash(state);
        target_size.hash(state);
        composition_size.hash(state);
    }
}
