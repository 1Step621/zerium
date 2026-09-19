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
    source: String,
    #[serde(default = "default_vertex_entry")]
    vertex_entry: String,
    #[serde(default = "default_fragment_entry")]
    fragment_entry: String,
}

impl ShaderSchema {
    pub(crate) fn source(&self) -> &str {
        &self.source
    }

    pub(crate) fn vertex_entry(&self) -> &str {
        &self.vertex_entry
    }

    pub(crate) fn fragment_entry(&self) -> &str {
        &self.fragment_entry
    }

    pub(super) fn validate(&self, owner_kind: &str, owner_id: &str) -> Result<(), PluginError> {
        validate_shader_source(owner_kind, owner_id, &self.source)?;
        validate_wgsl_identifier("vertex entry point", &self.vertex_entry)?;
        validate_wgsl_identifier("fragment entry point", &self.fragment_entry)?;
        Ok(())
    }
}

pub(super) fn validate_shader_source(
    owner_kind: &str,
    owner_id: &str,
    source: &str,
) -> Result<(), PluginError> {
    let Some(module_name) = source.strip_suffix(".wesl") else {
        return Err(PluginError::invalid_definition(format!(
            "{owner_kind} '{owner_id}' has an invalid shader source path"
        )));
    };
    if source.contains('\\')
        || module_name
            .split('/')
            .any(|segment| validate_wgsl_identifier("shader module", segment).is_err())
    {
        return Err(PluginError::invalid_definition(format!(
            "{owner_kind} '{owner_id}' has an invalid shader source path"
        )));
    }
    Ok(())
}

fn default_vertex_entry() -> String {
    "vertex_main".into()
}

fn default_fragment_entry() -> String {
    "fragment_main".into()
}
