//! Device ownership and render sessions for preview and export.
use crate::engine::{
    GpuContext,
    processing::visual::{RenderDevice, RenderError, RenderSession},
};
use std::sync::Arc;
use zerium_shader::ValidatedPluginShaders;

/// Shared rendering resources used by preview and export.
pub(crate) struct RenderRuntime {
    preview: Option<Result<Arc<RenderSession>, String>>,
    export_device: Option<Arc<RenderDevice>>,
    plugin_shaders: ValidatedPluginShaders,
}

impl RenderRuntime {
    pub(crate) fn new(plugin_shaders: ValidatedPluginShaders) -> Self {
        Self {
            preview: None,
            export_device: None,
            plugin_shaders,
        }
    }

    pub(crate) fn set_preview_session(
        &mut self,
        renderer: Result<Arc<RenderSession>, RenderError>,
    ) {
        self.preview = Some(renderer.map_err(|error| error.to_string()));
    }

    pub(crate) fn preview_session(&self) -> Option<Arc<RenderSession>> {
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
    pub(crate) fn export_session(&mut self) -> Result<RenderSession, RenderError> {
        let device = match &self.export_device {
            Some(device) => device.clone(),
            None => {
                let device = RenderDevice::create_headless(&self.plugin_shaders)?;
                self.export_device = Some(device.clone());
                device
            }
        };
        Ok(device.create_session())
    }

    pub(crate) fn create_preview_session(
        &self,
        gpu: GpuContext,
    ) -> Result<Arc<RenderSession>, RenderError> {
        let device = RenderDevice::create(gpu, &self.plugin_shaders)?;
        Ok(Arc::new(device.create_session()))
    }
}
