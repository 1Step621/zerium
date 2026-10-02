#![deny(unreachable_pub)]

mod abi;
mod bounds;
mod bundle;
mod capability;
mod category;
mod effect;
mod error;
mod identifier;
mod item;
mod manifest;
mod registry;
mod shader;
mod validation;

pub use abi::{PropertyLayout, abi_size, scalar_abi_size, value_string_count};
pub use bounds::{OutputBoundsSchema, program_context};
pub use bundle::Plugin;
pub use capability::{Capability, FileCapability, MAX_CAPABILITIES, MediaType, TextCapability};
pub use category::CatalogCategory;
pub use effect::{
    ComputeDispatchDimension, EffectInputSpace, EffectPassSchema, EffectSchema, PassConstantSchema,
    PassConstantValue,
};
pub use error::PluginError;
pub use identifier::validate_wgsl_identifier;
pub use item::ItemSchema;
pub use manifest::PluginManifest;
pub use registry::PluginRegistry;
pub use shader::{ShaderKind, ShaderSchema};

pub trait PluginCatalogEntry {
    fn id(&self) -> &str;
    fn label(&self) -> &str;
    fn category_id(&self) -> &str;
    fn category(&self) -> &str;
    fn tags(&self) -> &[String];
}
