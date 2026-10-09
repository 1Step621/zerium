//! Shared semantic validation for declarative schemas.

use std::collections::HashSet;

use crate::localized_text::LocalizedText;
use crate::property::{PropertyDefinition, PropertySchema, ScalarPropertyType, ValueSchema};

use super::PluginError;
use super::identifier::validate_logical_id;

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

/// Validate value contracts; ABI compilation already validates declaration names.
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
        self.check(
            id,
            "a tuple of two f32 values",
            |property| matches!(property.definition(), PropertyDefinition::Value(value) if is_f32_pair(value)),
        )
    }

    /// Two placement axes, each selecting start (0), center (1), or end (2).
    pub(super) fn origin(&self, id: &str) -> Result<&'a PropertySchema, PluginError> {
        self.check(id, "a tuple of two enums each containing exactly 0..=2", |property| {
            matches!(property.definition(), PropertyDefinition::Value(ValueSchema::Tuple(tuple))
                if tuple.len() == 2 && tuple.iter().all(|scalar| {
                    matches!(&scalar.ty, ScalarPropertyType::Enum(enumeration)
                        if enumeration.values().len() == 3 && (0..=2).all(|value| enumeration.contains(value)))
                }))
        })
    }
}

pub(super) fn is_f32_pair(schema: &ValueSchema) -> bool {
    matches!(schema, ValueSchema::Tuple(tuple) if tuple.len() == 2 && tuple.iter().all(|scalar| scalar.ty == ScalarPropertyType::F32))
}
