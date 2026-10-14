use std::collections::{BTreeMap, BTreeSet};
use wesl::syntax::ModulePath;

use zerium_core::plugin::{
    PluginError, PluginManifest, PropertyLayout, ShaderKind, validate_wgsl_identifier,
};
pub(crate) struct ShaderContract<'a> {
    pub kinds: BTreeSet<ShaderKind>,
    pub layout: &'a PropertyLayout,
    pub input_ids: Vec<String>,
}

impl<'a> ShaderContract<'a> {
    fn new(
        kinds: impl IntoIterator<Item = ShaderKind>,
        layout: &'a PropertyLayout,
        input_ids: Vec<String>,
    ) -> Self {
        Self {
            kinds: kinds.into_iter().collect(),
            layout,
            input_ids,
        }
    }
}

pub(crate) fn shader_contracts(
    manifest: &PluginManifest,
) -> Result<BTreeMap<String, ShaderContract<'_>>, PluginError> {
    let mut contracts = BTreeMap::new();
    for schema in manifest.items() {
        if schema.render().is_some() {
            insert_contract(
                &mut contracts,
                schema.id(),
                ShaderContract::new(
                    [ShaderKind::Item],
                    schema.property_layout(),
                    schema
                        .inputs()
                        .iter()
                        .map(|input| input.id().to_owned())
                        .collect(),
                ),
            )?;
        }
    }
    for schema in manifest.effects() {
        insert_contract(
            &mut contracts,
            schema.id(),
            ShaderContract::new(
                schema.render().passes.iter().map(|pass| pass.shader_kind()),
                schema.property_layout(),
                schema
                    .inputs()
                    .iter()
                    .map(|input| input.id().to_owned())
                    .collect(),
            ),
        )?;
    }
    Ok(contracts)
}

pub(crate) fn property_module(entity: &str, kind: ShaderKind) -> ModulePath {
    let mut module = ModulePath::new_root();
    module.push("generated");
    module.push(entity);
    if matches!(kind, ShaderKind::Compute | ShaderKind::Temporal) {
        module.push(kind.module_name());
    }
    module
}

fn insert_contract<'a>(
    contracts: &mut BTreeMap<String, ShaderContract<'a>>,
    module: &str,
    contract: ShaderContract<'a>,
) -> Result<(), PluginError> {
    validate_wgsl_identifier("generated entity module", module)?;
    if contracts.contains_key(module) {
        return Err(PluginError::invalid_definition(format!(
            "item and effect share generated entity ID '{module}'; use distinct IDs"
        )));
    }
    contracts.insert(module.to_owned(), contract);
    Ok(())
}
