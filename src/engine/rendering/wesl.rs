use std::{borrow::Cow, collections::BTreeMap};

use wesl::{
    CompileOptions, Compiler,
    resolver::{Router, StandardResolver, VirtualResolver},
    syntax::ModulePath,
};

use super::RenderError;
use crate::domain::plugin::{PassConstantSchema, PassConstantValue};

pub(super) fn compile(
    modules: &BTreeMap<String, String>,
    source_name: &str,
    source: &str,
    constants: &[PassConstantSchema],
) -> Result<String, RenderError> {
    let mut resolver = VirtualResolver::new();
    for (module, module_source) in modules {
        add_module(
            &mut resolver,
            &format!("package::generated::{module}"),
            module_source,
        )?;
    }
    let source_path = shader_module_path(source_name)?;
    add_module(&mut resolver, &source_path, source)?;

    let mut constants_resolver = StandardResolver::new(".");
    add_constants(&mut constants_resolver, constants);
    let mut router = Router::new();
    router.mount_resolver(ModulePath::new_root(), resolver);
    router.mount_fallback_resolver(constants_resolver);

    let main_path = module_path(&source_path)?;
    let mut compiler = Compiler::new_with_resolver(CompileOptions::default(), router);
    compiler.options.keep_main = true;
    compiler.options.sourcemap = false;
    compiler
        .compile_module(&main_path)
        .map(|result| result.syntax.to_string())
        .map_err(|error| RenderError::backend(format!("WESL shader compilation failed: {error}")))
}

fn shader_module_path(source_name: &str) -> Result<String, RenderError> {
    let source_name = source_name.strip_suffix(".wesl").ok_or_else(|| {
        RenderError::backend(format!(
            "shader source '{source_name}' does not have a .wesl extension"
        ))
    })?;
    Ok(format!("package::{}", source_name.replace('/', "::")))
}

fn add_constants(resolver: &mut StandardResolver, constants: &[PassConstantSchema]) {
    for constant in constants {
        match constant.value() {
            PassConstantValue::F32(value) => resolver.add_constant(constant.id(), value),
            PassConstantValue::I32(value) => resolver.add_constant(constant.id(), value),
            PassConstantValue::U32(value) => resolver.add_constant(constant.id(), value),
            PassConstantValue::Bool(value) => resolver.add_constant(constant.id(), value),
        }
    }
}

fn add_module(
    resolver: &mut VirtualResolver<'static>,
    path: &str,
    source: &str,
) -> Result<(), RenderError> {
    resolver.add_module(module_path(path)?, Cow::Owned(source.to_owned()));
    Ok(())
}

fn module_path(path: &str) -> Result<ModulePath, RenderError> {
    path.parse().map_err(|error| {
        RenderError::backend(format!("invalid WESL module path '{path}': {error}"))
    })
}
