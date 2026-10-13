//! Common GPU pipeline handles and raster-pipeline creation.
pub(super) struct RasterPipeline {
    pub(super) pipeline: wgpu::RenderPipeline,
    pub(super) vertex_count: u32,
}

pub(super) struct ComputePipeline {
    pub(super) pipeline: wgpu::ComputePipeline,
    pub(super) workgroup_size: [u32; 3],
}

pub(super) struct TexturePipeline {
    pub(super) pipeline: wgpu::RenderPipeline,
    pub(super) vertex_count: u32,
    pub(super) bind_group_layout: wgpu::BindGroupLayout,
    pub(super) input_count: usize,
}

pub(super) fn raster_pipeline<'a>(
    device: &wgpu::Device,
    layout: impl Into<Option<&'a wgpu::PipelineLayout>>,
    module: &wgpu::ShaderModule,
    entries: [&str; 2],
    target: wgpu::ColorTargetState,
    label: &str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: layout.into(),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(entries[0]),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(entries[1]),
            compilation_options: Default::default(),
            targets: &[Some(target)],
        }),
        multiview_mask: None,
        cache: None,
    })
}
