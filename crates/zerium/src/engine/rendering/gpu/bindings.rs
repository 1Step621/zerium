//! CPU data layouts consumed by shader storage and uniform buffers.
use super::scene::SurfaceRect;
use super::scene::{RenderSize, RenderView};
use bytemuck::{Pod, Zeroable};
use zerium_core::timeline::BlendMode;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct GpuItem {
    pub(super) property_offset: u32,
    pub(super) property_size: u32,
    pub(super) output_size: [f32; 2],
    pub(super) composition_size: [f32; 2],
    pub(super) surface_min: [f32; 2],
    pub(super) surface_size: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct GpuEffect {
    pub(super) property_offset: u32,
    pub(super) property_size: u32,
    pub(super) sample_index: u32,
    pub(super) sample_count: u32,
    pub(super) frame_offset: f32,
    pub(super) sample_progress: f32,
    pub(super) composition_size: [f32; 2],
    pub(super) surface_min: [f32; 2],
    pub(super) surface_size: [f32; 2],
    pub(super) input_min: [f32; 2],
    pub(super) input_size: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct GpuTextureInput {
    pub(super) size: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct GpuCompute {
    pub(super) property_offset: u32,
    pub(super) property_size: u32,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) composition_size: [f32; 2],
    pub(super) surface_min: [f32; 2],
    pub(super) surface_size: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct GpuComposite {
    pub(super) input_size: [u32; 2],
    pub(super) output_size: [u32; 2],
    pub(super) source_x: [f32; 4],
    pub(super) source_y: [f32; 4],
    // Axis-aligned source-texel footprint of one output pixel.
    pub(super) footprint: [f32; 2],
    pub(super) copy_pixels: u32,
    pub(super) _padding: u32,
    pub(super) blend_mode: [u32; 4],
}

impl GpuComposite {
    pub(super) fn new(
        input_size: RenderSize,
        input: SurfaceRect,
        output_size: RenderSize,
        output: SurfaceRect,
        view: RenderView,
        blend_mode: BlendMode,
    ) -> Self {
        let (sin, cos) = view.angle.to_radians().sin_cos();
        let inverse = [
            [cos / view.zoom, -sin / view.zoom],
            [sin / view.zoom, cos / view.zoom],
        ];
        let row = |axis: usize| {
            let density = f64::from([input_size.width, input_size.height][axis])
                / (input.max[axis] - input.min[axis]);
            [
                (f64::from(inverse[axis][0]) * (output.max[0] - output.min[0]) * density
                    / f64::from(output_size.width)) as f32,
                (f64::from(inverse[axis][1]) * (output.max[1] - output.min[1]) * density
                    / f64::from(output_size.height)) as f32,
                ((f64::from(view.position[axis])
                    + f64::from(inverse[axis][0]) * output.min[0]
                    + f64::from(inverse[axis][1]) * output.min[1]
                    - input.min[axis])
                    * density) as f32,
                0.,
            ]
        };
        let source_x = row(0);
        let source_y = row(1);
        let footprint = [
            (source_x[0].abs() + source_x[1].abs()).max(1.),
            (source_y[0].abs() + source_y[1].abs()).max(1.),
        ];
        let copy_pixels = input_size == output_size
            && source_x[..3] == [1., 0., 0.]
            && source_y[..3] == [0., 1., 0.];
        Self {
            input_size: [input_size.width, input_size.height],
            output_size: [output_size.width, output_size.height],
            source_x,
            source_y,
            footprint,
            copy_pixels: u32::from(copy_pixels),
            _padding: 0,
            blend_mode: [blend_mode as u32, 0, 0, 0],
        }
    }
}
