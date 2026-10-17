//! GPU encoding for independent item, image, and effect-pass nodes.

use super::bindings::GpuComposite;
use super::plan::{PassCommand, PassKind};
use super::resources::NodeResources;
use super::{RenderError, RenderSession};
use std::ops::Range;
use zerium_core::plugin::ComputeDispatchDimension;
use zerium_shader::ItemShaderId;

pub(super) struct PassBindings<'a> {
    pub(super) input: &'a wgpu::BindGroup,
    pub(super) capabilities: &'a wgpu::BindGroup,
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

    pub(super) fn encode_frame_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        bind_group: &wgpu::BindGroup,
        load: wgpu::LoadOp<wgpu::Color>,
    ) {
        let mut pass = render_pass(encoder, target_view, load, "zerium-frame-image-pass");
        pass.set_pipeline(&self.shared.frame_pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
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

    pub(super) fn encode_pass_node(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        resources: &NodeResources,
        bindings: PassBindings<'_>,
        command: &PassCommand,
    ) -> Result<(), RenderError> {
        match command.kind {
            PassKind::Compute { dispatch } => {
                let pipeline = self
                    .shared
                    .compute_pipelines
                    .get(&command.shader)
                    .expect("validated compute shader");
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("zerium-compute-pass"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&pipeline.pipeline);
                pass.set_bind_group(0, bindings.input, &[]);
                pass.set_bind_group(1, bindings.capabilities, &[]);
                let extent = |dimension| match dimension {
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
            }

            PassKind::Render | PassKind::Temporal => {
                let pipeline = if command.kind == PassKind::Temporal {
                    self.shared.temporal_pipelines.get(&command.shader)
                } else {
                    self.shared.effect_pipelines.get(&command.shader)
                }
                .expect("validated raster pass");
                let mut pass = render_pass(
                    encoder,
                    &resources.target.view,
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    "zerium-raster-pass",
                );
                pass.set_pipeline(&pipeline.pipeline);
                pass.set_bind_group(0, bindings.input, &[]);
                pass.set_bind_group(1, bindings.capabilities, &[]);
                pass.draw(0..pipeline.vertex_count, 0..1);
            }
        }

        Ok(())
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
