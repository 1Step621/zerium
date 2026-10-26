//! Project state, settings, and detached snapshots.

use super::ProjectId;
use crate::{
    media::MediaMetadataCache,
    timeline::{Frame, FrameRate, SceneDefinition, SceneId},
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, num::NonZeroU32, str::FromStr, sync::Arc};
use thiserror::Error;

/// Persistent state for one timeline project. Mutations are coordinated by
/// `TimelineEditor`, which also exposes immutable snapshots to background work.
#[derive(Clone)]
pub(in crate::timeline) struct TimelineProject {
    pub(in crate::timeline) id: ProjectId,
    pub(in crate::timeline) media_dependencies_revision: u64,
    pub(in crate::timeline) main_scene: SceneId,
    pub(in crate::timeline) scenes: Arc<HashMap<SceneId, SceneDefinition>>,
    pub(in crate::timeline) resolution: ProjectResolution,
    pub(in crate::timeline) beat_guide: BeatGuide,
}

impl TimelineProject {
    /// Every definition owns its local clock, including its expansion anchor.
    /// Prepare the entire project before publishing a frame-rate change.
    pub(in crate::timeline) fn retimed(
        &self,
        frame_rate: FrameRate,
    ) -> Result<Self, ProjectSettingsError> {
        let mut project = self.clone();
        if self.main_scene().document().frame_rate() != frame_rate {
            project.scenes = Arc::new(
                self.scenes
                    .iter()
                    .map(|(id, scene)| Ok((*id, scene.retimed(frame_rate)?)))
                    .collect::<Result<_, ProjectSettingsError>>()?,
            );
        }
        Ok(project)
    }

    pub(in crate::timeline) fn main_scene(&self) -> &SceneDefinition {
        &self.scenes[&self.main_scene]
    }

    pub(in crate::timeline) fn scenes_mut(&mut self) -> &mut HashMap<SceneId, SceneDefinition> {
        Arc::make_mut(&mut self.scenes)
    }
}

/// Immutable, detached timeline state for background save and export jobs.
///
/// Editing state deliberately is not `Clone`: copying an editor used to
/// silently discard its undo/redo history. Callers now request this explicit
/// read model, which contains only the state a background consumer can use.
#[derive(Clone)]
pub struct TimelineSnapshot {
    pub(in crate::timeline) project: Arc<TimelineProject>,
    pub(in crate::timeline) media_cache: Arc<MediaMetadataCache>,
    pub(in crate::timeline) playhead: Frame,
    pub(in crate::timeline) project_revision: u64,
}

impl TimelineSnapshot {
    pub fn with_media_cache(mut self, cache: crate::media::MediaMetadataCache) -> Self {
        self.media_cache = Arc::new(cache);
        self
    }

    pub fn media_dependencies_revision(&self) -> u64 {
        self.project.media_dependencies_revision
    }

    pub fn project_revision(&self) -> u64 {
        self.project_revision
    }

    pub fn project_id(&self) -> ProjectId {
        self.project.id
    }

    pub fn resolution(&self) -> ProjectResolution {
        self.project.resolution
    }

    pub fn beat_guide(&self) -> BeatGuide {
        self.project.beat_guide
    }

    pub fn main_scene_id(&self) -> crate::timeline::SceneId {
        self.project.main_scene
    }

    pub fn scene(&self, id: crate::timeline::SceneId) -> Option<&SceneDefinition> {
        self.project.scenes.get(&id)
    }

    pub fn scenes(&self) -> impl Iterator<Item = &SceneDefinition> {
        self.project.scenes.values()
    }
}

/// Project-wide guide spacing and origin, independent of the frame grid.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct BeatGuide {
    bpm: f32,
    offset_seconds: f32,
}

impl BeatGuide {
    pub const fn new(bpm: f32, offset_seconds: f32) -> Option<Self> {
        if bpm.is_finite() && bpm > 0. && offset_seconds.is_finite() {
            Some(Self {
                bpm,
                offset_seconds,
            })
        } else {
            None
        }
    }

    pub const fn bpm(self) -> f32 {
        self.bpm
    }

    pub const fn offset_seconds(self) -> f32 {
        self.offset_seconds
    }

    pub fn beat_seconds(self) -> f64 {
        60. / f64::from(self.bpm)
    }
}

impl Default for BeatGuide {
    fn default() -> Self {
        Self {
            bpm: 60.,
            offset_seconds: 0.,
        }
    }
}

impl<'de> Deserialize<'de> for BeatGuide {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            bpm: f32,
            offset_seconds: f32,
        }

        let fields = Fields::deserialize(deserializer)?;
        Self::new(fields.bpm, fields.offset_seconds).ok_or_else(|| {
            serde::de::Error::custom(
                "BPM must be finite and greater than zero, and its offset must be finite",
            )
        })
    }
}

/// Pixel dimensions of the project's final frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProjectResolution {
    width: NonZeroU32,
    height: NonZeroU32,
}

impl ProjectResolution {
    pub const MAX_DIMENSION: u32 = 8_192;
    pub const DEFAULT: Self = Self {
        width: NonZeroU32::new(1_920).unwrap(),
        height: NonZeroU32::new(1_080).unwrap(),
    };

    pub const fn new(width: u32, height: u32) -> Option<Self> {
        if width > Self::MAX_DIMENSION || height > Self::MAX_DIMENSION {
            return None;
        }
        match (NonZeroU32::new(width), NonZeroU32::new(height)) {
            (Some(width), Some(height)) => Some(Self { width, height }),
            _ => None,
        }
    }

    pub const fn width(self) -> u32 {
        self.width.get()
    }

    pub const fn height(self) -> u32 {
        self.height.get()
    }

    pub fn aspect_ratio(self) -> f32 {
        self.width() as f32 / self.height() as f32
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ProjectSettingsError {
    #[error("{0}")]
    InvalidFrameRate(String),
    #[error("{0}")]
    OutOfRange(String),
}

impl ProjectSettingsError {
    pub(in crate::timeline) fn out_of_range(message: impl Into<String>) -> Self {
        Self::OutOfRange(message.into())
    }

    fn invalid_frame_rate(message: impl Into<String>) -> Self {
        Self::InvalidFrameRate(message.into())
    }
}

impl FromStr for FrameRate {
    type Err = ProjectSettingsError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        let invalid = || {
            ProjectSettingsError::invalid_frame_rate(
                "Enter the frame rate as a decimal or fraction, such as 30 or 30000/1001",
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
            ProjectSettingsError::invalid_frame_rate("Frame rate must be greater than 0")
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
