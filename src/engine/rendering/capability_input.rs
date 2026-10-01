use crate::domain::plugin::MAX_CAPABILITIES;

/// All item and effect shaders use the same binding layout. The names come
/// from the manifest, while the binding positions follow declaration order.
pub(crate) const MAX_INPUTS: usize = MAX_CAPABILITIES;
pub(crate) const SAMPLER_BINDING: usize = MAX_INPUTS;
