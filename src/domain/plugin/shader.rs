use serde::Deserialize;

use super::{PluginError, identifier::validate_wgsl_identifier};

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
    let valid_segment = |segment: &str| {
        segment
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    };
    let valid =
        source.ends_with(".wgsl") && !source.contains('\\') && source.split('/').all(valid_segment);
    if !valid {
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
