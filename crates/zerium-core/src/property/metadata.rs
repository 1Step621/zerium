//! Presentation metadata for property controls.

use std::{collections::HashSet, path::Path};

use serde::{Deserialize, Serialize};

use super::PropertyError;
use crate::localized_text::LocalizedText;
use crate::property::ScalarPropertyType;

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
pub struct PropertyUi {
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<LocalizedText>,
    #[serde(skip_serializing_if = "String::is_empty")]
    unit: String,
    #[serde(skip_serializing_if = "is_one")]
    step: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    drag_step: Option<f32>,
    #[serde(skip_serializing_if = "is_true")]
    visible: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    multiline: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    editor: Option<PropertyEditor>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    extensions: Vec<String>,
}

impl Default for PropertyUi {
    fn default() -> Self {
        Self {
            label: None,
            unit: String::new(),
            step: 1.,
            drag_step: None,
            visible: true,
            multiline: false,
            editor: None,
            extensions: Vec::new(),
        }
    }
}

impl PropertyUi {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    /// Suggested file extensions for selection and automatic import routing.
    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }

    pub fn matches_file(&self, path: &Path) -> bool {
        self.extensions.is_empty()
            || path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    self.extensions
                        .iter()
                        .any(|allowed| allowed.eq_ignore_ascii_case(extension))
                })
    }

    pub fn label(&self) -> Option<&str> {
        self.label.as_ref().map(LocalizedText::resolve)
    }

    pub fn unit(&self) -> &str {
        &self.unit
    }

    pub const fn step(&self) -> f32 {
        self.step
    }

    pub const fn drag_step(&self) -> Option<f32> {
        self.drag_step
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub const fn is_multiline(&self) -> bool {
        self.multiline
    }

    pub const fn uses_font_family_editor(&self) -> bool {
        matches!(self.editor, Some(PropertyEditor::FontFamily))
    }

    pub(super) fn validate(
        &self,
        owner_kind: &str,
        owner_id: &str,
        property_id: &str,
        ty: &ScalarPropertyType,
        is_array: bool,
    ) -> Result<(), PropertyError> {
        let invalid = |message: &str| {
            PropertyError::invalid_definition(format!(
                "{owner_kind} '{owner_id}' property '{property_id}' {message}"
            ))
        };

        if self
            .drag_step
            .is_some_and(|step| !step.is_finite() || step <= 0.)
        {
            return Err(invalid("UI drag_step must be positive"));
        }
        if !self.step.is_finite() || self.step <= 0. {
            return Err(invalid("UI step must be positive"));
        }
        if self.label.as_ref().is_some_and(LocalizedText::is_empty) {
            return Err(invalid("UI scalar label must not be empty"));
        }
        if self.multiline && (is_array || *ty != ScalarPropertyType::String) {
            return Err(invalid("ui.multiline requires a string type"));
        }
        if self.uses_font_family_editor() && (!is_array || *ty != ScalarPropertyType::String) {
            return Err(invalid(
                "ui.editor requires an array of strings for the font_family editor",
            ));
        }

        let mut seen = HashSet::new();
        if !self.extensions.is_empty()
            && (*ty != ScalarPropertyType::File
                || !self.extensions.iter().all(|extension| {
                    !extension.is_empty()
                        && extension
                            .bytes()
                            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
                        && seen.insert(extension)
                }))
        {
            return Err(invalid(
                "ui.extensions requires a file type and unique lowercase extensions",
            ));
        }

        Ok(())
    }
}
