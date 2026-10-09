//! Fixed categories for item and effect catalog entries.

use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ItemCategory {
    Shape,
    Text,
    Media,
    Composite,
    Other,
}

impl ItemCategory {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Shape => "shape",
            Self::Text => "text",
            Self::Media => "media",
            Self::Composite => "composite",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EffectCategory {
    Transform,
    Color,
    Style,
    Filter,
    Composite,
    Other,
}

impl EffectCategory {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Transform => "transform",
            Self::Color => "color",
            Self::Style => "style",
            Self::Filter => "filter",
            Self::Composite => "composite",
            Self::Other => "other",
        }
    }
}
