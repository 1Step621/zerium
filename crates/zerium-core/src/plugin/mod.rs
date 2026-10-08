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
    AudioCapability, Capability, MAX_CAPABILITIES, MediaPlaybackSchema, PlaybackProperties,
    TextCapability, TimeMappingProperties,
};
pub use category::CatalogCategory;
pub use editor::EditorCapability;
pub use effect::{EffectInputSpace, EffectSchema};
pub use error::PluginError;
pub use identifier::validate_wgsl_identifier;
pub use item::ItemSchema;
pub use manifest::PluginManifest;
pub use passes::{
    ComputeDispatchDimension, EffectPassSchema, PassConstantSchema, PassConstantValue,
};
pub use registry::PluginRegistry;
pub use shader::{ShaderKind, ShaderSchema};

pub trait PluginCatalogEntry {
    fn id(&self) -> &str;

    fn label(&self) -> &str;

    fn category_id(&self) -> &str;

    fn category(&self) -> &str;

    fn tags(&self) -> &[String];
}
