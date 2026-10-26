//! Timeline models, editing operations, and evaluated read views.
use thiserror::Error;

mod editing;
mod evaluation;
mod model;

pub use editing::{
    AnimationEdit, AnimationEditTarget, AnimationValuePlan, EditContext, EditSession, ItemMoveEdit,
    SceneVariableEditError, TimelineEditor, ValueEdit, ValueEditTarget,
};
pub use evaluation::{
    AudioCalculationDiagnostic, AudioClipId, AudioTimelineSnapshot, AudioTimelineSource,
    EvaluatedAudioGain, EvaluatedClip, EvaluatedEffect, EvaluatedItem, EvaluatedSceneNode,
    RenderEvaluation, RenderPlacement, RenderPropertyTarget, RenderResultSettings,
    ResolvedRenderProperties, TimelineView,
};
pub use model::{
    EffectInstanceId, ItemId, LayerId, ProjectId, SceneId,
    document::{ResizeEdge, ResizeMode, TimelineDocument},
    item::{
        AspectRatio, BlendMode, EffectInstance, ItemCreation, ItemInterval, ItemSource,
        PropertyAddress, TimelineItem, TimelineItemKind,
    },
    project::{BeatGuide, ProjectResolution, ProjectSettingsError, TimelineSnapshot},
    scene::{
        CalculationDiagnostic, CalculationFailure, SceneBindingOwner, SceneBindingTarget,
        SceneCalculation, SceneCalculationInput, SceneDefinition, SceneEvaluation, SceneVariable,
        SceneVariablePreset, SceneVariableSource, resolve_scene_binding,
    },
    time::{Frame, FrameDuration, FrameRate, TimeMapping, TimelineTime},
};

/// An expected reason why an editor command could not be applied.
///
/// Commands that merely report whether they changed state still return
/// `bool`; commands that can reject valid-looking user input use this type so
/// the UI does not have to guess why they failed.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum TimelineEditError {
    #[error("Item '{plugin_id}/{item_id}' was not found")]
    PluginItemNotFound { plugin_id: String, item_id: String },
    #[error("Effect '{plugin_id}/{effect_id}' was not found")]
    PluginEffectNotFound {
        plugin_id: String,
        effect_id: String,
    },
    #[error("Scene {} was not found", .0.get())]
    SceneNotFound(SceneId),
    #[error("Scene reference cycle detected")]
    RecursiveSceneReference,
    #[error("No target item is selected")]
    NothingSelected,
    #[error("Effects require a visual item")]
    NonVisualItem,
    #[error(transparent)]
    InvalidTiming(ProjectSettingsError),
    #[error("Could not allocate a new ID")]
    IdentifierExhausted,
    #[error("Cannot place item at the specified position")]
    PlacementUnavailable,
    #[error("Item {} was not found", .0.get())]
    ItemNotFound(ItemId),
    #[error("Invalid value for property '{0}'")]
    InvalidPropertyValue(String),
    #[error("File type does not match the item input")]
    IncompatibleMedia,
    #[error("Playback speed must be between 25% and 400%")]
    InvalidPlaybackSpeed,
    #[error("Source interval must have a finite nonnegative start and a finite positive duration")]
    InvalidSourceRange,
}
