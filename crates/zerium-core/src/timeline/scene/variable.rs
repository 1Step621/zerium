//! A scene output is supplied either by an instance argument or a calculation.
use serde::{Deserialize, Serialize};

use super::{SceneBindingTarget, SceneCalculation};
use crate::property::{PropertyConstraints, PropertySchema, PropertyValue, ScalarPropertyType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneVariablePreset {
    Number,
    SignedInteger,
    UnsignedInteger,
    Boolean,
    Color,
    Text,
    File,
}

impl SceneVariablePreset {
    pub fn ty(self) -> ScalarPropertyType {
        match self {
            Self::Number => ScalarPropertyType::F32,
            Self::SignedInteger => ScalarPropertyType::I32,
            Self::UnsignedInteger => ScalarPropertyType::U32,
            Self::Boolean => ScalarPropertyType::Bool,
            Self::Color => ScalarPropertyType::Color,
            Self::Text => ScalarPropertyType::String,
            Self::File => ScalarPropertyType::File,
        }
    }
}

/// Only arguments have defaults, constraints, and instance editing metadata.
/// Calculations declare a result type and never manufacture a fallback value.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SceneVariableSource {
    Argument {
        /// Authored values obey this type contract without evaluation limits.
        schema: PropertySchema,
        /// Applied after sampling, before consumers read this argument.
        limits: PropertyConstraints,
    },
    Calculation {
        id: String,
        label: String,
        output_type: ScalarPropertyType,
        program: SceneCalculation,
    },
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SceneVariable {
    pub source: SceneVariableSource,
    pub bindings: Vec<SceneBindingTarget>,
}

impl SceneVariable {
    pub fn argument(mut schema: PropertySchema, bindings: Vec<SceneBindingTarget>) -> Option<Self> {
        schema.scalar_type(None, None)?;
        let limits = std::mem::take(&mut schema.configuration_mut(None).constraints);
        if !schema.allows_type(&schema.default_value()) {
            return None;
        }
        schema.configuration_mut(None).editable = true;
        schema.configuration_mut(None).scene_bindable = true;
        let variable = Self {
            source: SceneVariableSource::Argument { schema, limits },
            bindings,
        };
        variable.validate_argument().ok()?;
        Some(variable)
    }

    pub fn calculated(
        id: String,
        label: String,
        output_type: ScalarPropertyType,
        program: SceneCalculation,
    ) -> Self {
        Self {
            source: SceneVariableSource::Calculation {
                id,
                label,
                output_type,
                program,
            },
            bindings: Vec::new(),
        }
    }

    pub fn id(&self) -> &str {
        match &self.source {
            SceneVariableSource::Argument { schema, .. } => schema.id(),
            SceneVariableSource::Calculation { id, .. } => id,
        }
    }

    pub fn label(&self) -> &str {
        let label = match &self.source {
            SceneVariableSource::Argument { schema, .. } => schema.label(),
            SceneVariableSource::Calculation { label, .. } => label,
        };
        if label.is_empty() { self.id() } else { label }
    }

    pub fn ty(&self) -> &ScalarPropertyType {
        match &self.source {
            SceneVariableSource::Argument { schema, .. } => schema
                .scalar_type(None, None)
                .expect("argument has a scalar contract"),
            SceneVariableSource::Calculation { output_type, .. } => output_type,
        }
    }

    pub fn argument_schema(&self) -> Option<&PropertySchema> {
        match &self.source {
            SceneVariableSource::Argument { schema, .. } => Some(schema),
            _ => None,
        }
    }

    pub(crate) fn argument_schema_mut(&mut self) -> Option<&mut PropertySchema> {
        match &mut self.source {
            SceneVariableSource::Argument { schema, .. } => Some(schema),
            _ => None,
        }
    }

    /// Input schemas preserve authored defaults, overrides and animation keys.
    /// Limits apply only after an argument has been evaluated at the sample time.
    pub fn argument_limits(&self) -> Option<&PropertyConstraints> {
        match &self.source {
            SceneVariableSource::Argument { limits, .. } => Some(limits),
            _ => None,
        }
    }

    pub fn apply_argument_limits(&self, value: &PropertyValue) -> Option<PropertyValue> {
        self.argument_schema()?.allows_type(value).then_some(())?;
        self.argument_limits()?.clamp_value(value)
    }

    pub(super) fn validate_argument(&self) -> Result<(), String> {
        let SceneVariableSource::Argument { schema, limits } = &self.source else {
            return Ok(());
        };
        schema
            .validate("scene argument", schema.id())
            .map_err(|error| error.to_string())?;
        let ty = schema
            .scalar_type(None, None)
            .ok_or("Scene arguments must be scalar")?;
        if !schema.configuration_constraints(None).is_default() {
            return Err("Scene argument input schemas cannot constrain authored values".to_owned());
        }
        limits
            .validate("scene argument", schema.id(), schema.id(), ty, None)
            .map_err(|error| error.to_string())
    }

    pub fn calculation(&self) -> Option<&SceneCalculation> {
        match &self.source {
            SceneVariableSource::Calculation { program, .. } => Some(program),
            _ => None,
        }
    }

    pub(crate) fn calculation_mut(&mut self) -> Option<&mut SceneCalculation> {
        match &mut self.source {
            SceneVariableSource::Calculation { program, .. } => Some(program),
            _ => None,
        }
    }

    pub(crate) fn rename(&mut self, label: String) {
        match &mut self.source {
            SceneVariableSource::Argument { schema, .. } => schema.label = label.into(),
            SceneVariableSource::Calculation { label: current, .. } => *current = label,
        }
    }

    pub(crate) fn set_default(&mut self, value: &PropertyValue) -> Option<bool> {
        let schema = self.argument_schema_mut()?;
        if !schema.allows_type(value) {
            return None;
        }
        if schema.default_value() == *value {
            return Some(false);
        }
        schema.set_default(value.clone());
        Some(true)
    }

    pub(crate) fn set_numeric_settings(
        &mut self,
        settings: crate::property::NumericSettings,
    ) -> bool {
        let SceneVariableSource::Argument { schema, limits } = &mut self.source else {
            return false;
        };
        let (default, constraints) = settings.into_parts();
        if !schema.allows_type(&default)
            || (schema.default_value() == default && *limits == constraints)
        {
            return false;
        }
        schema.set_default(default);
        *limits = constraints;
        true
    }
}

pub(in crate::timeline) fn unique_scene_variable_name(
    variables: &[SceneVariable],
    preferred: &str,
    fallback: &str,
) -> String {
    let preferred = if preferred.trim().is_empty() {
        fallback
    } else {
        preferred.trim()
    };
    let duplicate = |candidate: &str| {
        variables
            .iter()
            .any(|variable| variable.label() == candidate)
    };
    if !duplicate(preferred) {
        return preferred.to_owned();
    }
    (2_u64..)
        .map(|suffix| format!("{preferred}_{suffix}"))
        .find(|candidate| !duplicate(candidate))
        .expect("a variable name is available")
}
