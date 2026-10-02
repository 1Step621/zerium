//! Localized labels using the application's shared `rust-i18n` locale.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct LocalizedText(BTreeMap<String, String>);

impl LocalizedText {
    pub fn resolve(&self) -> &str {
        self.resolve_for(&rust_i18n::locale())
    }

    /// Resolve a particular locale without changing the shared display language.
    pub fn resolve_for(&self, locale: &str) -> &str {
        let normalized_locale = locale.to_lowercase();
        self.find_locale(&normalized_locale)
            .or_else(|| self.find_locale("en-us"))
            .or_else(|| self.0.values().next())
            .map(String::as_str)
            .unwrap_or_default()
    }

    fn find_locale(&self, locale: &str) -> Option<&String> {
        self.0
            .iter()
            .find_map(|(tag, text)| (tag.to_lowercase() == locale).then_some(text))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty() || self.0.values().any(|text| text.trim().is_empty())
    }

    pub(crate) fn locales(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }
}

impl From<String> for LocalizedText {
    fn from(value: String) -> Self {
        // User-authored names are plain text; every locale uses this fallback.
        Self(BTreeMap::from([("en-us".to_owned(), value)]))
    }
}

impl From<&str> for LocalizedText {
    fn from(value: &str) -> Self {
        Self::from(value.to_owned())
    }
}
