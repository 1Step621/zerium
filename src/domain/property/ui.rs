//! Presentation metadata for property controls.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use super::PropertyError;
use crate::domain::property::{PropertyType, PropertyValueType, ScalarPropertyType};

fn is_one(value: &f32) -> bool {
    *value == 1.
}

fn is_true(value: &bool) -> bool {
    *value
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum PropertyEditor {
    FontFamily,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct PropertyUi {
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    unit: String,
    #[serde(skip_serializing_if = "is_one")]
    step: f32,
    #[serde(skip_serializing_if = "is_true")]
    visible: bool,
    #[serde(
        default,
        skip_serializing_if = "enum_variants_map::is_empty",
        serialize_with = "enum_variants_map::serialize",
        deserialize_with = "enum_variants_map::deserialize"
    )]
    enum_variants: BTreeMap<u32, String>,
    #[serde(default, skip_serializing_if = "is_false")]
    multiline: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    editor: Option<PropertyEditor>,
}

impl Default for PropertyUi {
    fn default() -> Self {
        Self {
            label: None,
            unit: String::new(),
            step: 1.,
            visible: true,
            enum_variants: BTreeMap::new(),
            multiline: false,
            editor: None,
        }
    }
}

impl PropertyUi {
    pub(crate) fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub(crate) fn enum_options(&self, ty: &ScalarPropertyType) -> Option<Vec<(u32, String)>> {
        let ScalarPropertyType::Enum(ty) = ty else {
            return None;
        };
        Some(
            ty.values()
                .iter()
                .map(|value| {
                    (
                        *value,
                        self.enum_label(*value)
                            .map(str::to_owned)
                            .unwrap_or_else(|| value.to_string()),
                    )
                })
                .collect(),
        )
    }

    pub(crate) fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    pub(crate) fn unit(&self) -> &str {
        &self.unit
    }

    pub(crate) const fn step(&self) -> f32 {
        self.step
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.visible
    }

    pub(crate) const fn is_multiline(&self) -> bool {
        self.multiline
    }

    pub(crate) const fn uses_font_family_editor(&self) -> bool {
        matches!(self.editor, Some(PropertyEditor::FontFamily))
    }

    pub(super) fn enum_label(&self, value: u32) -> Option<&str> {
        self.enum_variants.get(&value).map(String::as_str)
    }

    pub(super) fn validate(
        &self,
        owner_kind: &str,
        owner_id: &str,
        property_id: &str,
        ty: &PropertyType,
    ) -> Result<(), PropertyError> {
        let invalid = |message: &str| {
            PropertyError::invalid_definition(format!(
                "{owner_kind} '{owner_id}' property '{property_id}' {message}"
            ))
        };

        if !self.step.is_finite() || self.step <= 0. {
            return Err(invalid("UI step must be positive"));
        }
        if self
            .label
            .as_ref()
            .is_some_and(|label| label.trim().is_empty())
        {
            return Err(invalid("UI scalar label must not be empty"));
        }
        if self.multiline
            && *ty != PropertyType::Value(PropertyValueType::Scalar(ScalarPropertyType::String))
        {
            return Err(invalid("ui.multiline requires a string type"));
        }
        if self.uses_font_family_editor()
            && !matches!(
                ty,
                PropertyType::Array {
                    element_type: PropertyValueType::Scalar(ScalarPropertyType::String),
                    ..
                }
            )
        {
            return Err(invalid(
                "ui.editor requires an array of strings for the font_family editor",
            ));
        }

        self.validate_enum_variants(ty, invalid)
    }

    fn validate_enum_variants(
        &self,
        ty: &PropertyType,
        invalid: impl Fn(&str) -> PropertyError,
    ) -> Result<(), PropertyError> {
        let value_type = match ty {
            PropertyType::Value(value_type)
            | PropertyType::Array {
                element_type: value_type,
                ..
            } => value_type,
        };
        let PropertyValueType::Scalar(ScalarPropertyType::Enum(enumeration)) = value_type else {
            return self
                .enum_variants
                .is_empty()
                .then_some(())
                .ok_or_else(|| invalid("UI enum_variants requires an enum type"));
        };

        let mut values = enumeration.values().to_vec();
        values.sort_unstable();
        if !self.enum_variants.is_empty() && self.enum_variants.keys().copied().ne(values) {
            return Err(invalid(
                "UI enum_variants must define a label for every enum value",
            ));
        }
        if self
            .enum_variants
            .values()
            .any(|label| label.trim().is_empty())
        {
            return Err(invalid("UI enum variant labels must not be empty"));
        }
        let mut labels = HashSet::new();
        if self
            .enum_variants
            .values()
            .any(|label| !labels.insert(label.to_lowercase()))
        {
            return Err(invalid("UI enum variant labels must be unique"));
        }
        Ok(())
    }
}

mod enum_variants_map {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(super) fn serialize<S>(
        map: &BTreeMap<u32, String>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        map.iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect::<BTreeMap<_, _>>()
            .serialize(serializer)
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<BTreeMap<u32, String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        BTreeMap::<String, String>::deserialize(deserializer)?
            .into_iter()
            .map(|(key, value)| {
                key.parse::<u32>()
                    .map_err(serde::de::Error::custom)
                    .map(|key| (key, value))
            })
            .collect()
    }

    pub(super) fn is_empty(map: &BTreeMap<u32, String>) -> bool {
        map.is_empty()
    }
}
