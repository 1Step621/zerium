use std::{borrow::Cow, collections::BTreeMap};

use wesl::{
    CompileOptions, Compiler,
    resolver::{Router, StandardResolver, VirtualResolver},
    syntax::ModulePath,
};

use super::ShaderError;
use zerium_core::plugin::{PassConstantSchema, PassConstantValue, ShaderKind};

pub(super) fn compile(
    modules: &BTreeMap<String, String>,
    module: &str,
    entity: &str,
    kind: ShaderKind,
    constants: &[PassConstantSchema],
) -> Result<String, ShaderError> {
    let mut resolver = VirtualResolver::new();
    for (path, source) in modules {
        add_module(&mut resolver, path, source)?;
    }
    // Select packaged interfaces; no WESL declarations are generated at runtime.
    for (alias, target) in [
        ("entity", format!("package::generated::{entity}")),
        (
            "_props",
            format!("package::generated::host::{}", kind.internal_module_name()),
        ),
    ] {
        let source = modules.get(&target).ok_or_else(|| {
            ShaderError::backend(format!(
                "missing WESL module '{target}'; run `zerium plugin generate`"
            ))
        })?;
        add_module(
            &mut resolver,
            &format!("package::generated::host::{alias}"),
            source,
        )?;
    }
    let mut constants_resolver = StandardResolver::new(".");
    add_constants(&mut constants_resolver, constants);
    let mut router = Router::new();
    router.mount_resolver(ModulePath::new_root(), resolver);
    router.mount_fallback_resolver(constants_resolver);

    let main_path = module_path(&format!("package::{module}"))?;
    let mut compiler = Compiler::new_with_resolver(CompileOptions::default(), router);
    compiler.options.keep_main = true;
    compiler.options.sourcemap = false;
    compiler
        .compile_module(&main_path)
        .map(|result| result.syntax.to_string())
        .map_err(|error| ShaderError::backend(format!("WESL shader compilation failed: {error}")))
}

fn add_constants(resolver: &mut StandardResolver, constants: &[PassConstantSchema]) {
    for constant in constants {
        match constant.value {
            PassConstantValue::F32(value) => resolver.add_constant(&constant.id, value),
            PassConstantValue::I32(value) => resolver.add_constant(&constant.id, value),
            PassConstantValue::U32(value) => resolver.add_constant(&constant.id, value),
            PassConstantValue::Bool(value) => resolver.add_constant(&constant.id, value),
        }
    }
}

fn add_module(
    resolver: &mut VirtualResolver<'static>,
    path: &str,
    source: &str,
) -> Result<(), ShaderError> {
    resolver.add_module(module_path(path)?, Cow::Owned(source.to_owned()));
    Ok(())
}

fn module_path(path: &str) -> Result<ModulePath, ShaderError> {
    path.parse().map_err(|error| {
        ShaderError::backend(format!("invalid WESL module path '{path}': {error}"))
    })
}
