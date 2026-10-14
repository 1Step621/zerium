//! Shader tools shared by the application and plugin CLI, without GPU or UI dependencies.

#![deny(unreachable_pub)]

mod commands;
mod compile;
mod compiled;
mod error;
mod generate;
mod loader;
mod source;
mod validation;

pub use commands::{generate, validate};
pub use compile::compile_plugins;
pub use compiled::{
    CompiledEffectShader, CompiledPluginShaders, ComputeShaderDescriptor, EffectShaderDescriptor,
    EffectShaderId, ItemShaderDescriptor, ItemShaderId, TextureShaderDescriptor,
};
pub use error::ShaderError;
pub use loader::{PluginBundle, PluginLibrary, load_embedded_plugin, load_filesystem_plugin};
pub use validation::{validate_compute_shader, validate_render_shader};
pub use zerium_core::plugin::MAX_TEXTURE_INPUTS;

/// Item and effect textures follow manifest declaration order; their shared
/// sampler occupies the binding immediately after the reserved texture slots.
pub const CAPABILITY_SAMPLER_BINDING: usize = MAX_TEXTURE_INPUTS;
