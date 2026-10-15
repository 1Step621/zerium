import package::generated::host::compute::{context, effect_input, store};
import package::generated::host::util::srgb;
import package::generated::bloom::compute::props;
import package::helpers::color::luminance;

@compute @workgroup_size(8, 8, 1)
fn compute_main(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= vec2<u32>(context().output_size)) {
        return;
    }
    let source = textureLoad(effect_input, vec2<i32>(id.xy), 0);
    let linear = source.rgb / max(source.a, 0.00001);
    let luminance = luminance(linear);
    let threshold = srgb(vec3(props().threshold / 100.0)).x;
    let contribution = max(luminance - threshold, 0.0) / max(luminance, 0.00001);
    store(id.xy, source * contribution);
}
