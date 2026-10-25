//! Translate WESL modules and their imports into WGSL.

pub(crate) mod source;

use wesl::{CompileOptions, Compiler, resolver::Resolver};

use self::source::module_path;
use crate::ShaderError;

pub(crate) fn compile_module(resolver: &dyn Resolver, module: &str) -> Result<String, ShaderError> {
    let main_path = module_path(module)?;
    let mut compiler = Compiler::new_with_resolver(CompileOptions::default(), resolver);
    compiler.options.keep_main = true;
    compiler.options.sourcemap = false;
    compiler
        .compile_module(&main_path)
        .map(|result| result.syntax.to_string())
        .map_err(|error| {
            ShaderError::operation(format!(
                "WESL shader '{module}' failed to translate to WGSL: {error}"
            ))
        })
}
