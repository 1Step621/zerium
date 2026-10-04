//! Shared semantic validation for declarative schemas.

use std::collections::HashSet;

use crate::localized_text::LocalizedText;
use crate::property::{PropertySchema, PropertyType, PropertyValueType, ScalarPropertyType};

use super::PluginError;
use super::abi::validate_property_names;
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

/// Two placement axes, each selecting start (0), center (1), or end (2).
pub(super) fn validate_origin_property(
    context: &str,
    properties: &[PropertySchema],
    id: &str,
) -> Result<(), PluginError> {
    validate_property_reference(
        context,
        properties,
        id,
        "a tuple of two enums each containing exactly 0..=2",
        |property| {
            matches!(property.ty(), PropertyType::Value(PropertyValueType::Tuple(tuple))
            if tuple.scalars().len() == 2 && tuple.scalars().iter().all(|ty| {
                matches!(ty, ScalarPropertyType::Enum(enumeration)
                    if enumeration.values().len() == 3 && (0..=2).all(|value| enumeration.values().contains(&value)))
            }))
        },
    )
}

pub(super) fn validate_property_schemas(
    owner_kind: &str,
    owner_id: &str,
    properties: &[PropertySchema],
) -> Result<(), PluginError> {
    for property in properties {
        property.validate(owner_kind, owner_id)?;
    }
    validate_property_names(
        owner_kind,
        owner_id,
        properties
            .iter()
            .map(|property| (property.id(), property.ty())),
    )
}

/// Resolve a property reference and check the requirements of its consumer.
pub(super) fn validate_property_reference(
    context: &str,
    properties: &[PropertySchema],
    property_id: &str,
    expected: &str,
    valid: impl FnOnce(&PropertySchema) -> bool,
) -> Result<(), PluginError> {
    let property = properties
        .iter()
        .find(|property| property.id() == property_id)
        .ok_or_else(|| {
            PluginError::invalid_definition(format!(
                "{context} references missing property '{property_id}'"
            ))
        })?;
    if !valid(property) {
        return Err(PluginError::invalid_definition(format!(
            "{context} property '{property_id}' must be {expected}"
        )));
    }
    Ok(())
}
