use super::wesl;
use super::*;
use crate::domain::plugin::{PassConstantSchema, Plugin};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ItemShaderId(String);

impl ItemShaderId {
    pub(crate) fn plugin_item(plugin_id: &str, item_id: &str) -> Self {
        Self(format!("{plugin_id}::item::{item_id}"))
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
pub(crate) struct EffectShaderId(String);

impl EffectShaderId {
    pub(crate) fn plugin_pass(plugin_id: &str, effect_id: &str, pass_index: usize) -> Self {
        Self(format!(
            "{plugin_id}::effect::{effect_id}::pass::{pass_index}"
        ))
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
pub(crate) struct TextureShaderId(String);

impl TextureShaderId {
    pub(crate) fn plugin_item(plugin_id: &str, item_id: &str) -> Self {
        Self(format!("{plugin_id}::item::{item_id}"))
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

#[derive(Clone, Debug)]
pub(super) struct ItemShaderDescriptor {
    pub(super) id: ItemShaderId,
    pub(super) label: String,
    pub(super) wgsl: Arc<str>,
    pub(super) vertex_entry: String,
    pub(super) fragment_entry: String,
    pub(super) vertex_count: u32,
}

#[derive(Clone, Debug)]
pub(super) struct TextureShaderDescriptor {
    pub(super) id: TextureShaderId,
    pub(super) label: String,
    pub(super) wgsl: Arc<str>,
    pub(super) vertex_entry: String,
    pub(super) fragment_entry: String,
    pub(super) vertex_count: u32,
    pub(super) input_ids: Vec<String>,
}

#[derive(Clone, Debug)]
pub(super) struct EffectShaderDescriptor {
    pub(super) id: EffectShaderId,
    pub(super) label: String,
    pub(super) wgsl: Arc<str>,
    pub(super) vertex_entry: String,
    pub(super) fragment_entry: String,
}

#[derive(Clone, Debug)]
pub(super) struct ComputeShaderDescriptor {
    pub(super) id: EffectShaderId,
    pub(super) wgsl: Arc<str>,
    pub(super) entry: String,
    pub(super) workgroup_size: [u32; 3],
}

#[derive(Clone, Debug)]
pub(super) enum CompiledEffectShader {
    Render(EffectShaderDescriptor),
    Compute(ComputeShaderDescriptor),
    Temporal(EffectShaderDescriptor),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CompiledPluginShaders {
    pub(super) items: Vec<ItemShaderDescriptor>,
    pub(super) textures: Vec<TextureShaderDescriptor>,
    pub(super) effects: Vec<CompiledEffectShader>,
}

pub(crate) fn compile_plugins(
    plugins: &PluginRegistry,
) -> Result<Arc<CompiledPluginShaders>, RenderError> {
    let mut compiled = CompiledPluginShaders::default();
    let mut item_sources = HashMap::<(String, String), Arc<str>>::new();

    for (plugin_id, schema) in plugins.items() {
        let Some(shader) = schema.visual_shader() else {
            continue;
        };
        let source = compile_item_source(plugins, &mut item_sources, plugin_id, shader.source())?;
        if schema.is_procedural() {
            let id = ItemShaderId::plugin_item(plugin_id, schema.id());
            validate_render_shader(&id, &source, shader.vertex_entry(), shader.fragment_entry())?;
            compiled.items.push(ItemShaderDescriptor {
                id,
                label: shader.source().to_owned(),
                wgsl: source,
                vertex_entry: shader.vertex_entry().to_owned(),
                fragment_entry: shader.fragment_entry().to_owned(),
                vertex_count: schema.vertex_count().expect("visual shader was checked"),
            });
        } else if schema.uses_texture_pipeline() {
            let id = TextureShaderId::plugin_item(plugin_id, schema.id());
            validate_render_shader(&id, &source, shader.vertex_entry(), shader.fragment_entry())?;
            compiled.textures.push(TextureShaderDescriptor {
                id,
                label: shader.source().to_owned(),
                wgsl: source,
                vertex_entry: shader.vertex_entry().to_owned(),
                fragment_entry: shader.fragment_entry().to_owned(),
                vertex_count: schema.vertex_count().expect("visual shader was checked"),
                input_ids: schema.texture_input_ids(),
            });
        }
    }

    for (plugin_id, schema) in plugins.effects() {
        for (pass_index, pass) in schema.passes().iter().enumerate() {
            let id = EffectShaderId::plugin_pass(plugin_id, schema.id(), pass_index);
            let source: Arc<str> =
                compile_plugin_shader(plugins, plugin_id, pass.shader_source(), pass.constants())?
                    .into();
            let label = format!("{} pass {pass_index}", schema.id());
            let effect = match pass {
                EffectPassSchema::Render { shader, .. } => {
                    validate_render_shader(
                        &id,
                        &source,
                        shader.vertex_entry(),
                        shader.fragment_entry(),
                    )?;
                    CompiledEffectShader::Render(EffectShaderDescriptor {
                        id,
                        label,
                        wgsl: source,
                        vertex_entry: shader.vertex_entry().to_owned(),
                        fragment_entry: shader.fragment_entry().to_owned(),
                    })
                }
                EffectPassSchema::Compute { shader, .. } => {
                    let workgroup_size = validate_compute_shader(&id, &source, shader.entry())?;
                    CompiledEffectShader::Compute(ComputeShaderDescriptor {
                        id,
                        wgsl: source,
                        entry: shader.entry().to_owned(),
                        workgroup_size,
                    })
                }
                EffectPassSchema::Temporal { reducer, .. } => {
                    validate_render_shader(
                        &id,
                        &source,
                        reducer.vertex_entry(),
                        reducer.fragment_entry(),
                    )?;
                    CompiledEffectShader::Temporal(EffectShaderDescriptor {
                        id,
                        label,
                        wgsl: source,
                        vertex_entry: reducer.vertex_entry().to_owned(),
                        fragment_entry: reducer.fragment_entry().to_owned(),
                    })
                }
            };
            compiled.effects.push(effect);
        }
    }

    Ok(Arc::new(compiled))
}

fn compile_item_source(
    plugins: &PluginRegistry,
    cache: &mut HashMap<(String, String), Arc<str>>,
    plugin_id: &str,
    source_name: &str,
) -> Result<Arc<str>, RenderError> {
    let key = (plugin_id.to_owned(), source_name.to_owned());
    if let Some(source) = cache.get(&key) {
        return Ok(source.clone());
    }
    let source: Arc<str> = compile_plugin_shader(plugins, plugin_id, source_name, &[])?.into();
    cache.insert(key, source.clone());
    Ok(source)
}

fn compile_plugin_shader(
    plugins: &PluginRegistry,
    plugin_id: &str,
    source_name: &str,
    constants: &[PassConstantSchema],
) -> Result<String, RenderError> {
    let plugin = plugins
        .plugin(plugin_id)
        .ok_or_else(|| RenderError::backend(format!("plugin '{plugin_id}' was not loaded")))?;
    compile_shader(plugin, source_name, constants)
}

fn compile_shader(
    plugin: &Plugin,
    source_name: &str,
    constants: &[PassConstantSchema],
) -> Result<String, RenderError> {
    let source = plugin.shader_source(source_name).ok_or_else(|| {
        RenderError::backend(format!(
            "plugin '{}' shader source '{source_name}' was not loaded",
            plugin.manifest().id()
        ))
    })?;
    wesl::compile(plugin.wesl_modules(), source_name, source, constants)
}

fn parse_and_validate_shader(
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
