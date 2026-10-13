//! Fullscreen YUV-plane conversion shared by CPU uploads and GPU frames.
use super::{RenderError, pipelines::raster_pipeline};
use zerium_shader::validate_render_shader;

/// Both CPU planes and imported GPU surfaces use the same fullscreen conversion.
/// wgpu derives the binding layout and uniform sizes directly from each shader.
pub(super) struct YuvConversionPipeline {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}

impl YuvConversionPipeline {
    pub(in crate::engine) fn new(
        device: &wgpu::Device,
        label: &str,
        shader: &str,
    ) -> Result<Self, RenderError> {
        validate_render_shader(label, shader, "vertex_main", "fragment_main")?;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(shader.into()),
        });
        let pipeline = raster_pipeline(
            device,
            None,
            &module,
            ["vertex_main", "fragment_main"],
            wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            },
            label,
        );
        let layout = pipeline.get_bind_group_layout(0);
        Ok(Self { layout, pipeline })
    }

    pub(in crate::engine) fn binding<'a>(
        &self,
        device: &wgpu::Device,
        views: impl IntoIterator<Item = &'a wgpu::TextureView>,
        uniform: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        let mut entries: Vec<_> = views
            .into_iter()
            .enumerate()
            .map(|(binding, view)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: wgpu::BindingResource::TextureView(view),
            })
            .collect();
        entries.push(wgpu::BindGroupEntry {
            binding: entries.len() as u32,
            resource: uniform.as_entire_binding(),
        });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("zerium-video-input-binding"),
            layout: &self.layout,
            entries: &entries,
        })
    }

    pub(in crate::engine) fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        binding: &wgpu::BindGroup,
        output: &wgpu::Texture,
    ) {
        let view = output.create_view(&wgpu::TextureViewDescriptor {
            format: Some(wgpu::TextureFormat::Rgba8Unorm),
            ..Default::default()
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("zerium-video-input-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, binding, &[]);
        pass.draw(0..3, 0..1);
    }
}
