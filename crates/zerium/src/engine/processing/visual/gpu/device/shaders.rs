//! Compile plugin shaders into the shared device's GPU pipelines.

use super::RenderDevice;
use crate::engine::processing::visual::{RenderError, gpu::SCENE_FORMAT};
use std::collections::BTreeMap;
use zerium_shader::{ComputeShaderDescriptor, EffectShaderDescriptor, ItemShaderDescriptor};

fn pipeline_constants(overrides: &BTreeMap<String, f64>) -> Vec<(&str, f64)> {
    overrides
        .iter()
        .map(|(key, value)| (key.as_str(), *value))
        .collect()
}

impl RenderDevice {
    pub(in crate::engine::processing::visual::gpu) fn register_compute_shader(
        &mut self,
        descriptor: ComputeShaderDescriptor,
    ) -> Result<(), RenderError> {
        let ComputeShaderDescriptor {
            id,
            wgsl,
            entry,
            workgroup_size,
            overrides,
        } = descriptor;

        if self.compute_pipelines.contains_key(&id) {
            return Err(RenderError::backend(format!(
                "compute effect shader '{}' is already registered",
                id
            )));
        }

        let error_scope = self
            .gpu
            .device
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self
            .gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(id.as_str()),
                source: wgpu::ShaderSource::Wgsl(wgsl.as_ref().into()),
            });
        let layout = self
            .gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("zerium-compute-pipeline-layout"),
                bind_group_layouts: &[
                    Some(&self.compute_bind_group_layout),
                    Some(&self.capability_bind_group_layout),
                ],
                immediate_size: 0,
            });
        let pipeline = self
            .gpu
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(id.as_str()),
                layout: Some(&layout),
                module: &module,
                entry_point: Some(&entry),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &pipeline_constants(&overrides),
                    ..Default::default()
                },
                cache: None,
            });

        if let Some(error) = pollster::block_on(error_scope.pop()) {
            return Err(RenderError::backend(format!(
                "compute effect shader '{}' is incompatible with the render pipeline: {error}",
                id
            )));
        }

        self.compute_pipelines.insert(
            id,
            ComputePipeline {
                pipeline,
                workgroup_size,
            },
        );
        Ok(())
    }

    pub(in crate::engine::processing::visual::gpu) fn register_temporal_shader(
        &mut self,
        descriptor: EffectShaderDescriptor,
    ) -> Result<(), RenderError> {
        let EffectShaderDescriptor {
            id,
            label,
            wgsl,
            vertex_entry,
            fragment_entry,
            overrides,
        } = descriptor;

        if self.temporal_pipelines.contains_key(&id) {
            return Err(RenderError::backend(format!(
                "temporal effect reducer '{}' is already registered",
                id
            )));
        }

        let error_scope = self
            .gpu
            .device
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self
            .gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&label),
                source: wgpu::ShaderSource::Wgsl(wgsl.as_ref().into()),
            });
        let pipeline = raster_pipeline(
            &self.gpu.device,
            &self.pass_pipeline_layout,
            &module,
            [&vertex_entry, &fragment_entry],
            wgpu::ColorTargetState {
                format: SCENE_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            },
            &label,
            &pipeline_constants(&overrides),
        );

        if let Some(error) = pollster::block_on(error_scope.pop()) {
            return Err(RenderError::backend(format!(
                "temporal effect reducer '{}' is incompatible with the render pipeline: {error}",
                id
            )));
        }

        self.temporal_pipelines.insert(
            id,
            RasterPipeline {
                pipeline,
                vertex_count: 3,
            },
        );
        Ok(())
    }

    pub(in crate::engine::processing::visual::gpu) fn register_item_shader(
        &mut self,
        descriptor: ItemShaderDescriptor,
    ) -> Result<(), RenderError> {
        if descriptor.id.as_str().is_empty() {
            return Err(RenderError::backend("item shader ID must not be empty"));
        }

        if self.item_pipelines.contains_key(&descriptor.id) {
            return Err(RenderError::backend(format!(
                "item shader '{}' is already registered",
                descriptor.id
            )));
        }

        let error_scope = self
            .gpu
            .device
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self
            .gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&descriptor.label),
                source: wgpu::ShaderSource::Wgsl(descriptor.wgsl.as_ref().into()),
            });
        let pipeline = raster_pipeline(
            &self.gpu.device,
            &self.pipeline_layout,
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

        self.item_pipelines.insert(
            descriptor.id,
            RasterPipeline {
                pipeline,
                vertex_count: descriptor.vertex_count,
            },
        );
        Ok(())
    }

    pub(in crate::engine::processing::visual::gpu) fn register_effect_shader(
        &mut self,
        descriptor: EffectShaderDescriptor,
    ) -> Result<(), RenderError> {
        if descriptor.id.as_str().is_empty() {
            return Err(RenderError::backend("effect shader ID must not be empty"));
        }

        if self.effect_pipelines.contains_key(&descriptor.id) {
            return Err(RenderError::backend(format!(
                "effect shader '{}' is already registered",
                descriptor.id
            )));
        }

        let error_scope = self
            .gpu
            .device
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self
            .gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&descriptor.label),
                source: wgpu::ShaderSource::Wgsl(descriptor.wgsl.as_ref().into()),
            });
        let pipeline = raster_pipeline(
            &self.gpu.device,
            &self.pass_pipeline_layout,
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

        self.effect_pipelines.insert(
            descriptor.id,
            RasterPipeline {
                pipeline,
                vertex_count: 3,
            },
        );
        Ok(())
    }
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
