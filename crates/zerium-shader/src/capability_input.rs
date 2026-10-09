use zerium_core::plugin::MAX_TEXTURE_INPUTS;

/// All item and effect shaders use the same binding layout. The names come
/// from the manifest, while the binding positions follow declaration order.
pub const MAX_INPUTS: usize = MAX_TEXTURE_INPUTS;
pub const SAMPLER_BINDING: usize = MAX_INPUTS;
