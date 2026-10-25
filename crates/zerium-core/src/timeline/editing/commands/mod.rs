//! Editing, navigation, selection, and preview commands.
//!
//! Commands validate edit targets and coordinate history. The editor
//! internals it needs are visible only inside `timeline`.

use thiserror::Error;

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum SceneVariableEditError {
    #[error("This variable is used by a calculation. Disconnect its inputs first.")]
    VariableInUse,
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

mod animation;
mod effect;
mod item;
mod property;
mod scene;

pub use self::{
    animation::{AnimationEdit, AnimationEditTarget},
    item::ItemMoveEdit,
    property::{AnimationValuePlan, ValueEdit, ValueEditTarget},
};
