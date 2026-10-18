//! WGSL validation and pipeline specialization, independent of WESL and GPU devices.

use std::{borrow::Cow, collections::BTreeMap, fmt::Display, sync::Arc};

use naga::{ShaderStage, back::PipelineConstants};

use crate::ShaderError;

/// Parsing and structural validation are shared by all passes using a module.
/// Entry points and overrides are checked separately for each pipeline.
pub(crate) struct ValidatedShader {
    source: Arc<str>,
    module: naga::Module,
    info: naga::valid::ModuleInfo,
}

impl ValidatedShader {
    pub(crate) fn parse(
        id: impl Display,
        source: impl Into<Arc<str>>,
    ) -> Result<Self, ShaderError> {
        let source = source.into();
        let module = naga::front::wgsl::parse_str(&source).map_err(|error| {
            ShaderError::operation(format!("shader '{id}' failed to parse: {error}"))
        })?;
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .map_err(|error| {
            ShaderError::operation(format!("shader '{id}' failed validation: {error}"))
        })?;
        Ok(Self {
            source,
            module,
            info,
        })
    }

    pub(crate) fn source(&self) -> &Arc<str> {
        &self.source
    }

    pub(crate) fn render(
        &self,
        id: impl Display,
        vertex_entry: &str,
        fragment_entry: &str,
        overrides: &BTreeMap<String, f64>,
    ) -> Result<(), ShaderError> {
        let id = id.to_string();
        let constants = pipeline_constants(overrides);
        for (stage, entry) in [
            (ShaderStage::Vertex, vertex_entry),
            (ShaderStage::Fragment, fragment_entry),
        ] {
            self.specialize_entry(&id, stage, entry, &constants)?;
        }
        Ok(())
    }

    pub(crate) fn compute(
        &self,
        id: impl Display,
        entry: &str,
        overrides: &BTreeMap<String, f64>,
    ) -> Result<[u32; 3], ShaderError> {
        let id = id.to_string();
        let constants = pipeline_constants(overrides);
        let module = self.specialize_entry(&id, ShaderStage::Compute, entry, &constants)?;
        let entry_point = module
            .entry_points
            .iter()
            .find(|candidate| candidate.stage == ShaderStage::Compute && candidate.name == entry)
            .expect("specialization preserves the selected entry point");
        let size = entry_point.workgroup_size;
        let threads = size
            .into_iter()
            .try_fold(1_u32, u32::checked_mul)
            .unwrap_or(u32::MAX);
        if size.contains(&0) || threads > 1_024 {
            return Err(ShaderError::operation(format!(
                "compute shader '{id}' declares invalid workgroup {size:?}"
            )));
        }
        Ok(size)
    }

    fn specialize_entry(
        &self,
        id: &str,
        stage: ShaderStage,
        entry: &str,
        constants: &PipelineConstants,
    ) -> Result<Cow<'_, naga::Module>, ShaderError> {
        if !self
            .module
            .entry_points
            .iter()
            .any(|candidate| candidate.stage == stage && candidate.name == entry)
        {
            return Err(ShaderError::operation(format!(
                "shader '{id}' does not define {stage:?} entry point '{entry}'"
            )));
        }
        naga::back::pipeline_constants::process_overrides(
            &self.module,
            &self.info,
            Some((stage, entry)),
            constants,
        )
        .map(|(module, _)| module)
        .map_err(|error| {
            ShaderError::operation(format!(
                "shader '{id}' has invalid pipeline overrides for '{entry}': {error}"
            ))
        })
    }
}

fn pipeline_constants(overrides: &BTreeMap<String, f64>) -> PipelineConstants {
    overrides
        .iter()
        .map(|(key, value)| (key.clone(), *value))
        .collect()
}

/// Validate a render pipeline that uses the shader's default override values.
pub fn validate_render_shader(
    id: impl Display,
    source: &str,
    vertex_entry: &str,
    fragment_entry: &str,
) -> Result<(), ShaderError> {
    let id = id.to_string();
    ValidatedShader::parse(&id, source)?.render(&id, vertex_entry, fragment_entry, &BTreeMap::new())
}

/// Validate a compute pipeline and return its specialized workgroup dimensions.
pub fn validate_compute_shader(
    id: impl Display,
    source: &str,
    entry: &str,
) -> Result<[u32; 3], ShaderError> {
    let id = id.to_string();
    ValidatedShader::parse(&id, source)?.compute(&id, entry, &BTreeMap::new())
}
