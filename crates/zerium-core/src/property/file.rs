//! A host resource selected by a file property and consumed by capabilities.

use super::PropertyError;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FilePropertyType {
    #[serde(default)]
    extensions: Vec<String>,
}

impl FilePropertyType {
    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }

    pub(super) fn validate(&self) -> Result<(), PropertyError> {
        let mut extensions = HashSet::new();
        for extension in &self.extensions {
            if extension.is_empty()
                || !extension
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
                || !extensions.insert(extension)
            {
                return Err(PropertyError::invalid_definition(
                    "File extensions must be unique, nonempty lowercase ASCII names",
                ));
            }
        }
        Ok(())
    }
}
