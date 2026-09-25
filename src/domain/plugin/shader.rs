use serde::Deserialize;

use super::{PluginError, identifier::validate_wgsl_identifier};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShaderKind {
    Item,
    Effect,
    Compute,
    Temporal,
}

impl ShaderKind {
    pub(crate) const ALL: [Self; 4] = [Self::Item, Self::Effect, Self::Compute, Self::Temporal];

    pub(crate) const fn module_name(self) -> &'static str {
        match self {
            Self::Item => "item",
            Self::Effect => "effect",
            Self::Compute => "compute",
            Self::Temporal => "temporal",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ShaderSchema {
    module: String,
    #[serde(default = "default_vertex_entry")]
    vertex_entry: String,
    #[serde(default = "default_fragment_entry")]
    fragment_entry: String,
}

impl ShaderSchema {
    pub(crate) fn module(&self) -> &str {
        &self.module
    }

    pub(crate) fn vertex_entry(&self) -> &str {
        &self.vertex_entry
    }

    pub(crate) fn fragment_entry(&self) -> &str {
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
    if module == "generated" {
        return Err(PluginError::invalid_definition(format!(
            "{owner_kind} '{owner_id}' uses reserved shader module 'generated'"
        )));
    }
    validate_wgsl_identifier("shader module", module)
}

fn default_vertex_entry() -> String {
    "vertex_main".into()
}

fn default_fragment_entry() -> String {
    "fragment_main".into()
}
