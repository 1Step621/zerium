//! Render, compute, and temporal pass declarations.

use crate::{
    plugin::{
        PluginError, ShaderSchema,
        schema::{validate_shader_module, validation::validate_wgsl_identifier},
    },
    property::{PropertySchema, PropertyValue, PropertyValues, ScalarPropertyType},
    timeline::FrameDuration,
};
use serde::Deserialize;
use std::collections::BTreeMap;

const MAX_TEMPORAL_SAMPLES: u32 = 32;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EffectPassSchema {
    Render {
        shader: ShaderSchema,
        bounds: crate::plugin::OutputBoundsSchema,
        #[serde(default)]
        overrides: BTreeMap<String, f64>,
    },
    Compute {
        shader: ComputeShaderSchema,
        bounds: crate::plugin::OutputBoundsSchema,
        #[serde(default = "default_compute_dispatch")]
        dispatch: [ComputeDispatchDimension; 3],
        #[serde(default)]
        overrides: BTreeMap<String, f64>,
    },
    /// Forward one upstream image without a shader or an intermediate texture.
    Sample { sampling: SampleTimeSchema },
    Temporal {
        sampling: TemporalSamplingSchema,
        reducer: ShaderSchema,
        bounds: crate::plugin::OutputBoundsSchema,
        #[serde(default)]
        overrides: BTreeMap<String, f64>,
    },
}

impl EffectPassSchema {
    pub fn bounds(&self) -> Option<&crate::plugin::OutputBoundsSchema> {
        match self {
            Self::Render { bounds, .. }
            | Self::Compute { bounds, .. }
            | Self::Temporal { bounds, .. } => Some(bounds),
            Self::Sample { .. } => None,
        }
    }

    pub fn overrides(&self) -> Option<&BTreeMap<String, f64>> {
        match self {
            Self::Render { overrides, .. }
            | Self::Compute { overrides, .. }
            | Self::Temporal { overrides, .. } => Some(overrides),
            Self::Sample { .. } => None,
        }
    }

    pub fn shader_module(&self) -> Option<&str> {
        match self {
            Self::Render { shader, .. } => Some(shader.module()),
            Self::Compute { shader, .. } => Some(&shader.module),
            Self::Temporal { reducer, .. } => Some(reducer.module()),
            Self::Sample { .. } => None,
        }
    }

    pub(in crate::plugin) fn validate(
        &self,
        effect_id: &str,
        properties: &[PropertySchema],
        inputs: &[crate::plugin::TextureInput],
        pass_index: usize,
    ) -> Result<(), PluginError> {
        if let Some(bounds) = self.bounds() {
            let overrides = self.overrides().expect("shader pass overrides");
            bounds.validate("effect pass", effect_id, properties, inputs, overrides)?;
            for (key, value) in overrides {
                if key.parse::<u16>().is_err() {
                    validate_wgsl_identifier("pipeline override", key)?;
                }
                if !value.is_finite() {
                    return Err(PluginError::invalid_definition(format!(
                        "effect '{effect_id}' pass {pass_index} override '{key}' must be finite"
                    )));
                }
            }
        }
        if let Self::Sample { .. } | Self::Temporal { .. } = self
            && pass_index != 0
        {
            return Err(PluginError::invalid_definition(format!(
                "effect '{effect_id}' sampling pass must be first"
            )));
        }

        match self {
            Self::Sample { sampling, .. } => sampling.validate(effect_id, properties)?,
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

/// A single source time in the owner's local clock.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SampleTimeSchema {
    Offset { offset: String },
    Progress { progress: String },
}

impl SampleTimeSchema {
    fn validate(&self, effect_id: &str, properties: &[PropertySchema]) -> Result<(), PluginError> {
        let id = match self {
            Self::Offset { offset } => offset,
            Self::Progress { progress } => progress,
        };
        if !properties
            .iter()
            .any(|p| p.id() == id && p.scalar_type(None, None) == Some(&ScalarPropertyType::F32))
        {
            return Err(PluginError::invalid_definition(format!(
                "effect '{effect_id}' sample property '{id}' must be f32"
            )));
        }
        Ok(())
    }

    pub fn sample_time(
        &self,
        values: &PropertyValues,
        local_frame: f64,
        duration: FrameDuration,
    ) -> Option<f64> {
        let id = match self {
            Self::Offset { offset } => offset,
            Self::Progress { progress } => progress,
        };
        let value = values.property(id)?.as_f32()?;
        if !value.is_finite() {
            return None;
        }
        Some(match self {
            Self::Offset { .. } => local_frame + f64::from(value),
            Self::Progress { .. } => {
                f64::from(value.clamp(0., 1.)) * duration.get().saturating_sub(1) as f64
            }
        })
    }
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
                let valid = properties
                    .iter()
                    .find(|property| property.id() == offsets)
                    .and_then(PropertySchema::array)
                    .is_some_and(|array| {
                        array
                            .element
                            .scalar(None)
                            .is_some_and(|scalar| scalar.ty == ScalarPropertyType::F32)
                            && array.min_items >= 1
                            && array.max_items <= MAX_TEMPORAL_SAMPLES
                    });

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

    /// Resolve absolute item-local source times at the consuming effect's clock.
    pub fn sample_times(&self, values: &PropertyValues, local_frame: f64) -> Option<Vec<f64>> {
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
                Some(
                    (0..count)
                        .map(|index| {
                            local_frame + start + (f64::from(index) + 0.5) * span / f64::from(count)
                        })
                        .collect(),
                )
            }

            Self::Offsets { offsets } => {
                let elements = values.property(offsets)?.as_array()?;
                elements
                    .iter()
                    .map(|element| match element.value() {
                        PropertyValue::F32(value) if value.is_finite() => {
                            Some(local_frame + f64::from(*value))
                        }
                        _ => None,
                    })
                    .collect()
            }
        }
    }
}
