use serde::Deserialize;

use super::{PluginError, identifier::validate_wgsl_identifier};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ShaderSchema {
    module: String,
    #[serde(default = "default_vertex_entry")]
    vertex_entry: String,
    #[serde(default = "default_fragment_entry")]
    fragment_entry: String,
}

impl ShaderSchema {
    pub fn module(&self) -> &str {
        &self.module
    }

    pub fn vertex_entry(&self) -> &str {
        &self.vertex_entry
    }

    pub fn fragment_entry(&self) -> &str {
        &self.fragment_entry
    }

    pub(super) fn validate(&self, owner_kind: &str, owner_id: &str) -> Result<(), PluginError> {
        validate_shader_module(owner_kind, owner_id, &self.module)?;
        validate_wgsl_identifier("vertex entry point", &self.vertex_entry)?;
        validate_wgsl_identifier("fragment entry point", &self.fragment_entry)?;
        Ok(())
    }
}

pub(super) fn validate_shader_module(
    owner_kind: &str,
    owner_id: &str,
    module: &str,
) -> Result<(), PluginError> {
    if module.split("::").next() == Some("generated") {
        return Err(PluginError::invalid_definition(format!(
            "{owner_kind} '{owner_id}' uses reserved shader module '{module}'"
        )));
    }

    for segment in module.split("::") {
        validate_wgsl_identifier("shader module path segment", segment)?;
    }
    Ok(())
}

fn default_vertex_entry() -> String {
    "vertex_main".into()
}

fn default_fragment_entry() -> String {
    "fragment_main".into()
}
