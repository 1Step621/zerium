//! Effect schemas, render passes, and temporal sampling.
use self::passes::EffectPassSchema;
use super::{
    capability::{TextureInput, validate_texture_inputs},
    editor::{EditorCapability, validate_editors},
    validation::{validate_catalog_entry, validate_property_schemas},
};
use crate::{
    localized_text::LocalizedText,
    plugin::{PluginError, abi::PropertyLayout},
    property::PropertySchema,
};
use serde::{Deserialize, Deserializer, de::Error as _};

pub(in crate::plugin) mod passes;

#[derive(Clone, Debug, PartialEq)]
pub struct EffectSchema {
    id: String,
    label: LocalizedText,
    icon: String,
    category: EffectCategory,
    tags: Vec<String>,
    render: EffectRenderSchema,
    editor: Vec<EditorCapability>,
    property_layout: PropertyLayout,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EffectRenderSchema {
    #[serde(default = "default_effect_render_scale")]
    pub scale: u32,
    #[serde(default)]
    pub inputs: Vec<TextureInput>,
    pub passes: Vec<EffectPassSchema>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EffectSchemaDefinition {
    id: String,
    label: LocalizedText,
    icon: String,
    category: EffectCategory,
    #[serde(default)]
    tags: Vec<String>,
    render: EffectRenderSchema,
    #[serde(default)]
    editor: Vec<EditorCapability>,
    properties: Vec<PropertySchema>,
}

impl<'de> Deserialize<'de> for EffectSchema {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let definition = EffectSchemaDefinition::deserialize(deserializer)?;
        let property_layout =
            PropertyLayout::compile("effect", &definition.id, definition.properties)
                .map_err(D::Error::custom)?;
        let schema = Self {
            id: definition.id,
            label: definition.label,
            icon: definition.icon,
            category: definition.category,
            tags: definition.tags,
            render: definition.render,
            editor: definition.editor,
            property_layout,
        };
        schema.validate().map_err(D::Error::custom)?;
        Ok(schema)
    }
}

const fn default_effect_render_scale() -> u32 {
    1
}

impl EffectSchema {
    pub fn render(&self) -> &EffectRenderSchema {
        &self.render
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn label(&self) -> &str {
        self.label.resolve()
    }

    pub fn icon(&self) -> &str {
        &self.icon
    }

    pub fn category(&self) -> EffectCategory {
        self.category
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    pub fn editor(&self) -> &[EditorCapability] {
        &self.editor
    }

    pub fn aspect_lock_property(&self) -> Option<&PropertySchema> {
        let (property, _) = self.editor.iter().find_map(EditorCapability::aspect_lock)?;
        self.property(property)
    }

    pub fn aspect_lock_default(&self) -> bool {
        self.editor
            .iter()
            .find_map(EditorCapability::aspect_lock)
            .is_some_and(|(_, default)| default)
    }

    pub fn properties(&self) -> &[PropertySchema] {
        self.property_layout.properties()
    }

    pub fn file_properties(&self) -> impl Iterator<Item = &PropertySchema> {
        self.properties()
            .iter()
            .filter(|property| property.is_file())
    }

    pub fn file_property(&self, id: &str) -> Option<&PropertySchema> {
        self.property(id).filter(|property| property.is_file())
    }

    pub fn inputs(&self) -> &[TextureInput] {
        &self.render.inputs
    }

    pub fn media_sources(&self) -> impl Iterator<Item = crate::media::MediaSource<'_>> {
        self.render
            .inputs
            .iter()
            .filter_map(TextureInput::media_source)
    }

    pub fn property_layout(&self) -> &PropertyLayout {
        &self.property_layout
    }

    pub(in crate::plugin) fn validate(&self) -> Result<(), PluginError> {
        validate_catalog_entry("effect", &self.id, &self.label, &self.icon, &self.tags)?;
        validate_property_schemas("effect", &self.id, self.properties())?;
        validate_texture_inputs("effect", &self.id, self.properties(), &self.render.inputs)?;

        validate_editors("effect", &self.id, self.properties(), &self.editor)?;

        if !(1..=4).contains(&self.render.scale) {
            return Err(PluginError::invalid_definition(format!(
                "effect '{}' render.scale must be between 1 and 4",
                self.id
            )));
        }

        if self.render.passes.is_empty() {
            return Err(PluginError::invalid_definition(format!(
                "effect '{}' must define at least one pass",
                self.id
            )));
        }

        for (pass_index, pass) in self.render.passes.iter().enumerate() {
            pass.validate(&self.id, self.properties(), &self.render.inputs, pass_index)?;
        }
        Ok(())
    }

    pub fn property(&self, id: &str) -> Option<&PropertySchema> {
        self.properties().iter().find(|property| property.id == id)
    }
}

impl crate::plugin::PluginCatalogEntry for EffectSchema {
    type Category = EffectCategory;

    fn id(&self) -> &str {
        self.id()
    }

    fn label(&self) -> &str {
        self.label()
    }

    fn icon(&self) -> &str {
        self.icon()
    }

    fn category(&self) -> EffectCategory {
        self.category
    }

    fn tags(&self) -> &[String] {
        self.tags()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EffectCategory {
    Transform,
    Color,
    Style,
    Filter,
    Composite,
    Other,
}

impl EffectCategory {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Transform => "transform",
            Self::Color => "color",
            Self::Style => "style",
            Self::Filter => "filter",
            Self::Composite => "composite",
            Self::Other => "other",
        }
    }
}
