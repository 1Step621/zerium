//! Shared semantic validation for declarative schemas.

use std::collections::HashSet;

use crate::localized_text::LocalizedText;
use crate::property::{PropertySchema, ScalarPropertyType, ValueSchema};

use super::PluginError;

/// Validates an identifier used only for manifest lookup and namespacing.
pub(super) fn validate_logical_id(kind: &str, id: &str) -> Result<(), PluginError> {
    let mut bytes = id.bytes();
    let valid = bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    valid.then_some(()).ok_or_else(|| {
        PluginError::invalid_definition(format!(
            "{kind} ID '{id}' must start with an ASCII letter or digit and contain only ASCII letters, digits, '.', '_', or '-'"
        ))
    })
}

/// Validates an identifier emitted directly into a shader module.
pub fn validate_wgsl_identifier(kind: &str, id: &str) -> Result<(), PluginError> {
    let mut bytes = id.bytes();
    let valid_syntax = bytes
        .next()
        .is_some_and(|byte| byte == b'_' || byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric());
    if !valid_syntax {
        return Err(PluginError::invalid_definition(format!(
            "{kind} ID '{id}' must be an ASCII WGSL identifier"
        )));
    }
    if id == "_" || id.starts_with("__") || naga::keywords::wgsl::RESERVED.contains(&id) {
        return Err(PluginError::invalid_definition(format!(
            "{kind} ID '{id}' is reserved by WGSL"
        )));
    }
    Ok(())
}

pub(super) fn validate_catalog_entry(
    kind: &str,
    id: &str,
    label: &LocalizedText,
    tags: &[String],
) -> Result<(), PluginError> {
    validate_logical_id(kind, id)?;
    if label.is_empty() {
        return Err(PluginError::invalid_definition(format!(
            "{kind} '{id}' label must not be empty"
        )));
    }
    validate_search_tags(kind, id, tags)
}

pub(super) fn validate_unique_ids<'a>(
    kind: &str,
    ids: impl IntoIterator<Item = &'a str>,
) -> Result<(), PluginError> {
    let mut unique = HashSet::new();
    for id in ids {
        if !unique.insert(id) {
            return Err(PluginError::invalid_definition(format!(
                "duplicate {kind} ID '{id}'"
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_search_tags(
    kind: &str,
    id: &str,
    tags: &[String],
) -> Result<(), PluginError> {
    let mut normalized = HashSet::new();
    for tag in tags {
        let trimmed = tag.trim();
        if trimmed.is_empty() || trimmed != tag {
            return Err(PluginError::invalid_definition(format!(
                "{kind} '{id}' search tags must be non-empty and have no surrounding whitespace"
            )));
        }
        if !normalized.insert(trimmed.to_lowercase()) {
            return Err(PluginError::invalid_definition(format!(
                "{kind} '{id}' has duplicate search tag '{tag}'"
            )));
        }
    }
    Ok(())
}

/// Validate property value contracts; callers validate declaration names and uniqueness.
pub(super) fn validate_property_schemas(
    owner_kind: &str,
    owner_id: &str,
    properties: &[PropertySchema],
) -> Result<(), PluginError> {
    for property in properties {
        property.validate(owner_kind, owner_id)?;
    }
    Ok(())
}

/// Property references checked in the context of one capability or editor.
pub(super) struct PropertyReferences<'a> {
    context: &'a str,
    properties: &'a [PropertySchema],
}

impl<'a> PropertyReferences<'a> {
    pub(super) fn new(context: &'a str, properties: &'a [PropertySchema]) -> Self {
        Self {
            context,
            properties,
        }
    }

    pub(super) fn check(
        &self,
        property_id: &str,
        expected: &str,
        valid: impl FnOnce(&PropertySchema) -> bool,
    ) -> Result<&'a PropertySchema, PluginError> {
        let property = self
            .properties
            .iter()
            .find(|property| property.id() == property_id)
            .ok_or_else(|| {
                PluginError::invalid_definition(format!(
                    "{} references missing property '{property_id}'",
                    self.context
                ))
            })?;
        if !valid(property) {
            return Err(PluginError::invalid_definition(format!(
                "{} property '{property_id}' must be {expected}",
                self.context
            )));
        }
        Ok(property)
    }

    pub(super) fn scalar(
        &self,
        id: &str,
        ty: ScalarPropertyType,
    ) -> Result<&'a PropertySchema, PluginError> {
        self.check(id, &format!("a {ty:?} value"), |property| {
            property
                .scalar_type(None, None)
                .is_some_and(|actual| actual.same_type(&ty))
        })
    }

    pub(super) fn pair(&self, id: &str) -> Result<&'a PropertySchema, PluginError> {
        self.check(id, "a tuple of two f32 values", |property| {
            property.single_value_schema().is_some_and(is_f32_pair)
        })
    }

    /// Two placement axes, each selecting start (0), center (1), or end (2).
    pub(super) fn origin(&self, id: &str) -> Result<&'a PropertySchema, PluginError> {
        self.check(
            id,
            "a tuple of two enums each containing exactly 0..=2",
            |property| {
                property
                    .single_value_schema()
                    .and_then(ValueSchema::tuple)
                    .is_some_and(|tuple| {
                        tuple.len() == 2
                            && tuple
                                .iter()
                                .all(|scalar| has_enum_values(&scalar.ty, &[0, 1, 2]))
                    })
            },
        )
    }
}

pub(super) fn is_f32_pair(schema: &ValueSchema) -> bool {
    schema.tuple().is_some_and(|tuple| {
        tuple.len() == 2
            && tuple
                .iter()
                .all(|scalar| scalar.ty == ScalarPropertyType::F32)
    })
}

pub(super) fn has_enum_values(ty: &ScalarPropertyType, values: &[u32]) -> bool {
    ty.enumeration()
        .is_some_and(|enumeration| enumeration.has_exact_values(values))
}
