//! Effect schemas, render passes, and temporal sampling.

use serde::{Deserialize, Deserializer, de::Error as _};
use serde_json::Value;

use super::PluginError;
use super::abi::{CompiledParameterAbi, ParameterAbiField, ParameterInterfaceNames};
use super::identifier::validate_wgsl_identifier;
use super::shader::{ShaderSchema, validate_shader_source};
use super::validation::validate_catalog_entry;
use crate::domain::parameter::{
    ParameterSchema, ParameterType, ParameterValue, ParameterValueType, ParameterValues,
    ScalarParameterType,
};
use crate::domain::plugin::validation::validate_parameter_schemas;

const MAX_TEMPORAL_SAMPLES: u32 = 32;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EffectSchema {
    id: String,
    label: String,
    category: String,
    tags: Vec<String>,
    render_scale: u32,
    parameters: Vec<ParameterSchema>,
    passes: Vec<EffectPassSchema>,
    pass_abis: Vec<CompiledParameterAbi>,
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
    parameters: Vec<ParameterSchema>,
    passes: Vec<EffectPassSchema>,
}

impl<'de> Deserialize<'de> for EffectSchema {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let definition = EffectSchemaDefinition::deserialize(deserializer)?;
        let mut schema = Self {
            id: definition.id,
            label: definition.label,
            category: definition.category,
            tags: definition.tags,
            render_scale: definition.render_scale,
            parameters: definition.parameters,
            passes: definition.passes,
            pass_abis: Vec::new(),
        };
        schema.validate().map_err(D::Error::custom)?;
        schema.pass_abis = schema.compile_pass_abis().map_err(D::Error::custom)?;
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
    pub(crate) fn constants(&self) -> &[PassConstantSchema] {
        match self {
            Self::Render { constants, .. }
            | Self::Compute { constants, .. }
            | Self::Temporal { constants, .. } => constants,
        }
    }

    pub(crate) fn shader_source(&self) -> &str {
        match self {
            Self::Render { shader, .. } => shader.source(),
            Self::Compute { shader, .. } => &shader.source,
            Self::Temporal { reducer, .. } => reducer.source(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PassConstantSchema {
    id: String,
    parameter_type: ParameterType,
    value: ParameterValue,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PassConstantSchemaDefinition {
    id: String,
    #[serde(rename = "type")]
    ty: ParameterValueType,
    value: Value,
}

impl<'de> Deserialize<'de> for PassConstantSchema {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;

        let definition = PassConstantSchemaDefinition::deserialize(deserializer)?;
        if definition.ty.scalar_type() == Some(&ScalarParameterType::String) {
            return Err(D::Error::custom(
                "effect pass constants must have a fixed-size ABI type",
            ));
        }
        let ty = ParameterType::Value(definition.ty.clone());
        let value = ParameterValue::from_json(&definition.value, &ty).ok_or_else(|| {
            D::Error::custom("effect pass constant value does not match its type")
        })?;
        Ok(Self {
            id: definition.id,
            parameter_type: ty,
            value,
        })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum TemporalSamplingSchema {
    Shutter {
        sample_count: String,
        angle: String,
        #[serde(default)]
        phase: Option<String>,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ComputeShaderSchema {
    source: String,
    #[serde(default = "default_compute_entry")]
    entry: String,
}

impl ComputeShaderSchema {
    pub(crate) fn entry(&self) -> &str {
        &self.entry
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
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

    pub(crate) fn parameters(&self) -> &[ParameterSchema] {
        &self.parameters
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
        validate_parameter_schemas("effect", &self.id, &self.parameters)?;
        if self.passes.is_empty() {
            return Err(PluginError::invalid_definition(format!(
                "effect '{}' must define at least one pass",
                self.id
            )));
        }
        for (pass_index, pass) in self.passes.iter().enumerate() {
            match pass {
                EffectPassSchema::Render { shader, .. } => {
                    shader.validate("render effect pass", &self.id)?;
                }
                EffectPassSchema::Compute { shader, .. } => {
                    validate_shader_source("compute effect pass", &self.id, &shader.source)?;
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
        if self
            .passes
            .iter()
            .filter(|pass| matches!(pass, EffectPassSchema::Temporal { .. }))
            .count()
            > 1
        {
            return Err(PluginError::invalid_definition(format!(
                "effect '{}' may define only one temporal pass",
                self.id
            )));
        }
        Ok(())
    }

    pub(crate) fn parameter(&self, id: &str) -> Option<&ParameterSchema> {
        self.parameters.iter().find(|parameter| parameter.id == id)
    }

    pub(crate) fn default_parameter_values(&self) -> ParameterValues {
        ParameterValues::for_owner("effect", &self.id, &self.parameters)
    }

    pub(crate) fn temporal_sample_offsets(
        &self,
        pass: &EffectPassSchema,
        values: &ParameterValues,
    ) -> Option<Vec<f64>> {
        let EffectPassSchema::Temporal { sampling, .. } = pass else {
            return None;
        };
        sampling.sample_offsets(values)
    }

    pub(crate) fn wgsl_parameter_interface(
        &self,
        pass: &EffectPassSchema,
    ) -> Result<String, PluginError> {
        let index = self
            .passes
            .iter()
            .position(|candidate| std::ptr::eq(candidate, pass))
            .or_else(|| self.passes.iter().position(|candidate| candidate == pass))
            .ok_or_else(|| {
                PluginError::invalid_definition("effect pass does not belong to schema")
            })?;
        Ok(self.pass_abis[index].interface().to_owned())
    }

    pub(crate) fn pack_pass_parameters(
        &self,
        values: &ParameterValues,
    ) -> Result<Vec<Vec<u8>>, PluginError> {
        values.validate_for("effect", &self.id, &self.parameters)?;
        self.pass_abis
            .iter()
            .map(|abi| {
                abi.pack("effect", &self.id, |id, _| {
                    values.get(id).ok_or_else(|| {
                        PluginError::invalid_definition(format!(
                            "effect '{}' is missing parameter '{id}'",
                            self.id
                        ))
                    })
                })
            })
            .collect()
    }

    fn compile_pass_abis(&self) -> Result<Vec<CompiledParameterAbi>, PluginError> {
        self.passes
            .iter()
            .enumerate()
            .map(|(pass_index, pass)| {
                let runtime = self.parameters.iter().map(|parameter| ParameterAbiField {
                    id: parameter.id(),
                    ty: parameter.ty(),
                    static_value: None,
                });
                let constants = pass.constants().iter().map(|constant| ParameterAbiField {
                    id: &constant.id,
                    ty: &constant.parameter_type,
                    static_value: Some(&constant.value),
                });
                CompiledParameterAbi::compile(
                    "effect pass",
                    &format!("{}:{pass_index}", self.id),
                    runtime.chain(constants),
                    ParameterInterfaceNames {
                        struct_name: "ZeriumParameters",
                        load_function: "zerium_load_parameters",
                        raw_load_function: "zerium_raw_params_for_effect",
                        accessor_prefix: "zerium_parameter",
                        takes_instance_index: false,
                    },
                )
            })
            .collect()
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
        let Self::Shutter {
            sample_count,
            angle,
            phase,
        } = self;
        for (parameter_id, expected_type, role) in [
            (
                sample_count.as_str(),
                ScalarParameterType::U32,
                "sample count",
            ),
            (angle.as_str(), ScalarParameterType::F32, "shutter angle"),
        ] {
            if effect
                .parameter(parameter_id)
                .map(|parameter| &parameter.ty)
                != Some(&ParameterType::Value(ParameterValueType::Scalar(
                    expected_type,
                )))
            {
                return Err(PluginError::invalid_definition(format!(
                    "effect '{}' temporal {role} parameter '{}' has the wrong type",
                    effect.id, parameter_id
                )));
            }
        }
        let samples = effect
            .parameter(sample_count)
            .expect("sample-count parameter type was checked");
        if !samples.constraints.min.is_some_and(|minimum| minimum >= 1.)
            || !samples
                .constraints
                .max
                .is_some_and(|maximum| maximum <= MAX_TEMPORAL_SAMPLES as f64)
        {
            return Err(PluginError::invalid_definition(format!(
                "effect '{}' temporal sample count must be constrained to 1..={MAX_TEMPORAL_SAMPLES}",
                effect.id
            )));
        }
        let angle = effect
            .parameter(angle)
            .expect("shutter-angle parameter type was checked");
        if !angle.constraints.min.is_some_and(|minimum| minimum >= 0.) {
            return Err(PluginError::invalid_definition(format!(
                "effect '{}' temporal shutter angle must have a non-negative minimum",
                effect.id
            )));
        }
        if let Some(parameter_id) = phase
            && effect
                .parameter(parameter_id)
                .map(|parameter| &parameter.ty)
                != Some(&ParameterType::Value(ParameterValueType::Scalar(
                    ScalarParameterType::F32,
                )))
        {
            return Err(PluginError::invalid_definition(format!(
                "effect '{}' temporal shutter phase parameter '{}' has the wrong type",
                effect.id, parameter_id
            )));
        }
        if let Some(parameter_id) = phase {
            let phase = effect
                .parameter(parameter_id)
                .expect("shutter-phase parameter type was checked");
            if !phase.constraints.min.is_some_and(|minimum| minimum >= -1.)
                || !phase.constraints.max.is_some_and(|maximum| maximum <= 1.)
            {
                return Err(PluginError::invalid_definition(format!(
                    "effect '{}' temporal shutter phase must be constrained to -1..=1",
                    effect.id
                )));
            }
        }
        Ok(())
    }

    fn sample_offsets(&self, values: &ParameterValues) -> Option<Vec<f64>> {
        let Self::Shutter {
            sample_count,
            angle,
            phase,
        } = self;
        let sample_count = match values.get(sample_count)? {
            ParameterValue::U32(value) => (*value).clamp(1, MAX_TEMPORAL_SAMPLES),
            _ => return None,
        };
        let shutter_angle = match values.get(angle)? {
            ParameterValue::F32(value) if value.is_finite() => f64::from(value.abs()),
            _ => return None,
        };
        if sample_count == 1 || shutter_angle <= f64::EPSILON {
            return Some(vec![0.]);
        }
        let phase = phase
            .as_deref()
            .and_then(|id| values.get(id))
            .and_then(|value| match value {
                ParameterValue::F32(value) if value.is_finite() => Some(f64::from(*value)),
                _ => None,
            })
            .unwrap_or(0.)
            .clamp(-1., 1.);
        let exposure_frames = shutter_angle / 360.;
        let start = (phase - 1.) * exposure_frames * 0.5;
        let step = exposure_frames / f64::from(sample_count);
        Some(
            (0..sample_count)
                .map(|index| start + (f64::from(index) + 0.5) * step)
                .collect(),
        )
    }
}
