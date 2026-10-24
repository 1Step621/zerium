mod aspect_ratio;
mod audio;
mod blend_mode;
mod commands;
mod diagnostic;
mod document;
mod editor;
mod evaluated;
mod evaluation;
mod history;
mod ids;
mod item;
mod media;
mod placement;
mod project;
mod properties;
mod property_address;
mod render;
mod scene;
mod selection;
mod settings;
mod time;
mod time_mapping;
mod view;
mod visibility;

pub use aspect_ratio::AspectRatio;
pub use blend_mode::BlendMode;
pub use commands::{
    AnimationEdit, AnimationEditTarget, AnimationValuePlan, EditContext, EditSession,
    SceneVariableEditError, TimelineEditError, ValueEdit, ValueEditTarget,
};
pub use diagnostic::{AudioCalculationDiagnostic, CalculationDiagnostic, CalculationFailure};
pub use document::TimelineDocument;
pub use document::{ResizeEdge, ResizeMode};
pub use editor::TimelineEditor;
pub use evaluated::{EvaluatedEffect, EvaluatedItem};
pub use evaluation::{EvaluatedClip, EvaluatedSceneNode};
pub use ids::{EffectInstanceId, ItemId, LayerId, ProjectId, SceneId};
pub use item::{EffectInstance, RenderResultSettings, TimelineItem, TimelineItemKind};
pub use property_address::PropertyAddress;
pub use scene::SceneDefinition;
pub use scene::{
    SceneBindingOwner, SceneBindingTarget, SceneCalculation, SceneCalculationInput,
    SceneEvaluation, SceneVariable, SceneVariablePreset, SceneVariableSource,
    resolve_scene_binding,
};
pub use settings::{BeatGuide, ProjectResolution};
pub use time::{Frame, FrameDuration, FrameRate, TimelineTime};
pub use time_mapping::TimeMapping;
pub use view::{TimelineSnapshot, TimelineView};

pub use audio::{AudioClipId, AudioTimelineSnapshot, AudioTimelineSource, EvaluatedAudioGain};

pub use render::{
    RenderEvaluation, RenderPlacement, RenderPropertyTarget, ResolvedRenderProperties,
};
