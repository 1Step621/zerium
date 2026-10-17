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
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct GpuSurface {
    pub(super) output_size: [f32; 2],
    pub(super) composition_size: [f32; 2],
    pub(super) surface_min: [f32; 2],
    pub(super) surface_size: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct GpuPass {
    pub property_offset: u32,
    pub property_size: u32,
    pub sample_index: u32,
    pub sample_count: u32,
    pub frame_offset: f32,
    pub sample_progress: f32,
    pub composition_size: [f32; 2],
    pub surface_min: [f32; 2],
    pub surface_size: [f32; 2],
    pub output_size: [u32; 2],
    pub bounds_min: [f32; 2],
    pub bounds_size: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct GpuImage {
    pub bounds_min: [f32; 2],
    pub bounds_size: [f32; 2],
    pub raster_min: [f32; 2],
    pub raster_size: [f32; 2],
    pub density: [f32; 2],
    pub padding: [f32; 2],
}

impl GpuImage {
    pub(super) fn new(bounds: SurfaceRect, raster: SurfaceRect, size: RenderSize) -> Self {
        let extent = |rect: SurfaceRect| {
            [
                (rect.max[0] - rect.min[0]) as f32,
                (rect.max[1] - rect.min[1]) as f32,
            ]
        };
        let raster_size = extent(raster);
        Self {
            bounds_min: bounds.min.map(|value| value as f32),
            bounds_size: extent(bounds),
            raster_min: raster.min.map(|value| value as f32),
            raster_size,
            density: [
                size.width as f32 / raster_size[0],
                size.height as f32 / raster_size[1],
            ],
            padding: [0.; 2],
        }
    }
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
        let inverse = view.inverse_transform();
        let row = |axis: usize| {
            let density = f64::from([input_size.width, input_size.height][axis])
                / (input.max[axis] - input.min[axis]);
            [
                (inverse[axis][0] * (output.max[0] - output.min[0]) * density
                    / f64::from(output_size.width)) as f32,
                (inverse[axis][1] * (output.max[1] - output.min[1]) * density
                    / f64::from(output_size.height)) as f32,
                ((inverse[axis][2]
                    + inverse[axis][0] * output.min[0]
                    + inverse[axis][1] * output.min[1]
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
