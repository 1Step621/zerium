#![deny(unreachable_pub)]

mod abi;
mod bounds;
mod bundle;
mod capability;
mod category;
mod editor;
mod effect;
mod error;
mod identifier;
mod item;
mod manifest;
mod passes;
mod registry;
mod shader;
mod validation;

pub use abi::{PropertyLayout, abi_size, scalar_abi_size, value_string_count};
pub use bounds::{OutputBoundsSchema, program_context};
pub use bundle::Plugin;
pub use capability::{
    AudioCapability, MAX_DECIMAL_PLACES, MAX_TEXTURE_INPUTS, MediaPlaybackSchema,
    PlaybackProperties, TextStyle, TextureInput, TimeMappingProperties,
};
pub use category::{EffectCategory, ItemCategory};
pub use editor::EditorCapability;
pub use effect::{EffectInputSpace, EffectRenderSchema, EffectSchema};
pub use error::PluginError;
pub use identifier::validate_wgsl_identifier;
pub use item::{ItemRenderSchema, ItemSchema};
pub use manifest::PluginManifest;
pub use passes::{
    ComputeDispatchDimension, EffectPassSchema, PassConstantSchema, PassConstantValue,
};
pub use registry::PluginRegistry;
pub use shader::{ShaderKind, ShaderSchema};

pub trait PluginCatalogEntry {
    type Category;

    fn id(&self) -> &str;

    fn label(&self) -> &str;

    fn category(&self) -> Self::Category;

    fn tags(&self) -> &[String];
}
