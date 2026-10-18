use super::SceneBindingTarget;
use crate::property::{PropertySchema, PropertyValue, ScalarPropertyType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneArgumentPreset {
    Number,
    SignedInteger,
    UnsignedInteger,
    Boolean,
    Color,
    Text,
    File,
}

impl SceneArgumentPreset {
    pub fn ty(self) -> ScalarPropertyType {
        match self {
            Self::File => ScalarPropertyType::File,
            Self::Number => ScalarPropertyType::F32,
            Self::SignedInteger => ScalarPropertyType::I32,
            Self::UnsignedInteger => ScalarPropertyType::U32,
            Self::Boolean => ScalarPropertyType::Bool,
            Self::Color => ScalarPropertyType::Color,
            Self::Text => ScalarPropertyType::String,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneArgument {
    pub schema: PropertySchema,
    pub bindings: Vec<SceneBindingTarget>,
}

impl SceneArgument {
    /// Scene arguments own their editable scalar contract independently of the
    /// plugin properties to which they are connected.
    pub fn new(mut schema: PropertySchema, bindings: Vec<SceneBindingTarget>) -> Option<Self> {
        schema.scalar_type(None, None)?;
        schema.set_default(schema.constrained_value(&schema.default_value())?);
        schema.configuration_mut(None).editable = true;
        schema.configuration_mut(None).scene_bindable = true;
        Some(Self { schema, bindings })
    }

    pub fn label(&self) -> &str {
        let label = self.schema.label();
        if label.is_empty() {
            self.schema.id()
        } else {
            label
        }
    }

    pub fn set_default(&mut self, value: &PropertyValue) -> Option<bool> {
        let default = self.schema.constrained_value(value)?;
        if self.schema.default_value() == default {
            return Some(false);
        }
        self.schema.set_default(default);
        Some(true)
    }

    pub fn set_numeric_settings(&mut self, settings: crate::property::NumericSettings) -> bool {
        let (default, constraints) = settings.into_parts();
        if !self.schema.allows_type(&default)
            || (self.schema.default_value() == default
                && *self.schema.configuration_constraints(None) == constraints)
        {
            return false;
        }
        self.schema.set_default(default);
        self.schema.configuration_mut(None).constraints = constraints;
        true
    }
}

pub(in crate::timeline) fn unique_scene_argument_name(
    arguments: &[SceneArgument],
    preferred: &str,
    fallback: &str,
) -> String {
    let normalized = if preferred.trim().is_empty() {
        fallback
    } else {
        preferred.trim()
    };
    let normalized_is_duplicate = |candidate: &str| {
        arguments
            .iter()
            .any(|argument| argument.schema.label() == candidate)
    };
    if !normalized_is_duplicate(normalized) {
        return normalized.to_owned();
    }
    (2_u64..)
        .map(|suffix| format!("{normalized}_{suffix}"))
        .find(|candidate| !normalized_is_duplicate(candidate))
        .expect("a scene argument name suffix must eventually be available")
}
