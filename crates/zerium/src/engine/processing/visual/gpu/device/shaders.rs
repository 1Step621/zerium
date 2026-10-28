//! Compile validated shader descriptions into device-specific GPU pipelines.

use crate::engine::{
    GpuContext,
    processing::visual::{RenderError, gpu::SCENE_FORMAT},
};
use std::collections::BTreeMap;
use zerium_shader::{ComputeShaderDescriptor, EffectShaderDescriptor, ItemShaderDescriptor};

fn pipeline_constants(overrides: &BTreeMap<String, f64>) -> Vec<(&str, f64)> {
    overrides
        .iter()
        .map(|(key, value)| (key.as_str(), *value))
        .collect()
}

pub(super) fn compute_pipeline(
    gpu: &GpuContext,
    layout: &wgpu::PipelineLayout,
    descriptor: &ComputeShaderDescriptor,
) -> Result<ComputePipeline, RenderError> {
    let error_scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(descriptor.id.as_str()),
            source: wgpu::ShaderSource::Wgsl(descriptor.wgsl.as_ref().into()),
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(descriptor.id.as_str()),
            layout: Some(layout),
            module: &module,
            entry_point: Some(&descriptor.entry),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &pipeline_constants(&descriptor.overrides),
                ..Default::default()
            },
            cache: None,
        });
    if let Some(error) = pollster::block_on(error_scope.pop()) {
        return Err(RenderError::backend(format!(
            "compute effect shader '{}' is incompatible with the render pipeline: {error}",
            descriptor.id
        )));
    }
    Ok(ComputePipeline {
        pipeline,
        workgroup_size: descriptor.workgroup_size,
    })
}

pub(super) fn item_pipeline(
    gpu: &GpuContext,
    layout: &wgpu::PipelineLayout,
    descriptor: &ItemShaderDescriptor,
) -> Result<RasterPipeline, RenderError> {
    let error_scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&descriptor.label),
            source: wgpu::ShaderSource::Wgsl(descriptor.wgsl.as_ref().into()),
        });
    let pipeline = raster_pipeline(
        &gpu.device,
        layout,
        &module,
        [&descriptor.vertex_entry, &descriptor.fragment_entry],
        wgpu::ColorTargetState {
            format: SCENE_FORMAT,
            blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        },
        &descriptor.label,
        &[],
    );
    if let Some(error) = pollster::block_on(error_scope.pop()) {
        return Err(RenderError::backend(format!(
            "item shader '{}' is incompatible with the render pipeline: {error}",
            descriptor.id
        )));
    }
    Ok(RasterPipeline {
        pipeline,
        vertex_count: descriptor.vertex_count,
    })
}

pub(super) fn effect_pipeline(
    gpu: &GpuContext,
    layout: &wgpu::PipelineLayout,
    descriptor: &EffectShaderDescriptor,
) -> Result<RasterPipeline, RenderError> {
    let error_scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&descriptor.label),
            source: wgpu::ShaderSource::Wgsl(descriptor.wgsl.as_ref().into()),
        });
    let pipeline = raster_pipeline(
        &gpu.device,
        layout,
        &module,
        [&descriptor.vertex_entry, &descriptor.fragment_entry],
        wgpu::ColorTargetState {
            format: SCENE_FORMAT,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        },
        &descriptor.label,
        &pipeline_constants(&descriptor.overrides),
    );
    if let Some(error) = pollster::block_on(error_scope.pop()) {
        return Err(RenderError::backend(format!(
            "effect shader '{}' is incompatible with the render pipeline: {error}",
            descriptor.id
        )));
    }
    Ok(RasterPipeline {
        pipeline,
        vertex_count: 3,
    })
}

pub(in crate::engine::processing::visual::gpu) struct RasterPipeline {
    pub(in crate::engine::processing::visual::gpu) pipeline: wgpu::RenderPipeline,
    pub(in crate::engine::processing::visual::gpu) vertex_count: u32,
}

pub(in crate::engine::processing::visual::gpu) struct ComputePipeline {
    pub(in crate::engine::processing::visual::gpu) pipeline: wgpu::ComputePipeline,
    pub(in crate::engine::processing::visual::gpu) workgroup_size: [u32; 3],
}

pub(in crate::engine::processing::visual::gpu) fn raster_pipeline<'a>(
    device: &wgpu::Device,
    layout: impl Into<Option<&'a wgpu::PipelineLayout>>,
    module: &wgpu::ShaderModule,
    entries: [&str; 2],
    target: wgpu::ColorTargetState,
    label: &str,
    constants: &[(&str, f64)],
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: layout.into(),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(entries[0]),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants,
                ..Default::default()
            },
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(entries[1]),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants,
                ..Default::default()
            },
            targets: &[Some(target)],
        }),
        multiview_mask: None,
        cache: None,
    })
}
