//! Structural property contracts. Storage and editor projections live in their adapters.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::value::{MAX_STRING_BYTES, PropertyValue};

pub(super) const MAX_TUPLE_ELEMENTS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnumVariant {
    pub value: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<crate::localized_text::LocalizedText>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<EnumVariant>", into = "Vec<EnumVariant>")]
pub struct EnumPropertyType {
    variants: Box<[EnumVariant]>,
}

impl EnumPropertyType {
    pub(super) fn new(variants: Vec<EnumVariant>) -> Result<Self, &'static str> {
        let mut seen = HashSet::new();
        if variants.is_empty() || !variants.iter().all(|variant| seen.insert(variant.value)) {
            return Err("enum values must be unique and non-empty");
        }
        if variants.iter().any(|variant| {
            variant
                .label
                .as_ref()
                .is_some_and(crate::localized_text::LocalizedText::is_empty)
        }) {
            return Err("enum variant labels must not be empty");
        }
        let locales = variants
            .iter()
            .filter_map(|variant| variant.label.as_ref())
            .flat_map(crate::localized_text::LocalizedText::locales)
            .map(str::to_lowercase)
            .chain(std::iter::once("en-us".to_owned()))
            .collect::<std::collections::BTreeSet<_>>();
        for locale in locales {
            let mut labels = HashSet::new();
            if variants.iter().any(|variant| {
                !labels.insert(variant.label.as_ref().map_or_else(
                    || variant.value.to_string(),
                    |label| label.resolve_for(&locale).to_lowercase(),
                ))
            }) {
                return Err("enum variant labels must be unique");
            }
        }
        Ok(Self {
            variants: variants.into(),
        })
    }

    pub fn values(&self) -> impl ExactSizeIterator<Item = u32> + '_ {
        self.variants.iter().map(|variant| variant.value)
    }

    pub fn contains(&self, value: u32) -> bool {
        self.values().any(|candidate| candidate == value)
    }

    pub fn variants(&self) -> &[EnumVariant] {
        &self.variants
    }

    pub fn options(&self) -> Vec<(u32, String)> {
        self.variants
            .iter()
            .map(|variant| {
                (
                    variant.value,
                    variant.label.as_ref().map_or_else(
                        || variant.value.to_string(),
                        |label| label.resolve().to_owned(),
                    ),
                )
            })
            .collect()
    }
}

impl TryFrom<Vec<EnumVariant>> for EnumPropertyType {
    type Error = &'static str;

    fn try_from(variants: Vec<EnumVariant>) -> Result<Self, Self::Error> {
        Self::new(variants)
    }
}

impl From<EnumPropertyType> for Vec<EnumVariant> {
    fn from(value: EnumPropertyType) -> Self {
        value.variants.into()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScalarPropertyType {
    File,
    F32,
    I32,
    U32,
    Bool,
    Color,
    String,
    Enum(EnumPropertyType),
}

impl ScalarPropertyType {
    /// Display labels do not affect binding or common-control type compatibility.
    pub fn same_type(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Enum(left), Self::Enum(right)) => {
                left.values().len() == right.values().len()
                    && left.values().all(|value| right.contains(value))
            }
            _ => self == other,
        }
    }

    pub const fn is_interpolatable(&self) -> bool {
        matches!(self, Self::F32 | Self::I32 | Self::U32 | Self::Color)
    }

    pub const fn is_shader_value(&self) -> bool {
        !matches!(self, Self::File)
    }

    pub fn allows(&self, value: &PropertyValue) -> bool {
        match (self, value) {
            (Self::File, PropertyValue::File(path)) => path
                .as_ref()
                .is_none_or(|path| !path.as_os_str().is_empty()),
            (Self::F32, PropertyValue::F32(value)) => value.is_finite(),
            (Self::I32, PropertyValue::I32(_))
            | (Self::U32, PropertyValue::U32(_))
            | (Self::Bool, PropertyValue::Bool(_)) => true,
            (Self::String, PropertyValue::String(value)) => value.len() <= MAX_STRING_BYTES,
            (Self::Color, PropertyValue::Color(values)) => {
                values.iter().all(|value| value.is_finite())
            }
            (Self::Enum(ty), PropertyValue::Enum(value)) => ty.contains(*value),
            _ => false,
        }
    }
}
