#![deny(unreachable_pub)]

mod abi;
mod bounds;
pub(crate) use bounds::register_functions as register_script_functions;
mod capability;
mod category;
mod editor;
mod effect;
mod error;
mod item;
mod manifest;
mod passes;
mod playback;
mod registry;
mod shader;
mod validation;

pub use abi::{PropertyLayout, abi_size, scalar_abi_size};
pub use bounds::{OutputBoundsSchema, program_context};
pub(crate) use capability::initialize_frame_rates;
pub use capability::{
    AudioCapability, MAX_DECIMAL_PLACES, MAX_TEXTURE_INPUTS, RenderResultView, TextStyle,
    TextureInput,
};
pub use category::{EffectCategory, ItemCategory};
pub use editor::EditorCapability;
pub use effect::{EffectRenderSchema, EffectSchema};
pub use error::PluginError;
pub use item::{ItemRenderSchema, ItemSchema};
pub use manifest::PluginManifest;
pub use passes::{ComputeDispatchDimension, EffectPassSchema};
pub use playback::ItemPlaybackSchema;
pub use registry::PluginRegistry;
pub use shader::ShaderSchema;
pub use validation::validate_wgsl_identifier;

pub trait PluginCatalogEntry {
    type Category;

    fn id(&self) -> &str;

    fn label(&self) -> &str;

    fn category(&self) -> Self::Category;

    fn tags(&self) -> &[String];
}
