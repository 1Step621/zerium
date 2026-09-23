use std::sync::Arc;

use super::{CompiledPluginShaders, FrameRenderer, RenderError, RendererBuilder, RendererDevice};

/// Shared rendering resources used by preview and export.
pub(crate) struct RenderRuntime {
    renderer: Option<Arc<FrameRenderer>>,
    export_device: Option<Arc<RendererDevice>>,
    plugin_shaders: Arc<CompiledPluginShaders>,
    error: Option<String>,
}

impl RenderRuntime {
    pub(crate) fn new(plugin_shaders: Arc<CompiledPluginShaders>) -> Self {
        Self {
            renderer: None,
            export_device: None,
            plugin_shaders,
            error: None,
        }
    }

    pub(crate) fn set_preview_renderer(
        &mut self,
        renderer: Option<Arc<FrameRenderer>>,
        error: Option<String>,
    ) {
        self.renderer = renderer;
        self.error = error;
    }

    pub(crate) fn renderer(&self) -> Option<Arc<FrameRenderer>> {
        self.renderer.clone()
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Creates the dedicated export device lazily, then gives each export an
    /// independent mutable rendering session.
    pub(crate) fn export_session(&mut self) -> Result<Arc<FrameRenderer>, RenderError> {
        let device = match &self.export_device {
            Some(device) => device.clone(),
            None => {
                let device = RendererDevice::create_headless(&self.plugin_shaders)?;
                self.export_device = Some(device.clone());
                device
            }
        };
        Ok(Arc::new(device.create_session()))
    }

    pub(crate) fn create_preview_renderer(
        &self,
        surface_device: Arc<wgpu::Device>,
        surface_queue: Arc<wgpu::Queue>,
    ) -> Result<Arc<FrameRenderer>, RenderError> {
        let builder = RendererBuilder::new(surface_device, surface_queue)?
            .register_plugins(&self.plugin_shaders)?;
        Ok(Arc::new(builder.build().create_session()))
    }
}
