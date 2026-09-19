use super::wesl;
use super::*;
use crate::domain::plugin::PassConstantSchema;

/// Stable identifier used to select the WGSL implementation for an item.
///
/// Manifest-backed IDs are derived from the plugin and item IDs. Callers that
/// register a descriptor directly own the namespace of the value they pass.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ItemShaderId(Cow<'static, str>);

impl ItemShaderId {
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self(Cow::Owned(value.into()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ItemShaderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct EffectShaderId(Cow<'static, str>);

impl EffectShaderId {
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self(Cow::Owned(value.into()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EffectShaderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TextureShaderId(Cow<'static, str>);

impl TextureShaderId {
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self(Cow::Owned(value.into()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TextureShaderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A kind-specific vertex and fragment shader registered with the renderer.
///
/// The source has already been linked from WESL into standalone WGSL and must
/// define the configured vertex and fragment entry points.
#[derive(Clone, Debug)]
pub(super) struct ItemShaderDescriptor {
    pub id: ItemShaderId,
    pub label: Cow<'static, str>,
    pub wgsl: Cow<'static, str>,
    pub(super) vertex_entry: Cow<'static, str>,
    pub(super) fragment_entry: Cow<'static, str>,
    pub(super) vertex_count: u32,
}

#[derive(Clone, Debug)]
pub(super) struct EffectShaderDescriptor {
    pub id: EffectShaderId,
    pub label: Cow<'static, str>,
    pub wgsl: Cow<'static, str>,
    pub(super) vertex_entry: Cow<'static, str>,
    pub(super) fragment_entry: Cow<'static, str>,
    pub(super) vertex_count: u32,
}

impl EffectShaderDescriptor {
    pub(super) fn from_schema(
        pass: &EffectPassSchema,
        id: EffectShaderId,
        label: impl Into<Cow<'static, str>>,
        wgsl: impl Into<Cow<'static, str>>,
    ) -> Result<Self, RenderError> {
        let EffectPassSchema::Render { shader, .. } = pass else {
            return Err(RenderError::backend("effect pass is not a render pass"));
        };
        Ok(Self {
            id,
            label: label.into(),
            wgsl: wgsl.into(),
            vertex_entry: Cow::Owned(shader.vertex_entry().to_owned()),
            fragment_entry: Cow::Owned(shader.fragment_entry().to_owned()),
            vertex_count: 3,
        })
    }
}

impl ItemShaderDescriptor {
    pub(super) fn from_schema(
        plugin_id: &str,
        schema: &ItemSchema,
        label: impl Into<Cow<'static, str>>,
        wgsl: impl Into<Cow<'static, str>>,
    ) -> Result<Self, RenderError> {
        let shader = schema
            .visual_shader()
            .ok_or_else(|| RenderError::backend("item has no visual shader"))?;
        Ok(Self {
            id: ItemShaderId::new(format!("{plugin_id}::item::{}", schema.id())),
            label: label.into(),
            wgsl: wgsl.into(),
            vertex_entry: Cow::Owned(shader.vertex_entry().to_owned()),
            fragment_entry: Cow::Owned(shader.fragment_entry().to_owned()),
            vertex_count: schema.vertex_count().expect("visual shader was checked"),
        })
    }
}

pub(super) fn compile_plugin_shader(
    plugins: &PluginRegistry,
    plugin_id: &str,
    source_name: &str,
    constants: &[PassConstantSchema],
) -> Result<String, RenderError> {
    let plugin = plugins
        .plugin(plugin_id)
        .ok_or_else(|| RenderError::backend(format!("plugin '{plugin_id}' was not loaded")))?;
    let source = plugin.shader_source(source_name).ok_or_else(|| {
        RenderError::backend(format!(
            "plugin '{plugin_id}' shader source '{source_name}' was not loaded"
        ))
    })?;
    wesl::compile(plugin.wesl_modules(), source, constants)
}

pub(super) fn parse_and_validate_shader(
    id: impl fmt::Display,
    source: &str,
) -> Result<naga::Module, RenderError> {
    let id = id.to_string();
    let module = naga::front::wgsl::parse_str(source)
        .map_err(|error| RenderError::backend(format!("shader '{id}' failed to parse: {error}")))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|error| RenderError::backend(format!("shader '{id}' failed validation: {error}")))?;
    Ok(module)
}

pub(super) fn validate_render_shader(
    id: impl fmt::Display,
    source: &str,
    vertex_entry: &str,
    fragment_entry: &str,
) -> Result<(), RenderError> {
    let id = id.to_string();
    let module = parse_and_validate_shader(&id, source)?;
    for (entry, stage) in [
        (vertex_entry, naga::ShaderStage::Vertex),
        (fragment_entry, naga::ShaderStage::Fragment),
    ] {
        if !module
            .entry_points
            .iter()
            .any(|candidate| candidate.stage == stage && candidate.name == entry)
        {
            return Err(RenderError::backend(format!(
                "shader '{id}' does not define {stage:?} entry point '{entry}'"
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_compute_shader(
    id: impl fmt::Display,
    source: &str,
    entry: &str,
) -> Result<[u32; 3], RenderError> {
    let id = id.to_string();
    let module = parse_and_validate_shader(&id, source)?;
    let Some(entry_point) = module.entry_points.iter().find(|entry_point| {
        entry_point.stage == naga::ShaderStage::Compute && entry_point.name == entry
    }) else {
        return Err(RenderError::backend(format!(
            "compute shader '{id}' does not define entry point '{entry}'"
        )));
    };
    let workgroup_size = entry_point.workgroup_size;
    let workgroup_threads = workgroup_size
        .into_iter()
        .try_fold(1_u32, u32::checked_mul)
        .unwrap_or(u32::MAX);
    if workgroup_size.contains(&0) || workgroup_threads > 1_024 {
        return Err(RenderError::backend(format!(
            "compute shader '{id}' declares invalid workgroup {workgroup_size:?}"
        )));
    }
    Ok(workgroup_size)
}

pub(super) fn texture_input_ids(schema: &ItemSchema) -> Vec<String> {
    if schema.is_text() {
        return vec!["text".to_owned()];
    }
    schema
        .texture_inputs()
        .map(|input| input.id().to_owned())
        .collect()
}
