use std::sync::Arc;

use super::{FrameRenderer, RenderError, RendererBuilder, RendererDevice};
use zerium_shader::CompiledPluginShaders;

/// Shared rendering resources used by preview and export.
pub(crate) struct RenderRuntime {
    preview: Option<Result<Arc<FrameRenderer>, String>>,
    export_device: Option<Arc<RendererDevice>>,
    plugin_shaders: Arc<CompiledPluginShaders>,
}

impl RenderRuntime {
    pub(crate) fn new(plugin_shaders: Arc<CompiledPluginShaders>) -> Self {
        Self {
            preview: None,
            export_device: None,
            plugin_shaders,
        }
    }

    pub(crate) fn set_preview_renderer(
        &mut self,
        renderer: Result<Arc<FrameRenderer>, RenderError>,
    ) {
        self.preview = Some(renderer.map_err(|error| error.to_string()));
    }

    pub(crate) fn renderer(&self) -> Option<Arc<FrameRenderer>> {
        self.preview
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .cloned()
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.preview
            .as_ref()
            .and_then(|result| result.as_ref().err())
            .map(String::as_str)
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
