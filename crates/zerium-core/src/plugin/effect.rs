//! Effect schemas, render passes, and temporal sampling.

use crate::localized_text::LocalizedText;
use serde::{Deserialize, Deserializer, de::Error as _};

use super::CatalogCategory;
use super::OutputBoundsSchema;
use super::PluginError;
use super::abi::PropertyLayout;
use super::capability::{Capability, validate_capabilities};
use super::editor::{EditorCapability, validate_editors};
use super::passes::EffectPassSchema;
use super::validation::{validate_catalog_entry, validate_property_schemas};
use crate::property::PropertySchema;

/// Which rectangle the first render pass sees in `effect_input`.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EffectInputSpace {
    /// The input is composited into the effect's output rectangle first.
    #[default]
    Output,
    /// The input keeps its own rectangle for coordinate-based transforms.
    Source,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectSchema {
    id: String,
    label: LocalizedText,
    category: CatalogCategory,
    tags: Vec<String>,
    render_scale: u32,
    output_bounds: OutputBoundsSchema,
    input_space: EffectInputSpace,
    editor: Vec<EditorCapability>,
    capabilities: Vec<Capability>,
    properties: Vec<PropertySchema>,
    passes: Vec<EffectPassSchema>,
    property_abi: PropertyLayout,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EffectSchemaDefinition {
    id: String,
    label: LocalizedText,
    category: CatalogCategory,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default = "default_effect_render_scale")]
    render_scale: u32,
    output_bounds: OutputBoundsSchema,
    #[serde(default)]
    input_space: EffectInputSpace,
    #[serde(default)]
    editor: Vec<EditorCapability>,
    #[serde(default)]
    capabilities: Vec<Capability>,
    properties: Vec<PropertySchema>,
    passes: Vec<EffectPassSchema>,
}

impl<'de> Deserialize<'de> for EffectSchema {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let definition = EffectSchemaDefinition::deserialize(deserializer)?;
        let property_abi = PropertyLayout::compile(
            "effect",
            &definition.id,
            definition
                .properties
                .iter()
                .map(|property| (property.id(), property.ty())),
        )
        .map_err(D::Error::custom)?;
        let schema = Self {
            id: definition.id,
            label: definition.label,
            category: definition.category,
            tags: definition.tags,
            render_scale: definition.render_scale,
            output_bounds: definition.output_bounds,
            input_space: definition.input_space,
            editor: definition.editor,
            capabilities: definition.capabilities,
            properties: definition.properties,
            passes: definition.passes,
            property_abi,
        };
        schema.validate().map_err(D::Error::custom)?;
        Ok(schema)
    }
}

const fn default_effect_render_scale() -> u32 {
    1
}

impl EffectSchema {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn label(&self) -> &str {
        self.label.resolve()
    }

    pub fn category(&self) -> &str {
        self.category.label()
    }

    pub fn category_id(&self) -> &str {
        self.category.id()
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    pub const fn render_scale(&self) -> u32 {
        self.render_scale
    }

    pub fn output_bounds(&self) -> &OutputBoundsSchema {
        &self.output_bounds
    }

    pub const fn input_space(&self) -> EffectInputSpace {
        self.input_space
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
        &self.properties
    }

    pub fn file_properties(&self) -> impl Iterator<Item = &PropertySchema> {
        self.properties.iter().filter(|property| property.is_file())
    }

    pub fn file_property(&self, id: &str) -> Option<&PropertySchema> {
        self.property(id).filter(|property| property.is_file())
    }

    pub fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }

    pub fn media_sources(&self) -> impl Iterator<Item = crate::media::MediaSource<'_>> {
        self.capabilities
            .iter()
            .filter_map(Capability::media_source)
    }

    pub fn property_layout(&self) -> &PropertyLayout {
        &self.property_abi
    }

    pub fn passes(&self) -> &[EffectPassSchema] {
        &self.passes
    }

    pub(super) fn validate(&self) -> Result<(), PluginError> {
        self.category.validate("effect", &self.id)?;
        validate_catalog_entry("effect", &self.id, &self.label, &self.tags)?;
        validate_property_schemas("effect", &self.id, &self.properties)?;
        self.output_bounds
            .validate("effect", &self.id, &self.properties)?;
        validate_capabilities("effect", &self.id, &self.properties, &self.capabilities)?;
        validate_editors("effect", &self.id, &self.properties, &self.editor)?;
        if self.input_space == EffectInputSpace::Source
            && (self.passes.len() != 1
                || !matches!(self.passes[0], EffectPassSchema::Render { .. }))
        {
            return Err(PluginError::invalid_definition(format!(
                "effect '{}' source input space requires one render pass",
                self.id
            )));
        }
        if !(1..=4).contains(&self.render_scale) {
            return Err(PluginError::invalid_definition(format!(
                "effect '{}' render_scale must be between 1 and 4",
                self.id
            )));
        }
        if self.passes.is_empty() {
            return Err(PluginError::invalid_definition(format!(
                "effect '{}' must define at least one pass",
                self.id
            )));
        }
        for (pass_index, pass) in self.passes.iter().enumerate() {
            pass.validate(&self.id, &self.properties, pass_index)?;
        }
        Ok(())
    }

    pub fn property(&self, id: &str) -> Option<&PropertySchema> {
        self.properties.iter().find(|property| property.id == id)
    }
}

impl super::PluginCatalogEntry for EffectSchema {
    fn id(&self) -> &str {
        self.id()
    }

    fn label(&self) -> &str {
        self.label()
    }

    fn category(&self) -> &str {
        self.category()
    }

    fn category_id(&self) -> &str {
        self.category_id()
    }

    fn tags(&self) -> &[String] {
        self.tags()
    }
}
