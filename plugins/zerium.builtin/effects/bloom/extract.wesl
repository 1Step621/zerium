import package::generated::host::effect::{context, pixel_position, load_input};
import package::generated::host::compute::store;
import package::generated::support::util::srgb;
import package::generated::bloom::props;
import package::helpers::color::luminance;

@compute @workgroup_size(8, 8, 1)
fn compute_main(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= vec2<u32>(context().output_size)) {
        return;
    }

    let source = load_input(pixel_position(vec2<i32>(id.xy)));
    let linear = source.rgb / max(source.a, 0.00001);
    let luminance = luminance(linear);
    let threshold = srgb(vec3(props().threshold / 100.0)).x;
    let contribution = max(luminance - threshold, 0.0) / max(luminance, 0.00001);
    store(id.xy, source * contribution);
}
