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
mod shader_contract;
mod validation;

pub(crate) use abi::{PropertyLayout, abi_size, scalar_abi_size, value_string_count};
pub(crate) use bounds::{OutputBoundsSchema, program_context};
pub(crate) use bundle::Plugin;
pub(crate) use capability::{
    Capability, FileCapability, MAX_CAPABILITIES, MediaType, TextCapability,
};
pub(crate) use category::CatalogCategory;
pub(crate) use effect::{
    ComputeDispatchDimension, EffectInputSpace, EffectPassSchema, EffectSchema, PassConstantSchema,
    PassConstantValue,
};
pub(crate) use error::PluginError;
pub(crate) use item::ItemSchema;
pub(crate) use manifest::PluginManifest;
pub(crate) use registry::PluginRegistry;
pub(crate) use shader::{ShaderKind, ShaderSchema};
pub(crate) use shader_contract::{
    ShaderContract, ShaderProperty, shader_contract_fingerprint, shader_contracts,
};

pub(crate) trait PluginCatalogEntry {
    fn id(&self) -> &str;
    fn label(&self) -> &str;
    fn category_id(&self) -> &str;
    fn category(&self) -> &str;
    fn tags(&self) -> &[String];
}
