//! How an item's completed image is composited onto the layers below it.

use serde::{Deserialize, Serialize};

// Discriminants are the GPU ABI used by the application's compositing shader.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    #[default]
    Normal = 0,
    Darken = 1,
    Multiply = 2,
    Lighten = 3,
    Screen = 4,
    Add = 5,
    Overlay = 6,
    SoftLight = 7,
    HardLight = 8,
    Difference = 9,
    Exclusion = 10,
}

impl BlendMode {
    pub const ALL: [Self; 11] = [
        Self::Normal,
        Self::Darken,
        Self::Multiply,
        Self::Lighten,
        Self::Screen,
        Self::Add,
        Self::Overlay,
        Self::SoftLight,
        Self::HardLight,
        Self::Difference,
        Self::Exclusion,
    ];

    pub const fn id(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Darken => "darken",
            Self::Multiply => "multiply",
            Self::Lighten => "lighten",
            Self::Screen => "screen",
            Self::Add => "add",
            Self::Overlay => "overlay",
            Self::SoftLight => "soft_light",
            Self::HardLight => "hard_light",
            Self::Difference => "difference",
            Self::Exclusion => "exclusion",
        }
    }
}
