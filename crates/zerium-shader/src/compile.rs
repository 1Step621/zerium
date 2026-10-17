//! Assemble renderer descriptions from linked, validated plugin modules.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use zerium_core::plugin::{EffectPassSchema, ShaderSchema};

use crate::{
    PluginLibrary, ShaderError,
    compiled::{
        CompiledEffectShader, CompiledPluginShaders, ComputeShaderDescriptor,
        EffectShaderDescriptor, EffectShaderId, ItemShaderDescriptor, ItemShaderId,
    },
    validation::ValidatedShader,
};

struct LinkedShader {
    source: Arc<str>,
    validation: ValidatedShader,
}

type LinkedModules<'a> = BTreeMap<(&'a str, &'a str), LinkedShader>;

/// Link and validate each declared module once, then specialize each pipeline.
/// The resulting descriptions contain no device-specific GPU objects.
pub fn compile_plugins(plugins: &PluginLibrary) -> Result<Arc<CompiledPluginShaders>, ShaderError> {
    let registry = plugins.registry();
    let mut modules = BTreeMap::new();
    for manifest in registry.manifests() {
        let sources = plugins
            .sources(manifest.id())
            .expect("registered plugins have resolvers");
        for module in manifest.shader_modules().collect::<BTreeSet<_>>() {
            let source = Arc::<str>::from(sources.link(module)?);
            let validation =
                ValidatedShader::parse(format_args!("{}::{module}", manifest.id()), &source)?;
            modules.insert((manifest.id(), module), LinkedShader { source, validation });
        }
    }

    let mut compiled = CompiledPluginShaders::default();
    for (plugin_id, schema) in registry.items() {
        if let Some(render) = schema.render() {
            let module = linked_module(&modules, plugin_id, render.shader.module());
            let id = ItemShaderId::plugin_item(plugin_id, schema.id());
            module.validation.render(
                &id,
                render.shader.vertex_entry(),
                render.shader.fragment_entry(),
                &BTreeMap::new(),
            )?;
            compiled.items.push(ItemShaderDescriptor {
                id,
                label: render.shader.module().to_owned(),
                wgsl: Arc::clone(&module.source),
                vertex_entry: render.shader.vertex_entry().to_owned(),
                fragment_entry: render.shader.fragment_entry().to_owned(),
                vertex_count: render.vertex_count,
            });
        }
    }
    for (plugin_id, schema) in registry.effects() {
        for (pass_index, pass) in schema.render().passes.iter().enumerate() {
            let id = EffectShaderId::plugin_pass(plugin_id, schema.id(), pass_index);
            let module = linked_module(&modules, plugin_id, pass.shader_module());
            let label = format!("{} pass {pass_index}", schema.id());
            let effect = match pass {
                EffectPassSchema::Render { shader, .. } => CompiledEffectShader::Render(
                    render_pass(module, id, label, shader, pass.overrides())?,
                ),
                EffectPassSchema::Temporal { reducer, .. } => CompiledEffectShader::Temporal(
                    render_pass(module, id, label, reducer, pass.overrides())?,
                ),
                EffectPassSchema::Compute { shader, .. } => {
                    let workgroup_size =
                        module
                            .validation
                            .compute(&id, shader.entry(), pass.overrides())?;
                    CompiledEffectShader::Compute(ComputeShaderDescriptor {
                        id,
                        wgsl: Arc::clone(&module.source),
                        entry: shader.entry().to_owned(),
                        workgroup_size,
                        overrides: pass.overrides().clone(),
                    })
                }
            };
            compiled.effects.push(effect);
        }
    }
    Ok(Arc::new(compiled))
}

fn linked_module<'a>(
    modules: &'a LinkedModules<'_>,
    plugin: &'a str,
    module: &'a str,
) -> &'a LinkedShader {
    modules
        .get(&(plugin, module))
        .expect("declared shader modules were linked and validated")
}

fn render_pass(
    module: &LinkedShader,
    id: EffectShaderId,
    label: String,
    shader: &ShaderSchema,
    overrides: &BTreeMap<String, f64>,
) -> Result<EffectShaderDescriptor, ShaderError> {
    module.validation.render(
        &id,
        shader.vertex_entry(),
        shader.fragment_entry(),
        overrides,
    )?;
    Ok(EffectShaderDescriptor {
        id,
        label,
        wgsl: Arc::clone(&module.source),
        vertex_entry: shader.vertex_entry().to_owned(),
        fragment_entry: shader.fragment_entry().to_owned(),
        overrides: overrides.clone(),
    })
}
