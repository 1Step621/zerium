//! Effect schemas, render passes, and temporal sampling.

use serde::{Deserialize, Deserializer, de::Error as _};

use super::PluginError;
use super::abi::PropertyLayout;
use super::capability::{Capability, FileCapability, validate_capabilities};
use super::identifier::validate_wgsl_identifier;
use super::shader::{ShaderKind, ShaderSchema, validate_shader_module};
use super::validation::{validate_catalog_entry, validate_property_schemas};
use crate::domain::property::{
    PropertySchema, PropertyType, PropertyValue, PropertyValueType, PropertyValues,
    ScalarPropertyType,
};

const MAX_TEMPORAL_SAMPLES: u32 = 32;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EffectSchema {
    id: String,
    label: String,
    category: String,
    tags: Vec<String>,
    render_scale: u32,
    uses_effect_bounds: bool,
    capabilities: Vec<Capability>,
    properties: Vec<PropertySchema>,
    passes: Vec<EffectPassSchema>,
    property_abi: PropertyLayout,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EffectSchemaDefinition {
    id: String,
    label: String,
    category: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default = "default_effect_render_scale")]
    render_scale: u32,
    #[serde(default)]
    uses_effect_bounds: bool,
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
            uses_effect_bounds: definition.uses_effect_bounds,
            capabilities: definition.capabilities,
            properties: definition.properties,
            passes: definition.passes,
            property_abi,
        };
        schema.validate().map_err(D::Error::custom)?;
        Ok(schema)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum EffectPassSchema {
    Render {
        shader: ShaderSchema,
        #[serde(default)]
        constants: Vec<PassConstantSchema>,
    },
    Compute {
        shader: ComputeShaderSchema,
        #[serde(default = "default_compute_dispatch")]
        dispatch: [ComputeDispatchDimension; 3],
        #[serde(default)]
        constants: Vec<PassConstantSchema>,
    },
    Temporal {
        sampling: TemporalSamplingSchema,
        reducer: ShaderSchema,
        #[serde(default)]
        constants: Vec<PassConstantSchema>,
    },
}

impl EffectPassSchema {
    pub(crate) const fn shader_kind(&self) -> ShaderKind {
        match self {
            Self::Render { .. } => ShaderKind::Effect,
            Self::Compute { .. } => ShaderKind::Compute,
            Self::Temporal { .. } => ShaderKind::Temporal,
        }
    }

    pub(crate) fn constants(&self) -> &[PassConstantSchema] {
        match self {
            Self::Render { constants, .. }
            | Self::Compute { constants, .. }
            | Self::Temporal { constants, .. } => constants,
        }
    }

    pub(crate) fn shader_module(&self) -> &str {
        match self {
            Self::Render { shader, .. } => shader.module(),
            Self::Compute { shader, .. } => &shader.module,
            Self::Temporal { reducer, .. } => reducer.module(),
        }
    }

    pub(crate) fn temporal_sample_offsets(&self, values: &PropertyValues) -> Option<Vec<f64>> {
        let Self::Temporal { sampling, .. } = self else {
            return None;
        };
        sampling.sample_offsets(values)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PassConstantSchema {
    pub(crate) id: String,
    pub(crate) value: PassConstantValue,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PassConstantValue {
    F32(f32),
    I32(i32),
    U32(u32),
    Bool(bool),
}

/// Selects source times independently of the shader that combines them.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum TemporalSamplingSchema {
    Range {
        sample_count: String,
        start_offset: String,
        end_offset: String,
    },
    Offsets {
        offsets: String,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ComputeShaderSchema {
    module: String,
    #[serde(default = "default_compute_entry")]
    entry: String,
}

impl ComputeShaderSchema {
    pub(crate) fn entry(&self) -> &str {
        &self.entry
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ComputeDispatchDimension {
    Width,
    Height,
    MaxDimension,
    One,
}

fn default_compute_dispatch() -> [ComputeDispatchDimension; 3] {
    [
        ComputeDispatchDimension::Width,
        ComputeDispatchDimension::Height,
        ComputeDispatchDimension::One,
    ]
}

fn default_compute_entry() -> String {
    "compute_main".to_owned()
}

const fn default_effect_render_scale() -> u32 {
    1
}

impl EffectSchema {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    pub(crate) fn category(&self) -> &str {
        &self.category
    }

    pub(crate) fn tags(&self) -> &[String] {
        &self.tags
    }

    pub(crate) const fn render_scale(&self) -> u32 {
        self.render_scale
    }

    pub(crate) const fn uses_effect_bounds(&self) -> bool {
        self.uses_effect_bounds
    }

    pub(crate) fn properties(&self) -> &[PropertySchema] {
        &self.properties
    }

    pub(crate) fn files(&self) -> impl Iterator<Item = &FileCapability> {
        self.capabilities.iter().filter_map(Capability::media_file)
    }

    pub(crate) fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }

    pub(crate) fn property_layout(&self) -> &PropertyLayout {
        &self.property_abi
    }

    pub(crate) fn passes(&self) -> &[EffectPassSchema] {
        &self.passes
    }

    pub(super) fn validate(&self) -> Result<(), PluginError> {
        validate_catalog_entry("effect", &self.id, &self.label, &self.category, &self.tags)?;
        if !(1..=4).contains(&self.render_scale) {
            return Err(PluginError::invalid_definition(format!(
                "effect '{}' render_scale must be between 1 and 4",
                self.id
            )));
        }
        validate_property_schemas("effect", &self.id, &self.properties)?;
        validate_capabilities("effect", &self.id, &self.properties, &self.capabilities)?;
        if self.passes.is_empty() {
            return Err(PluginError::invalid_definition(format!(
                "effect '{}' must define at least one pass",
                self.id
            )));
        }
        for (pass_index, pass) in self.passes.iter().enumerate() {
            let mut constant_ids = std::collections::HashSet::new();
            for constant in pass.constants() {
                validate_wgsl_identifier("pass constant", &constant.id)?;
                if matches!(constant.value, PassConstantValue::F32(value) if !value.is_finite()) {
                    return Err(PluginError::invalid_definition(format!(
                        "effect '{}' pass {pass_index} constant '{}' must be finite",
                        self.id, constant.id
                    )));
                }
                if !constant_ids.insert(&constant.id) {
                    return Err(PluginError::invalid_definition(format!(
                        "effect '{}' pass {pass_index} has duplicate constant ID '{}'",
                        self.id, constant.id
                    )));
                }
            }
            match pass {
                EffectPassSchema::Render { shader, .. } => {
                    shader.validate("render effect pass", &self.id)?;
                }
                EffectPassSchema::Compute { shader, .. } => {
                    validate_shader_module("compute effect pass", &self.id, &shader.module)?;
                    validate_wgsl_identifier("compute entry", &shader.entry)?;
                }
                EffectPassSchema::Temporal {
                    sampling, reducer, ..
                } => {
                    if pass_index != 0 {
                        return Err(PluginError::invalid_definition(format!(
                            "effect '{}' temporal pass must be first",
                            self.id
                        )));
                    }
                    sampling.validate(self)?;
                    reducer.validate("temporal effect pass", &self.id)?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn property(&self, id: &str) -> Option<&PropertySchema> {
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

    fn tags(&self) -> &[String] {
        self.tags()
    }
}

impl TemporalSamplingSchema {
    fn validate(&self, effect: &EffectSchema) -> Result<(), PluginError> {
        let scalar = |id: &str, ty: ScalarPropertyType| {
            (effect.property(id).map(|property| property.ty())
                == Some(&PropertyType::Value(PropertyValueType::Scalar(ty))))
            .then_some(())
            .ok_or_else(|| {
                PluginError::invalid_definition(format!(
                    "effect '{}' temporal property '{id}' has the wrong type",
                    effect.id
                ))
            })
        };
        match self {
            Self::Range {
                sample_count,
                start_offset,
                end_offset,
            } => {
                scalar(sample_count, ScalarPropertyType::U32)?;
                scalar(start_offset, ScalarPropertyType::F32)?;
                scalar(end_offset, ScalarPropertyType::F32)?;
                let constraints = effect
                    .property(sample_count)
                    .expect("sample count type was checked")
                    .configuration_constraints(None);
                if !constraints.min.is_some_and(|min| min >= 1.)
                    || !constraints
                        .max
                        .is_some_and(|max| max <= f64::from(MAX_TEMPORAL_SAMPLES))
                {
                    return Err(PluginError::invalid_definition(format!(
                        "effect '{}' temporal sample count must be constrained to 1..={MAX_TEMPORAL_SAMPLES}",
                        effect.id
                    )));
                }
            }
            Self::Offsets { offsets } => {
                let valid = matches!(
                    effect.property(offsets).map(|property| property.ty()),
                    Some(PropertyType::Array {
                        element_type: PropertyValueType::Scalar(ScalarPropertyType::F32),
                        min_items,
                        max_items,
                    }) if *min_items >= 1 && *max_items <= MAX_TEMPORAL_SAMPLES
                );
                if !valid {
                    return Err(PluginError::invalid_definition(format!(
                        "effect '{}' temporal offsets '{offsets}' must be a nonempty f32 array with at most {MAX_TEMPORAL_SAMPLES} entries",
                        effect.id
                    )));
                }
            }
        }
        Ok(())
    }

    fn sample_offsets(&self, values: &PropertyValues) -> Option<Vec<f64>> {
        match self {
            Self::Range {
                sample_count,
                start_offset,
                end_offset,
            } => {
                let PropertyValue::U32(count) = values.property(sample_count)? else {
                    return None;
                };
                let PropertyValue::F32(start) = values.property(start_offset)? else {
                    return None;
                };
                let PropertyValue::F32(end) = values.property(end_offset)? else {
                    return None;
                };
                if !start.is_finite() || !end.is_finite() {
                    return None;
                }
                let count = (*count).clamp(1, MAX_TEMPORAL_SAMPLES);
                let start = f64::from(*start);
                let span = f64::from(*end) - start;
                if span == 0. {
                    return Some(vec![start]);
                }
                Some(
                    (0..count)
                        .map(|index| start + (f64::from(index) + 0.5) * span / f64::from(count))
                        .collect(),
                )
            }
            Self::Offsets { offsets } => {
                let PropertyValue::Array(elements) = values.property(offsets)? else {
                    return None;
                };
                elements
                    .iter()
                    .map(|element| match element.value() {
                        PropertyValue::F32(value) if value.is_finite() => Some(f64::from(*value)),
                        _ => None,
                    })
                    .collect()
            }
        }
    }
}
