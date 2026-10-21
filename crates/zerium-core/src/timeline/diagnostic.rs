//! Evaluation failures retain connections and source locations until presentation.
use super::{Frame, PropertyAddress, SceneId};
use std::sync::Arc;
use thiserror::Error;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum CalculationFailure {
    #[error("{0}")]
    InvalidGraph(String),
    #[error("Missing variable '{0}'")]
    MissingVariable(String),
    #[error("Missing instance argument '{0}'")]
    MissingArgument(String),
    #[error("Invalid argument '{0}'")]
    InvalidArgument(String),
    #[error("{0}")]
    Script(String),
    #[error("Input '{name}': {cause}")]
    Input {
        name: String,
        variable_id: String,
        #[source]
        cause: Arc<CalculationFailure>,
    },
    #[error(transparent)]
    Argument(#[from] Arc<CalculationDiagnostic>),
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("Scene '{scene_name}' variable '{variable_label}' (item {item}): {cause}", item = address.item_id.get())]
pub struct CalculationDiagnostic {
    pub scene_id: SceneId,
    pub scene_name: String,
    pub variable_id: String,
    pub variable_label: String,
    pub address: PropertyAddress,
    #[source]
    pub cause: CalculationFailure,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("At audio frame {frame}: {diagnostic}", frame = frame.get())]
pub struct AudioCalculationDiagnostic {
    pub frame: Frame,
    pub clip: super::AudioClipId,
    #[source]
    pub diagnostic: CalculationDiagnostic,
}
