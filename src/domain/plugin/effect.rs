//! Effect schemas, render passes, and temporal sampling.

use serde::{Deserialize, Deserializer, de::Error as _};
use serde_json::Value;

use super::PluginError;
use super::abi::{CompiledPropertyAbi, PropertyAbiField, PropertyInterfaceNames};
use super::identifier::validate_wgsl_identifier;
use super::shader::{ShaderSchema, validate_shader_source};
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
    properties: Vec<PropertySchema>,
    passes: Vec<EffectPassSchema>,
    pass_abis: Vec<CompiledPropertyAbi>,
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
    properties: Vec<PropertySchema>,
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
            properties: definition.properties,
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
    property_type: PropertyType,
    value: PropertyValue,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PassConstantSchemaDefinition {
    id: String,
    #[serde(rename = "type")]
    ty: PropertyValueType,
    value: Value,
}

impl<'de> Deserialize<'de> for PassConstantSchema {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;

        let definition = PassConstantSchemaDefinition::deserialize(deserializer)?;
        if definition.ty.scalar_at(None) == Some(&ScalarPropertyType::String) {
            return Err(D::Error::custom(
                "effect pass constants must have a fixed-size ABI type",
            ));
        }
        let ty = PropertyType::Value(definition.ty.clone());
        let value = PropertyValue::from_json(&definition.value, &ty).ok_or_else(|| {
            D::Error::custom("effect pass constant value does not match its type")
        })?;
        Ok(Self {
            id: definition.id,
            property_type: ty,
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

    pub(crate) fn properties(&self) -> &[PropertySchema] {
        &self.properties
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

    pub(crate) fn property(&self, id: &str) -> Option<&PropertySchema> {
        self.properties.iter().find(|property| property.id == id)
    }

    pub(crate) fn default_property_values(&self) -> PropertyValues {
        PropertyValues::for_owner("effect", &self.id, &self.properties)
    }

    pub(crate) fn temporal_sample_offsets(
        &self,
        pass: &EffectPassSchema,
        values: &PropertyValues,
    ) -> Option<Vec<f64>> {
        let EffectPassSchema::Temporal { sampling, .. } = pass else {
            return None;
        };
        sampling.sample_offsets(values)
    }

    pub(crate) fn wgsl_property_interface(
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

    pub(crate) fn pack_pass_properties(
        &self,
        values: &PropertyValues,
    ) -> Result<Vec<Vec<u8>>, PluginError> {
        values.validate_for("effect", &self.id, &self.properties)?;
        self.pass_abis
            .iter()
            .map(|abi| {
                abi.pack("effect", &self.id, |id, _| {
                    values.property(id).ok_or_else(|| {
                        PluginError::invalid_definition(format!(
                            "effect '{}' is missing property '{id}'",
                            self.id
                        ))
                    })
                })
            })
            .collect()
    }

    fn compile_pass_abis(&self) -> Result<Vec<CompiledPropertyAbi>, PluginError> {
        self.passes
            .iter()
            .enumerate()
            .map(|(pass_index, pass)| {
                let runtime = self.properties.iter().map(|property| PropertyAbiField {
                    id: property.id(),
                    ty: property.ty(),
                    static_value: None,
                });
                let constants = pass.constants().iter().map(|constant| PropertyAbiField {
                    id: &constant.id,
                    ty: &constant.property_type,
                    static_value: Some(&constant.value),
                });
                CompiledPropertyAbi::compile(
                    "effect pass",
                    &format!("{}:{pass_index}", self.id),
                    runtime.chain(constants),
                    PropertyInterfaceNames {
                        struct_name: "ZeriumProperties",
                        load_function: "zerium_load_properties",
                        raw_load_function: "zerium_raw_properties_for_effect",
                        accessor_prefix: "zerium_property",
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
        for (property_id, expected_type, role) in [
            (
                sample_count.as_str(),
                ScalarPropertyType::U32,
                "sample count",
            ),
            (angle.as_str(), ScalarPropertyType::F32, "shutter angle"),
        ] {
            if effect.property(property_id).map(|property| &property.ty)
                != Some(&PropertyType::Value(PropertyValueType::Scalar(
                    expected_type,
                )))
            {
                return Err(PluginError::invalid_definition(format!(
                    "effect '{}' temporal {role} property '{}' has the wrong type",
                    effect.id, property_id
                )));
            }
        }
        let samples = effect
            .property(sample_count)
            .expect("sample-count property type was checked");
        if !samples
            .scalar_constraints(None)
            .min
            .is_some_and(|minimum| minimum >= 1.)
            || !samples
                .scalar_constraints(None)
                .max
                .is_some_and(|maximum| maximum <= MAX_TEMPORAL_SAMPLES as f64)
        {
            return Err(PluginError::invalid_definition(format!(
                "effect '{}' temporal sample count must be constrained to 1..={MAX_TEMPORAL_SAMPLES}",
                effect.id
            )));
        }
        let angle = effect
            .property(angle)
            .expect("shutter-angle property type was checked");
        if !angle
            .scalar_constraints(None)
            .min
            .is_some_and(|minimum| minimum >= 0.)
        {
            return Err(PluginError::invalid_definition(format!(
                "effect '{}' temporal shutter angle must have a non-negative minimum",
                effect.id
            )));
        }
        if let Some(property_id) = phase
            && effect.property(property_id).map(|property| &property.ty)
                != Some(&PropertyType::Value(PropertyValueType::Scalar(
                    ScalarPropertyType::F32,
                )))
        {
            return Err(PluginError::invalid_definition(format!(
                "effect '{}' temporal shutter phase property '{}' has the wrong type",
                effect.id, property_id
            )));
        }
        if let Some(property_id) = phase {
            let phase = effect
                .property(property_id)
                .expect("shutter-phase property type was checked");
            if !phase
                .scalar_constraints(None)
                .min
                .is_some_and(|minimum| minimum >= -1.)
                || !phase
                    .scalar_constraints(None)
                    .max
                    .is_some_and(|maximum| maximum <= 1.)
            {
                return Err(PluginError::invalid_definition(format!(
                    "effect '{}' temporal shutter phase must be constrained to -1..=1",
                    effect.id
                )));
            }
        }
        Ok(())
    }

    fn sample_offsets(&self, values: &PropertyValues) -> Option<Vec<f64>> {
        let Self::Shutter {
            sample_count,
            angle,
            phase,
        } = self;
        let sample_count = match values.property(sample_count)? {
            PropertyValue::U32(value) => (*value).clamp(1, MAX_TEMPORAL_SAMPLES),
            _ => return None,
        };
        let shutter_angle = match values.property(angle)? {
            PropertyValue::F32(value) if value.is_finite() => f64::from(value.abs()),
            _ => return None,
        };
        if sample_count == 1 || shutter_angle <= f64::EPSILON {
            return Some(vec![0.]);
        }
        let phase = phase
            .as_deref()
            .and_then(|id| values.property(id))
            .and_then(|value| match value {
                PropertyValue::F32(value) if value.is_finite() => Some(f64::from(*value)),
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
