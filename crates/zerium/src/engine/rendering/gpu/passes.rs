//! Item, texture, effect, compute, compositing, and display pass encoding.
use super::bindings::{GpuComposite, GpuEffect};
use super::plan::{EffectPassCommand, EffectPassCommandKind, TemporalReduceCommand};
use super::resources::NodeResources;
use super::{RenderError, RenderSession};
use std::{num::NonZeroU64, ops::Range};
use zerium_core::plugin::ComputeDispatchDimension;
use zerium_shader::{EffectShaderId, ItemShaderId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RenderOutput {
    EffectA,
    EffectB,
}

impl RenderOutput {
    pub(super) fn view(self, resources: &NodeResources) -> &wgpu::TextureView {
        match self {
            Self::EffectA => &resources.targets[0].view,
            Self::EffectB => &resources.targets[1].view,
        }
    }

    pub(super) fn texture(self, resources: &NodeResources) -> &wgpu::Texture {
        match self {
            Self::EffectA => &resources.targets[0].texture,
            Self::EffectB => &resources.targets[1].texture,
        }
    }

    pub(super) fn next_effect_target(self) -> Self {
        if self == Self::EffectA {
            Self::EffectB
        } else {
            Self::EffectA
        }
    }
}

pub(super) struct PassBindings<'a> {
    pub(super) input: &'a wgpu::BindGroup,
    pub(super) capabilities: &'a wgpu::BindGroup,
}

pub(super) struct EffectPassContext<'a> {
    pub(super) stride: u32,
    pub(super) capabilities: &'a [&'a wgpu::TextureView],
}

impl RenderSession {
    pub(super) fn encode_item_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        bindings: PassBindings<'_>,
        shader: &ItemShaderId,
        instances: Range<u32>,
        load: wgpu::LoadOp<wgpu::Color>,
    ) {
        let mut pass = render_pass(encoder, target_view, load, "zerium-item-pass");
        let pipeline = self
            .shared
            .item_pipelines
            .get(shader)
            .expect("scene shaders were validated before encoding");
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, bindings.input, &[]);
        pass.set_bind_group(1, bindings.capabilities, &[]);
        pass.draw(0..pipeline.vertex_count, instances);
    }

    pub(super) fn encode_texture_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        bind_group: &wgpu::BindGroup,
        shader: &ItemShaderId,
        load: wgpu::LoadOp<wgpu::Color>,
    ) {
        let mut pass = render_pass(encoder, target_view, load, "zerium-video-pass");
        let pipeline = self
            .shared
            .texture_pipelines
            .get(shader)
            .expect("scene texture item shaders were validated before encoding");
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..pipeline.vertex_count, 0..1);
    }

    pub(super) fn effect_input_bind_group(
        &self,
        resources: &NodeResources,
        input: &wgpu::TextureView,
        source: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        self.shared
            .gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("zerium-dynamic-effect-input"),
                layout: &self.shared.effect_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(input),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.shared.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &resources.effect_instance_buffer,
                            offset: 0,
                            size: NonZeroU64::new(size_of::<GpuEffect>() as u64),
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: resources.effect_property_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(source),
                    },
                ],
            })
    }

    pub(super) fn composite_input_bind_group(
        &self,
        input: &wgpu::TextureView,
        info: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        self.shared
            .gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("zerium-dynamic-composite-input"),
                layout: &self.shared.composite_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(input),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.shared.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: info.as_entire_binding(),
                    },
                ],
            })
    }

    pub(super) fn encode_effect_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        bindings: PassBindings<'_>,
        shader: &EffectShaderId,
        instance_offset: u32,
    ) {
        let mut pass = render_pass(
            encoder,
            target_view,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            "zerium-effect-pass",
        );
        let pipeline = self
            .shared
            .effect_pipelines
            .get(shader)
            .expect("scene effect shaders were validated before encoding");
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, bindings.input, &[instance_offset]);
        pass.set_bind_group(1, bindings.capabilities, &[]);
        pass.draw(0..pipeline.vertex_count, 0..1);
    }

    pub(super) fn encode_composite_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        source: &wgpu::TextureView,
        info: GpuComposite,
        backdrop: Option<&wgpu::TextureView>,
    ) {
        let buffer = self
            .shared
            .gpu
            .device
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("zerium-spatial-composite-info"),
                size: size_of::<GpuComposite>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        self.shared
            .gpu
            .queue
            .write_buffer(&buffer, 0, bytemuck::bytes_of(&info));
        let input = self.composite_input_bind_group(source, &buffer);
        let backdrop = backdrop.map(|view| {
            self.shared
                .gpu
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("zerium-blend-backdrop"),
                    layout: &self.shared.backdrop_bind_group_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    }],
                })
        });
        let mut pass = render_pass(
            encoder,
            target_view,
            wgpu::LoadOp::Load,
            "zerium-effect-composite-pass",
        );
        if let Some(backdrop) = &backdrop {
            pass.set_pipeline(&self.shared.blend_pipeline);
            pass.set_bind_group(1, backdrop, &[]);
        } else {
            pass.set_pipeline(&self.shared.composite_pipeline);
        }
        pass.set_bind_group(0, &input, &[]);
        pass.draw(0..3, 0..1);
    }

    pub(super) fn encode_output_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        input: &wgpu::BindGroup,
    ) {
        let mut pass = render_pass(
            encoder,
            target_view,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            "zerium-output-transform-pass",
        );
        pass.set_pipeline(&self.shared.output_pipeline);
        pass.set_bind_group(0, input, &[]);
        pass.draw(0..3, 0..1);
    }

    pub(super) fn encode_temporal_reduce_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        bindings: PassBindings<'_>,
        command: &TemporalReduceCommand,
        instance_offset: u32,
    ) {
        let mut pass = render_pass(
            encoder,
            target_view,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            "zerium-temporal-reduce-pass",
        );
        let pipeline = self
            .shared
            .temporal_pipelines
            .get(&command.reducer)
            .expect("temporal shaders were validated before encoding");
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, bindings.input, &[instance_offset]);
        pass.set_bind_group(1, bindings.capabilities, &[]);
        pass.draw(0..pipeline.vertex_count, 0..1);
    }

    pub(super) fn encode_compute_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        resources: &NodeResources,
        input: &wgpu::BindGroup,
        capability_group: &wgpu::BindGroup,
        pass_command: &EffectPassCommand,
    ) -> Result<(), RenderError> {
        let EffectPassCommandKind::Compute { dispatch } = pass_command.kind else {
            return Err(RenderError::backend(
                "render pass was sent to the compute encoder",
            ));
        };
        let pipeline = self
            .shared
            .compute_pipelines
            .get(&pass_command.shader)
            .ok_or_else(|| {
                RenderError::backend(format!(
                    "compute effect shader '{}' is not registered",
                    pass_command.shader
                ))
            })?;
        let instance_offset = u64::from(pass_command.instance)
            .checked_mul(resources.compute_info_stride)
            .and_then(|offset| u32::try_from(offset).ok())
            .ok_or_else(|| RenderError::backend("compute info offset exceeds u32"))?;
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("zerium-compute-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, input, &[instance_offset]);
        pass.set_bind_group(1, capability_group, &[]);
        let extent = |dimension: ComputeDispatchDimension| match dimension {
            ComputeDispatchDimension::Width => resources.size.width,
            ComputeDispatchDimension::Height => resources.size.height,
            ComputeDispatchDimension::MaxDimension => {
                resources.size.width.max(resources.size.height)
            }
            ComputeDispatchDimension::One => 1,
        };
        pass.dispatch_workgroups(
            extent(dispatch[0]).div_ceil(pipeline.workgroup_size[0]),
            extent(dispatch[1]).div_ceil(pipeline.workgroup_size[1]),
            extent(dispatch[2]).div_ceil(pipeline.workgroup_size[2]),
        );
        Ok(())
    }

    pub(super) fn encode_effect_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        resources: &NodeResources,
        passes: &[EffectPassCommand],
        mut output: RenderOutput,
        context: EffectPassContext<'_>,
    ) -> Result<RenderOutput, RenderError> {
        let capability_group =
            self.capability_bind_group(context.capabilities, &resources.source.view);
        for effect_pass in passes {
            if effect_pass.captures_source {
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: output.texture(resources),
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &resources.source.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width: resources.size.width,
                        height: resources.size.height,
                        depth_or_array_layers: 1,
                    },
                );
            }
            let instance_offset = effect_pass
                .instance
                .checked_mul(context.stride)
                .ok_or_else(|| RenderError::backend("effect instance offset exceeds u32"))?;
            let target = output.next_effect_target();
            let target_view = target.view(resources);
            let effect_input = match output {
                RenderOutput::EffectA => &resources.effect_inputs[0],
                RenderOutput::EffectB => &resources.effect_inputs[1],
            };
            match effect_pass.kind {
                EffectPassCommandKind::Render => self.encode_effect_pass(
                    encoder,
                    target_view,
                    PassBindings {
                        input: effect_input,
                        capabilities: &capability_group,
                    },
                    &effect_pass.shader,
                    instance_offset,
                ),
                EffectPassCommandKind::Compute { .. } => {
                    let compute_input = match output {
                        RenderOutput::EffectA => &resources.compute_inputs[0],
                        RenderOutput::EffectB => &resources.compute_inputs[1],
                    };
                    self.encode_compute_pass(
                        encoder,
                        resources,
                        compute_input,
                        &capability_group,
                        effect_pass,
                    )?
                }
            }
            output = target;
        }
        Ok(output)
    }
}

pub(super) fn render_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
    label: &str,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    })
}
