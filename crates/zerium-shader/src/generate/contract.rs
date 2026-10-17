use std::collections::BTreeMap;
use wesl::syntax::ModulePath;

use zerium_core::plugin::{PluginError, PluginManifest, PropertyLayout, validate_wgsl_identifier};

#[derive(Clone, Copy)]
pub(crate) enum PropertyScope {
    Item,
    Pass,
}

pub(crate) struct ShaderContract<'a> {
    pub scope: PropertyScope,
    pub layout: &'a PropertyLayout,
    pub input_ids: Vec<String>,
}

impl<'a> ShaderContract<'a> {
    fn new(scope: PropertyScope, layout: &'a PropertyLayout, input_ids: Vec<String>) -> Self {
        Self {
            scope,
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
                    PropertyScope::Item,
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
                PropertyScope::Pass,
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

pub(crate) fn property_module(entity: &str) -> ModulePath {
    let mut module = ModulePath::new_root();
    module.push("generated");
    module.push(entity);
    module
}

pub(crate) fn image_module(entity: &str, input: &str) -> ModulePath {
    let mut module = property_module(entity);
    module.push("inputs");
    module.push(input);
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
