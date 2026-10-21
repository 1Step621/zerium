//! Editing, navigation, selection, and preview commands.
//!
//! Commands validate edit targets and coordinate history. The editor
//! internals it needs are visible only inside `timeline`.

use thiserror::Error;

use super::{ItemId, SceneId, TimelineEditor};

/// Identifies the document state for which an edit was resolved.
#[derive(Clone, Copy, PartialEq, Eq)]
struct EditContext {
    revision: u64,
    project_id: super::ProjectId,
    scene_id: Option<SceneId>,
}

pub use edit_session::EditSession;

impl TimelineEditor {
    fn edit_context(&self) -> EditContext {
        EditContext {
            revision: self.project_revision(),
            project_id: self.project().id,
            scene_id: self.active_scene_id(),
        }
    }

    pub fn begin_edit_gesture(&self) -> EditSession {
        EditSession::new(self, edit_session::UndoPolicy::Gesture)
    }

    pub fn edit_gesture(
        &mut self,
        gesture: &mut EditSession,
        update: impl FnOnce(&mut Self),
    ) -> bool {
        self.update_edit_session(gesture, |editor| {
            let before = editor.edit_context();
            update(editor);
            before != editor.edit_context()
        })
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum SceneVariableEditError {
    #[error("This variable is used by a calculation. Disconnect its inputs first.")]
    VariableInUse,
    #[error("No scene is active")]
    NoActiveScene,
    #[error("Scene variable was not found")]
    VariableNotFound,
    #[error("Binding target was not found")]
    TargetNotFound,
    #[error("Binding target is already connected")]
    TargetAlreadyBound,
    #[error("Binding target is animated")]
    TargetAnimated,
    #[error("Binding target does not support scene variables")]
    TargetNotBindable,
    #[error("Value or binding does not match the variable type")]
    IncompatibleContract,
}

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
    #[error("Could not allocate a new ID")]
    IdentifierExhausted,
    #[error("Cannot place item at the specified position")]
    PlacementUnavailable,
    #[error("Item {} was not found", .0.get())]
    ItemNotFound(ItemId),
    #[error("File type does not match the item input")]
    IncompatibleMedia,
    #[error("Playback speed must be between 25% and 400%")]
    InvalidPlaybackSpeed,
    #[error("Source interval must have a finite nonnegative start and a finite positive duration")]
    InvalidSourceRange,
}

mod animation;
mod animation_edit;
mod edit_session;
mod effect;
mod item;
mod property;
mod scene;
mod session;
mod value_edit;

pub use animation_edit::{AnimationEdit, AnimationEditTarget};
pub use value_edit::{AnimationValuePlan, ValueEdit, ValueEditTarget};
