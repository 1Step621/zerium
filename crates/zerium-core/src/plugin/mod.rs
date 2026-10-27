#![deny(unreachable_pub)]

use thiserror::Error;

mod abi;
mod registry;
mod schema;

pub use abi::{PropertyLayout, abi_size, scalar_abi_size};
pub use registry::PluginRegistry;
pub use schema::{
    PluginManifest, ShaderSchema,
    bounds::{OutputBoundsSchema, program_context},
    capability::{
        AudioCapability, MAX_DECIMAL_PLACES, MAX_TEXTURE_INPUTS, RenderResultView, TextStyle,
        TextureInput,
    },
    editor::EditorCapability,
    effect::{
        EffectCategory, EffectRenderSchema, EffectSchema,
        passes::{ComputeDispatchDimension, EffectPassSchema},
    },
    item::{ItemCategory, ItemRenderSchema, ItemSchema, playback::ItemPlaybackSchema},
    validation::validate_wgsl_identifier,
};
pub(crate) use schema::{
    bounds::register_functions as register_script_functions, capability::initialize_frame_rates,
};

pub trait PluginCatalogEntry {
    type Category;

    fn id(&self) -> &str;

    fn label(&self) -> &str;

    fn icon(&self) -> &str;

    fn category(&self) -> Self::Category;

    fn tags(&self) -> &[String];
}

/// Failure to decode, validate, or resolve a plugin bundle.
#[derive(Debug, Error)]
pub enum PluginError {
    #[error("{message}")]
    InvalidManifest {
        message: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("{0}")]
    InvalidDefinition(String),
    #[error("{0}")]
    MissingAsset(String),
}

impl PluginError {
    pub fn invalid_definition(message: impl Into<String>) -> Self {
        Self::InvalidDefinition(message.into())
    }

    pub fn invalid_manifest(message: impl Into<String>, source: serde_json::Error) -> Self {
        Self::InvalidManifest {
            message: message.into(),
            source,
        }
    }

    pub fn missing_asset(message: impl Into<String>) -> Self {
        Self::MissingAsset(message.into())
    }
}

impl From<crate::property::PropertyError> for PluginError {
    fn from(error: crate::property::PropertyError) -> Self {
        Self::invalid_definition(error.to_string())
    }
}
