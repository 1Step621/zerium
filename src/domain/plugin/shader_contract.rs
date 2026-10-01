use std::collections::BTreeMap;

use serde::Serialize;

use super::identifier::validate_wgsl_identifier;
use super::{MAX_CAPABILITIES, PluginError, PluginManifest, PropertyLayout, ShaderKind};
use crate::domain::property::{PropertyType, PropertyValueType, ScalarPropertyType};

#[derive(Clone, Serialize)]
pub(crate) struct ShaderProperty {
    pub(crate) id: String,
    #[serde(skip)]
    pub(crate) ty: PropertyType,
    pub(crate) offset: usize,
    shader_type: ShaderPropertyType,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
enum ShaderPropertyType {
    Value(ShaderValueType),
    Array(ShaderValueType),
}

#[derive(Clone, PartialEq, Eq, Serialize)]
enum ShaderValueType {
    Scalar(&'static str),
    Tuple(Vec<&'static str>),
}

impl ShaderValueType {
    fn from_property(ty: &PropertyValueType) -> Self {
        match ty {
            PropertyValueType::Scalar(ty) => Self::Scalar(scalar_type(ty)),
            PropertyValueType::Tuple(tuple) => {
                Self::Tuple(tuple.scalars().iter().map(scalar_type).collect())
            }
        }
    }
}

fn scalar_type(ty: &ScalarPropertyType) -> &'static str {
    match ty {
        ScalarPropertyType::F32 => "f32",
        ScalarPropertyType::I32 => "i32",
        ScalarPropertyType::U32 | ScalarPropertyType::Enum(_) => "u32",
        ScalarPropertyType::Bool => "bool",
        ScalarPropertyType::Color => "color",
        ScalarPropertyType::String => "string",
    }
}

#[derive(Serialize)]
pub(crate) struct ShaderContract {
    // Property loader shape: Item takes an instance index; Effect does not.
    pub(crate) kind: ShaderKind,
    shader_kinds: Vec<ShaderKind>,
    pub(crate) properties: Vec<ShaderProperty>,
    pub(crate) input_ids: Vec<String>,
}

impl ShaderContract {
    fn new(
        kind: ShaderKind,
        shader_kinds: Vec<ShaderKind>,
        layout: &PropertyLayout,
        input_ids: Vec<String>,
    ) -> Self {
        Self {
            kind,
            shader_kinds,
            properties: layout
                .fields()
                .map(|(id, ty, offset)| ShaderProperty {
                    id: id.to_owned(),
                    ty: ty.clone(),
                    offset,
                    shader_type: match ty {
                        PropertyType::Value(ty) => {
                            ShaderPropertyType::Value(ShaderValueType::from_property(ty))
                        }
                        PropertyType::Array { element_type, .. } => {
                            ShaderPropertyType::Array(ShaderValueType::from_property(element_type))
                        }
                    },
                })
                .collect(),
            input_ids,
        }
    }
}

pub(crate) fn shader_contracts(
    manifest: &PluginManifest,
) -> Result<BTreeMap<String, ShaderContract>, PluginError> {
    let mut contracts = BTreeMap::new();
    for schema in manifest.items() {
        if schema.shader().is_some() {
            insert_contract(
                &mut contracts,
                schema.id(),
                ShaderContract::new(
                    ShaderKind::Item,
                    vec![ShaderKind::Item],
                    schema.property_layout(),
                    schema
                        .capabilities()
                        .iter()
                        .map(|input| input.id().to_owned())
                        .collect(),
                ),
            )?;
        }
    }
    for schema in manifest.effects() {
        let mut shader_kinds = schema
            .passes()
            .iter()
            .map(|pass| pass.shader_kind())
            .collect::<Vec<_>>();
        shader_kinds.sort();
        shader_kinds.dedup();
        insert_contract(
            &mut contracts,
            schema.id(),
            ShaderContract::new(
                ShaderKind::Effect,
                shader_kinds,
                schema.property_layout(),
                schema
                    .capabilities()
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

pub(crate) fn shader_contract_fingerprint(
    contracts: &BTreeMap<String, ShaderContract>,
) -> Result<String, PluginError> {
    // Bump when host templates or the generated interface format change.
    const GENERATED_INTERFACE_REVISION: &[u8] = b"zerium-shader-contract-5\0";
    let signature = serde_json::to_vec(&(MAX_CAPABILITIES, contracts)).map_err(|error| {
        PluginError::invalid_definition(format!("cannot encode shader contracts: {error}"))
    })?;
    let hash = GENERATED_INTERFACE_REVISION
        .iter()
        .copied()
        .chain(signature)
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    Ok(format!("{hash:016x}"))
}
