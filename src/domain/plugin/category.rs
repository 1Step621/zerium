//! Stable grouping identity and localized display label for catalog categories.

use serde::Deserialize;

use crate::domain::localized_text::LocalizedText;

use super::{PluginError, identifier::validate_logical_id};

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CatalogCategory {
    id: String,
    label: LocalizedText,
}

impl CatalogCategory {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn label(&self) -> &str {
        self.label.resolve()
    }

    pub(crate) fn validate(&self, kind: &str, owner_id: &str) -> Result<(), PluginError> {
        validate_logical_id("category", &self.id)?;
        if self.label.is_empty() {
            return Err(PluginError::invalid_definition(format!(
                "{kind} '{owner_id}' category label must not be empty"
            )));
        }
        Ok(())
    }
}
