use std::sync::Arc;

use super::{
    RenderError,
    shader::{
        CompiledEffectShader, CompiledPluginShaders, ComputeShaderDescriptor,
        EffectShaderDescriptor, EffectShaderId, ItemShaderDescriptor, ItemShaderId,
        TextureShaderDescriptor,
    },
    wesl,
};
use crate::domain::plugin::{
    Capability, EffectPassSchema, PassConstantSchema, PluginRegistry, ShaderSchema,
};

pub(crate) fn compile_plugins(
    plugins: &PluginRegistry,
) -> Result<Arc<CompiledPluginShaders>, RenderError> {
    let mut compiled = CompiledPluginShaders::default();
    for (plugin_id, schema) in plugins.items() {
        if let Some(shader) = schema.shader() {
            let id = ItemShaderId::plugin_item(plugin_id, schema.id());
            compiled.items.push(compile_item_shader(
                plugins,
                plugin_id,
                shader,
                id,
                schema.vertex_count(),
                schema.capabilities(),
            )?);
        }
    }
    for (plugin_id, schema) in plugins.effects() {
        let interface = super::capability_input::interface(schema.capabilities());
        for (pass_index, pass) in schema.passes().iter().enumerate() {
            let id = EffectShaderId::plugin_pass(plugin_id, schema.id(), pass_index);
            let source: Arc<str> = compile_plugin_shader(
                plugins,
                plugin_id,
                pass.shader_module(),
                pass.constants(),
                &interface,
            )?
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
    compiled.textures.push(TextureShaderDescriptor {
        id: ItemShaderId::host_capability_frame(),
        label: "zerium capability frame".to_owned(),
        wgsl: include_str!("capability_frame.wgsl").into(),
        vertex_entry: "vertex_main".to_owned(),
        fragment_entry: "fragment_main".to_owned(),
        vertex_count: 3,
        input_ids: vec!["source".to_owned()],
    });
    Ok(Arc::new(compiled))
}

fn compile_item_shader(
    plugins: &PluginRegistry,
    plugin_id: &str,
    shader: &ShaderSchema,
    id: ItemShaderId,
    vertex_count: u32,
    capabilities: &[Capability],
) -> Result<ItemShaderDescriptor, RenderError> {
    let interface = super::capability_input::interface(capabilities);
    let source: Arc<str> =
        compile_plugin_shader(plugins, plugin_id, shader.module(), &[], &interface)?.into();
    validate_render_shader(&id, &source, shader.vertex_entry(), shader.fragment_entry())?;
    Ok(ItemShaderDescriptor {
        id,
        label: shader.module().to_owned(),
        wgsl: source,
        vertex_entry: shader.vertex_entry().to_owned(),
        fragment_entry: shader.fragment_entry().to_owned(),
        vertex_count,
    })
}

fn compile_plugin_shader(
    plugins: &PluginRegistry,
    plugin_id: &str,
    module: &str,
    constants: &[PassConstantSchema],
    capability_interface: &str,
) -> Result<String, RenderError> {
    let plugin = plugins
        .plugin(plugin_id)
        .ok_or_else(|| RenderError::backend(format!("plugin '{plugin_id}' was not loaded")))?;
    wesl::compile(plugin.modules(), module, constants, capability_interface)
}

fn parse_and_validate_shader(
    id: impl std::fmt::Display,
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
    id: impl std::fmt::Display,
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
    id: impl std::fmt::Display,
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
