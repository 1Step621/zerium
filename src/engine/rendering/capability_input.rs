use crate::domain::plugin::{Capability, MAX_CAPABILITIES};

/// All item and effect shaders use the same binding layout. The names come
/// from the manifest, while the binding positions follow declaration order.
pub(crate) const MAX_INPUTS: usize = MAX_CAPABILITIES;
pub(crate) const SAMPLER_BINDING: usize = MAX_INPUTS;

pub(crate) fn interface(capabilities: &[Capability]) -> String {
    let mut source = String::new();
    for (binding, capability) in capabilities.iter().enumerate() {
        source.push_str(&format!(
            "@group(1) @binding({binding})\nvar {}: texture_2d<f32>;\n\n",
            capability.id()
        ));
    }
    source.push_str(&format!(
        "@group(1) @binding({SAMPLER_BINDING})\nvar capability_sampler: sampler;\n"
    ));
    source
}
