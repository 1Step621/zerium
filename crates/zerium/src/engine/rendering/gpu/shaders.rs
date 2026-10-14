//! Compile plugin shaders into the shared device's GPU pipelines.
use super::bindings::GpuItem;
use super::{PROPERTY_WORD_SIZE, RenderDevice, RenderError, SCENE_FORMAT};
use zerium_shader::{
    ComputeShaderDescriptor, EffectShaderDescriptor, ItemShaderDescriptor, TextureShaderDescriptor,
};

use super::pipelines::{ComputePipeline, RasterPipeline, TexturePipeline, raster_pipeline};
use std::{collections::BTreeMap, num::NonZeroU64};

fn pipeline_constants(overrides: &BTreeMap<String, f64>) -> Vec<(&str, f64)> {
    overrides
        .iter()
        .map(|(key, value)| (key.as_str(), *value))
        .collect()
}

impl RenderDevice {
    pub(super) fn register_compute_shader(
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

    pub(super) fn register_temporal_shader(
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
            &self.temporal_pipeline_layout,
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

    pub(super) fn register_texture_shader(
        &mut self,
        descriptor: TextureShaderDescriptor,
    ) -> Result<(), RenderError> {
        let TextureShaderDescriptor {
            id,
            label,
            wgsl,
            vertex_entry,
            fragment_entry,
            vertex_count,
            input_ids,
        } = descriptor;
        if self.texture_pipelines.contains_key(&id) {
            return Err(RenderError::backend(format!(
                "texture item shader '{}' is already registered",
                id
            )));
        }
        let error_scope = self
            .gpu
            .device
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let input_count = input_ids.len();
        let sampler_binding = u32::try_from(2 + input_count)
            .map_err(|_| RenderError::backend("too many texture inputs"))?;
        let metadata_binding = sampler_binding + 1;
        let mut layout_entries = vec![
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(size_of::<GpuItem>() as u64),
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(PROPERTY_WORD_SIZE as u64),
                },
                count: None,
            },
        ];
        layout_entries.extend((0..input_count).map(|index| wgpu::BindGroupLayoutEntry {
            binding: u32::try_from(index + 2).expect("texture binding index exceeds u32"),
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }));
        layout_entries.push(wgpu::BindGroupLayoutEntry {
            binding: sampler_binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
        layout_entries.push(wgpu::BindGroupLayoutEntry {
            binding: metadata_binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: NonZeroU64::new(((input_count + 1) * 16) as u64),
            },
            count: None,
        });
        let bind_group_layout =
            self.gpu
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("zerium-texture-item-bind-group-layout"),
                    entries: &layout_entries,
                });
        let pipeline_layout =
            self.gpu
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("zerium-texture-item-pipeline-layout"),
                    bind_group_layouts: &[Some(&bind_group_layout)],
                    immediate_size: 0,
                });
        let module = self
            .gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&label),
                source: wgpu::ShaderSource::Wgsl(wgsl.as_ref().into()),
            });
        let pipeline = raster_pipeline(
            &self.gpu.device,
            &pipeline_layout,
            &module,
            [&vertex_entry, &fragment_entry],
            wgpu::ColorTargetState {
                format: SCENE_FORMAT,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            },
            &label,
            &[],
        );
        if let Some(error) = pollster::block_on(error_scope.pop()) {
            return Err(RenderError::backend(format!(
                "texture item shader '{}' is incompatible with the render pipeline: {error}",
                id
            )));
        }
        self.texture_pipelines.insert(
            id,
            TexturePipeline {
                pipeline,
                vertex_count,
                bind_group_layout,
                input_count,
            },
        );
        Ok(())
    }

    /// Registers a kind-specific WGSL fragment. This is the extension point for
    /// future plugins; IDs must be unique for the lifetime of the renderer.
    pub(super) fn register_item_shader(
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

    pub(super) fn register_effect_shader(
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
            &self.effect_pipeline_layout,
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
