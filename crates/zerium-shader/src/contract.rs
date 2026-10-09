use std::collections::BTreeMap;

use zerium_core::plugin::{
    PluginError, PluginManifest, PropertyLayout, ShaderKind, validate_wgsl_identifier,
};
use zerium_core::property::PropertySchema;

#[derive(Clone)]
pub struct ShaderProperty {
    pub schema: PropertySchema,
    pub offset: usize,
}

pub struct ShaderContract {
    // Property loader shape: Item takes an instance index; Effect does not.
    pub kind: ShaderKind,
    pub properties: Vec<ShaderProperty>,
    pub input_ids: Vec<String>,
}

impl ShaderContract {
    fn new(kind: ShaderKind, layout: &PropertyLayout, input_ids: Vec<String>) -> Self {
        Self {
            kind,
            properties: layout
                .fields()
                .map(|(schema, offset)| ShaderProperty {
                    schema: schema.clone(),
                    offset,
                })
                .collect(),
            input_ids,
        }
    }
}

pub fn shader_contracts(
    manifest: &PluginManifest,
) -> Result<BTreeMap<String, ShaderContract>, PluginError> {
    let mut contracts = BTreeMap::new();
    for schema in manifest.items() {
        if schema.render().is_some() {
            insert_contract(
                &mut contracts,
                schema.id(),
                ShaderContract::new(
                    ShaderKind::Item,
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
                ShaderKind::Effect,
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

fn insert_contract(
    contracts: &mut BTreeMap<String, ShaderContract>,
    module: &str,
    contract: ShaderContract,
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
