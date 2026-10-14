import package::generated::drop_shadow::compute::props;
import package::shared::rgba_blur::blur_rgba;
override direction_x: f32 = 1.0;
override direction_y: f32 = 0.0;
override radius_scale: f32 = 1.0;

@compute @workgroup_size(8, 8, 1)
fn compute_main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    blur_rgba(global_id, props().radius, vec2(direction_x, direction_y), radius_scale);
}
