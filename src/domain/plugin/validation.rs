//! Shared semantic validation for declarative schemas.

use std::collections::HashSet;

use super::PluginError;

use super::identifier::validate_logical_id;

pub(super) fn validate_catalog_entry(
    kind: &str,
    id: &str,
    label: &str,
    category: &str,
    tags: &[String],
) -> Result<(), PluginError> {
    validate_logical_id(kind, id)?;
    if label.trim().is_empty() || category.trim().is_empty() {
        return Err(PluginError::invalid_definition(format!(
            "{kind} '{id}' label and category must not be empty"
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

use super::abi::validate_parameter_names;
use crate::domain::parameter::ParameterSchema;
pub(super) fn validate_parameter_schemas(
    owner_kind: &str,
    owner_id: &str,
    parameters: &[ParameterSchema],
) -> Result<(), PluginError> {
    for parameter in parameters {
        parameter.validate(owner_kind, owner_id)?;
    }
    validate_parameter_names(
        owner_kind,
        owner_id,
        parameters
            .iter()
            .map(|parameter| (parameter.id(), parameter.ty())),
    )
}
