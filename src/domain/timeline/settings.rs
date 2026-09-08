use std::{error::Error, fmt, num::NonZeroU32, str::FromStr};

use super::FrameRate;

/// Pixel dimensions of the project's final frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProjectResolution {
    width: NonZeroU32,
    height: NonZeroU32,
}

impl ProjectResolution {
    pub(crate) const MAX_DIMENSION: u32 = 8_192;
    pub(crate) const DEFAULT: Self = Self {
        width: NonZeroU32::new(1_920).unwrap(),
        height: NonZeroU32::new(1_080).unwrap(),
    };

    pub(crate) const fn new(width: u32, height: u32) -> Option<Self> {
        if width > Self::MAX_DIMENSION || height > Self::MAX_DIMENSION {
            return None;
        }
        match (NonZeroU32::new(width), NonZeroU32::new(height)) {
            (Some(width), Some(height)) => Some(Self { width, height }),
            _ => None,
        }
    }

    pub(crate) const fn width(self) -> u32 {
        self.width.get()
    }

    pub(crate) const fn height(self) -> u32 {
        self.height.get()
    }

    pub(crate) fn aspect_ratio(self) -> f32 {
        self.width() as f32 / self.height() as f32
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProjectSettingsError {
    InvalidFrameRate(String),
    OutOfRange(String),
}

impl ProjectSettingsError {
    pub(super) fn out_of_range(message: impl Into<String>) -> Self {
        Self::OutOfRange(message.into())
    }

    fn invalid_frame_rate(message: impl Into<String>) -> Self {
        Self::InvalidFrameRate(message.into())
    }
}

impl fmt::Display for ProjectSettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidFrameRate(message) | Self::OutOfRange(message) => message,
        })
    }
}

impl Error for ProjectSettingsError {}

impl FromStr for FrameRate {
    type Err = ProjectSettingsError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        let invalid = || {
            ProjectSettingsError::invalid_frame_rate(
                "フレームレートは 30 または 30000/1001 の形式で入力してください",
            )
        };
        let (numerator, denominator) = match value.split_once('/') {
            Some((numerator, denominator)) => (
                numerator.trim().parse::<u32>().map_err(|_| invalid())?,
                denominator.trim().parse::<u32>().map_err(|_| invalid())?,
            ),
            None => decimal_ratio(value).ok_or_else(invalid)?,
        };
        let divisor = gcd(numerator, denominator).max(1);
        FrameRate::new(numerator / divisor, denominator / divisor).ok_or_else(|| {
            ProjectSettingsError::invalid_frame_rate("フレームレートは0より大きくしてください")
        })
    }
}

fn decimal_ratio(value: &str) -> Option<(u32, u32)> {
    let Some((integer, fraction)) = value.split_once('.') else {
        return Some((value.parse().ok()?, 1));
    };
    let denominator = 10_u32.checked_pow(fraction.len() as u32)?;
    let numerator = format!("{integer}{fraction}").parse::<u32>().ok()?;
    Some((numerator, denominator))
}

const fn gcd(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}
