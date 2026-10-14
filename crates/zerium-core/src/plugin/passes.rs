//! Render, compute, and temporal passes and their sampling contracts.
use super::identifier::validate_wgsl_identifier;
use super::shader::validate_shader_module;
use super::{PluginError, ShaderKind, ShaderSchema};
use crate::property::{
    PropertyDefinition, PropertySchema, PropertyValue, PropertyValues, ScalarPropertyType,
    ScalarSchema, ValueSchema,
};
use serde::Deserialize;
use std::collections::BTreeMap;

const MAX_TEMPORAL_SAMPLES: u32 = 32;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EffectPassSchema {
    Render {
        shader: ShaderSchema,
        #[serde(default)]
        overrides: BTreeMap<String, f64>,
    },
    Compute {
        shader: ComputeShaderSchema,
        #[serde(default = "default_compute_dispatch")]
        dispatch: [ComputeDispatchDimension; 3],
        #[serde(default)]
        overrides: BTreeMap<String, f64>,
    },
    Temporal {
        sampling: TemporalSamplingSchema,
        reducer: ShaderSchema,
        #[serde(default)]
        overrides: BTreeMap<String, f64>,
    },
}

impl EffectPassSchema {
    pub const fn shader_kind(&self) -> ShaderKind {
        match self {
            Self::Render { .. } => ShaderKind::Effect,
            Self::Compute { .. } => ShaderKind::Compute,
            Self::Temporal { .. } => ShaderKind::Temporal,
        }
    }

    pub fn overrides(&self) -> &BTreeMap<String, f64> {
        match self {
            Self::Render { overrides, .. }
            | Self::Compute { overrides, .. }
            | Self::Temporal { overrides, .. } => overrides,
        }
    }

    pub fn shader_module(&self) -> &str {
        match self {
            Self::Render { shader, .. } => shader.module(),
            Self::Compute { shader, .. } => &shader.module,
            Self::Temporal { reducer, .. } => reducer.module(),
        }
    }

    pub(super) fn validate(
        &self,
        effect_id: &str,
        properties: &[PropertySchema],
        pass_index: usize,
    ) -> Result<(), PluginError> {
        for (key, value) in self.overrides() {
            if key.parse::<u16>().is_err() {
                validate_wgsl_identifier("pipeline override", key)?;
            }
            if !value.is_finite() {
                return Err(PluginError::invalid_definition(format!(
                    "effect '{effect_id}' pass {pass_index} override '{key}' must be finite"
                )));
            }
        }
        match self {
            Self::Render { shader, .. } => {
                shader.validate("render effect pass", effect_id)?;
            }
            Self::Compute { shader, .. } => {
                validate_shader_module("compute effect pass", effect_id, &shader.module)?;
                validate_wgsl_identifier("compute entry", &shader.entry)?;
            }
            Self::Temporal {
                sampling, reducer, ..
            } => {
                if pass_index != 0 {
                    return Err(PluginError::invalid_definition(format!(
                        "effect '{}' temporal pass must be first",
                        effect_id
                    )));
                }
                sampling.validate(effect_id, properties)?;
                reducer.validate("temporal effect pass", effect_id)?;
            }
        }
        Ok(())
    }
}

/// Selects source times independently of the shader that combines them.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TemporalSamplingSchema {
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
pub struct ComputeShaderSchema {
    module: String,
    #[serde(default = "default_compute_entry")]
    entry: String,
}

impl ComputeShaderSchema {
    pub fn entry(&self) -> &str {
        &self.entry
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ComputeDispatchDimension {
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

impl TemporalSamplingSchema {
    fn validate(&self, effect_id: &str, properties: &[PropertySchema]) -> Result<(), PluginError> {
        let scalar = |id: &str, ty: ScalarPropertyType| {
            properties
                .iter()
                .find(|property| property.id() == id)
                .filter(|property| property.scalar_type(None, None) == Some(&ty))
                .ok_or_else(|| {
                    PluginError::invalid_definition(format!(
                        "effect '{effect_id}' temporal property '{id}' has the wrong type"
                    ))
                })
        };
        match self {
            Self::Range {
                sample_count,
                start_offset,
                end_offset,
            } => {
                let count = scalar(sample_count, ScalarPropertyType::U32)?;
                scalar(start_offset, ScalarPropertyType::F32)?;
                scalar(end_offset, ScalarPropertyType::F32)?;
                let constraints = count.configuration_constraints(None);
                if !constraints.min.is_some_and(|min| min >= 1.)
                    || !constraints
                        .max
                        .is_some_and(|max| max <= f64::from(MAX_TEMPORAL_SAMPLES))
                {
                    return Err(PluginError::invalid_definition(format!(
                        "effect '{}' temporal sample count must be constrained to 1..={MAX_TEMPORAL_SAMPLES}",
                        effect_id
                    )));
                }
            }
            Self::Offsets { offsets } => {
                let valid = matches!(
                    properties.iter().find(|property| property.id() == offsets).map(|property| property.definition()),
                    Some(PropertyDefinition::Array {
                        element: ValueSchema::Scalar(ScalarSchema { ty: ScalarPropertyType::F32, .. }),
                        min_items,
                        max_items,
                        ..
                    }) if *min_items >= 1 && *max_items <= MAX_TEMPORAL_SAMPLES
                );
                if !valid {
                    return Err(PluginError::invalid_definition(format!(
                        "effect '{}' temporal offsets '{offsets}' must be a nonempty f32 array with at most {MAX_TEMPORAL_SAMPLES} entries",
                        effect_id
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn sample_offsets(&self, values: &PropertyValues) -> Option<Vec<f64>> {
        match self {
            Self::Range {
                sample_count,
                start_offset,
                end_offset,
            } => {
                let count = values.property(sample_count)?.as_u32()?;
                let start = values.property(start_offset)?.as_f32()?;
                let end = values.property(end_offset)?.as_f32()?;
                if !start.is_finite() || !end.is_finite() {
                    return None;
                }
                let count = count.clamp(1, MAX_TEMPORAL_SAMPLES);
                let start = f64::from(start);
                let span = f64::from(end) - start;
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
                let elements = values.property(offsets)?.as_array()?;
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
