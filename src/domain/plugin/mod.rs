#![deny(unreachable_pub)]

mod abi;
mod bundle;
mod capability;
mod catalog;
mod effect;
mod error;
mod identifier;
mod item;
mod manifest;
mod registry;
mod shader;
mod validation;

pub(crate) use abi::{PropertyLayout, abi_size, scalar_abi_size, value_string_count};
pub(crate) use bundle::Plugin;
pub(crate) use capability::{FileCapability, MediaType, VisualCapability};
pub(crate) use catalog::PluginCatalogEntry;
pub(crate) use effect::{
    ComputeDispatchDimension, EffectPassSchema, EffectSchema, PassConstantSchema, PassConstantValue,
};
pub(crate) use error::PluginError;
pub(crate) use item::ItemSchema;
pub(crate) use manifest::PluginManifest;
pub(crate) use registry::PluginRegistry;
pub(crate) use shader::{ShaderKind, ShaderSchema};
