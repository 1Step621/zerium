//! Labels that plugins can localize while keeping old string labels valid.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub(crate) struct LocalizedText(BTreeMap<String, String>);

impl LocalizedText {
    pub(crate) fn resolve(&self) -> &str {
        let locale = crate::i18n::locale();
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

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty() || self.0.values().any(|text| text.trim().is_empty())
    }
}

impl From<String> for LocalizedText {
    fn from(value: String) -> Self {
        Self(BTreeMap::from([(crate::i18n::locale(), value)]))
    }
}

impl From<&str> for LocalizedText {
    fn from(value: &str) -> Self {
        Self::from(value.to_owned())
    }
}
