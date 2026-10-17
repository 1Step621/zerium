//! WESL resolution and linking by module path.

use std::{borrow::Cow, sync::Arc};

use wesl::{CompileOptions, Compiler, error::ResolveError, resolver::Resolver, syntax::ModulePath};
use zerium_core::plugin::{PluginError, PluginManifest};

use super::ShaderError;

#[derive(Clone)]
pub(crate) struct ShaderSources(Arc<dyn Resolver + Send + Sync>);

impl ShaderSources {
    pub(crate) fn new(resolver: impl Resolver + Send + Sync + 'static) -> Self {
        Self(Arc::new(resolver))
    }

    pub(crate) fn link(&self, module: &str) -> Result<String, ShaderError> {
        let main_path = module_path(module)?;
        let mut compiler = Compiler::new_with_resolver(CompileOptions::default(), self.clone());
        compiler.options.keep_main = true;
        compiler.options.sourcemap = false;
        compiler
            .compile_module(&main_path)
            .map(|result| result.syntax.to_string())
            .map_err(|error| {
                ShaderError::operation(format!("WESL shader '{module}' failed to compile: {error}"))
            })
    }
}

impl Resolver for ShaderSources {
    fn resolve_source<'a>(&'a self, path: &ModulePath) -> Result<Cow<'a, str>, ResolveError> {
        self.0.resolve_source(path)
    }

    fn display_name(&self, path: &ModulePath) -> Option<String> {
        self.0.display_name(path)
    }
}

pub(crate) fn require_shader_modules(
    manifest: &PluginManifest,
    resolver: &dyn Resolver,
) -> Result<(), PluginError> {
    for module in manifest.shader_modules() {
        let path = module_path(module)
            .map_err(|error| PluginError::invalid_definition(error.to_string()))?;
        require_module(resolver, &path)?;
    }
    Ok(())
}

pub(crate) fn require_module(
    resolver: &dyn Resolver,
    path: &ModulePath,
) -> Result<(), PluginError> {
    resolver.resolve_source(path).map(|_| ()).map_err(|error| {
        PluginError::missing_asset(format!("shader module '{path}' was not found: {error}"))
    })
}

fn module_path(module: &str) -> Result<ModulePath, ShaderError> {
    format!("package::{module}").parse().map_err(|error| {
        ShaderError::operation(format!("invalid WESL module path '{module}': {error}"))
    })
}
