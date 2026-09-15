use std::mem::size_of;

use super::PluginError;
use super::identifier::validate_wgsl_identifier;
use crate::domain::property::{
    MAX_STRING_BYTES, PropertyType, PropertyValue, PropertyValueType, ScalarPropertyType,
};

/// Property blocks are copied for every rendered instance/pass. Large data belongs in a separate
/// resource, not in this per-frame ABI.
pub(super) const MAX_PROPERTY_BLOCK_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(super) struct PropertyInterfaceNames {
    pub(super) struct_name: &'static str,
    pub(super) load_function: &'static str,
    pub(super) raw_load_function: &'static str,
    pub(super) accessor_prefix: &'static str,
    pub(super) takes_instance_index: bool,
}

pub(super) struct PropertyAbiField<'a> {
    pub(super) id: &'a str,
    pub(super) ty: &'a PropertyType,
    /// Pass-local immutable values are encoded into the ABI header once at schema load.
    pub(super) static_value: Option<&'a PropertyValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct CompiledPropertyAbi {
    fields: Box<[CompiledPropertyField]>,
    header_template: Box<[u8]>,
    worst_case_size: usize,
    interface: String,
}

#[derive(Clone, Debug, PartialEq)]
struct CompiledPropertyField {
    id: Box<str>,
    ty: PropertyType,
    offset: usize,
    runtime: bool,
}

impl CompiledPropertyAbi {
    pub(super) fn compile<'a>(
        owner_kind: &str,
        owner_id: &str,
        declarations: impl IntoIterator<Item = PropertyAbiField<'a>>,
        names: PropertyInterfaceNames,
    ) -> Result<Self, PluginError> {
        let declarations = declarations.into_iter().collect::<Vec<_>>();
        validate_property_names(
            owner_kind,
            owner_id,
            declarations
                .iter()
                .map(|declaration| (declaration.id, declaration.ty)),
        )?;

        let mut fields = Vec::with_capacity(declarations.len());
        let mut header_size = 0_usize;
        for declaration in &declarations {
            let size = header_abi_size(declaration.ty);
            let next = header_size.checked_add(size).ok_or_else(|| {
                PluginError::invalid_definition(format!(
                    "{owner_kind} '{owner_id}' property layout overflow"
                ))
            })?;
            if declaration.static_value.is_some_and(|value| {
                !declaration.ty.allows(value) || type_has_dynamic_data(declaration.ty)
            }) {
                return Err(PluginError::invalid_definition(format!(
                    "{owner_kind} '{owner_id}' pass constant '{}' must be a fixed-size value matching its type",
                    declaration.id
                )));
            }
            fields.push(CompiledPropertyField {
                id: declaration.id.into(),
                ty: declaration.ty.clone(),
                offset: header_size,
                runtime: declaration.static_value.is_none(),
            });
            header_size = next;
        }

        let mut worst_case_size = header_size;
        for field in &fields {
            if field.runtime {
                worst_case_size = worst_case_size
                    .checked_add(max_dynamic_size(&field.ty)?)
                    .ok_or_else(|| property_budget_error(owner_kind, owner_id))?;
            }
        }
        if worst_case_size > MAX_PROPERTY_BLOCK_BYTES {
            return Err(property_budget_error(owner_kind, owner_id));
        }

        let mut header_template = vec![0; header_size];
        for (declaration, field) in declarations.iter().zip(&fields) {
            if let Some(value) = declaration.static_value {
                let PropertyType::Value(ty) = &field.ty else {
                    unreachable!("dynamic pass constants were rejected");
                };
                pack_value(&mut header_template, field.offset, ty, value)?;
            }
        }
        let interface = generate_property_interface(&fields, names);
        Ok(Self {
            fields: fields.into_boxed_slice(),
            header_template: header_template.into_boxed_slice(),
            worst_case_size,
            interface,
        })
    }

    pub(super) fn interface(&self) -> &str {
        &self.interface
    }

    pub(super) fn pack<'a>(
        &self,
        owner_kind: &str,
        owner_id: &str,
        mut value_for: impl FnMut(&str, &PropertyType) -> Result<&'a PropertyValue, PluginError>,
    ) -> Result<Vec<u8>, PluginError> {
        let actual_size = self.actual_size(owner_kind, owner_id, &mut value_for)?;
        if actual_size > MAX_PROPERTY_BLOCK_BYTES || actual_size > self.worst_case_size {
            return Err(property_budget_error(owner_kind, owner_id));
        }

        let mut bytes = Vec::with_capacity(actual_size);
        bytes.extend_from_slice(&self.header_template);
        for field in &self.fields {
            if !field.runtime {
                continue;
            }
            let value = value_for(&field.id, &field.ty)?;
            match (&field.ty, value) {
                (PropertyType::Array { element_type, .. }, PropertyValue::Array(values)) => {
                    let data_offset = u32::try_from(bytes.len())
                        .map_err(|_| property_budget_error(owner_kind, owner_id))?;
                    write_u32(&mut bytes, field.offset, data_offset);
                    write_u32(&mut bytes, field.offset + 4, values.len() as u32);
                    let element_size = abi_size(element_type);
                    let data_size = values
                        .len()
                        .checked_mul(element_size)
                        .ok_or_else(|| property_budget_error(owner_kind, owner_id))?;
                    let data_end = bytes
                        .len()
                        .checked_add(data_size)
                        .ok_or_else(|| property_budget_error(owner_kind, owner_id))?;
                    bytes.resize(data_end, 0);
                    for (index, element) in values.iter().enumerate() {
                        pack_value(
                            &mut bytes,
                            data_offset as usize + index * element_size,
                            element_type,
                            element.value(),
                        )?;
                    }
                }
                (ty, value) => {
                    let PropertyType::Value(value_type) = ty else {
                        return Err(PluginError::invalid_definition(format!(
                            "{owner_kind} '{owner_id}' property '{}' has an invalid ABI type",
                            field.id
                        )));
                    };
                    pack_value(&mut bytes, field.offset, value_type, value)?;
                }
            }
        }
        debug_assert_eq!(bytes.len(), actual_size);
        Ok(bytes)
    }

    fn actual_size<'a>(
        &self,
        owner_kind: &str,
        owner_id: &str,
        value_for: &mut impl FnMut(&str, &PropertyType) -> Result<&'a PropertyValue, PluginError>,
    ) -> Result<usize, PluginError> {
        let mut size = self.header_template.len();
        for field in &self.fields {
            if !field.runtime {
                continue;
            }
            let value = value_for(&field.id, &field.ty)?;
            if !field.ty.allows(value) {
                return Err(PluginError::invalid_definition(format!(
                    "{owner_kind} '{owner_id}' property '{}' value does not match its type",
                    field.id
                )));
            }
            size = size
                .checked_add(dynamic_value_size(&field.ty, value)?)
                .ok_or_else(|| property_budget_error(owner_kind, owner_id))?;
            if size > MAX_PROPERTY_BLOCK_BYTES {
                return Err(property_budget_error(owner_kind, owner_id));
            }
        }
        Ok(size)
    }
}

pub(super) fn validate_property_names<'a>(
    owner_kind: &str,
    owner_id: &str,
    properties: impl IntoIterator<Item = (&'a str, &'a PropertyType)>,
) -> Result<(), PluginError> {
    let mut ids = std::collections::HashSet::new();
    let mut fields = std::collections::HashSet::from(["_raw".to_owned()]);
    for (id, ty) in properties {
        validate_wgsl_identifier("property", id)?;
        if !ids.insert(id) {
            return Err(PluginError::invalid_definition(format!(
                "{owner_kind} '{owner_id}' has duplicate property ID '{id}'"
            )));
        }
        let field = if matches!(ty, PropertyType::Array { .. }) {
            format!("{id}_len")
        } else {
            id.to_owned()
        };
        if !fields.insert(field.clone()) {
            return Err(PluginError::invalid_definition(format!(
                "{owner_kind} '{owner_id}' has conflicting generated WGSL field '{field}'"
            )));
        }
    }
    Ok(())
}

fn property_budget_error(owner_kind: &str, owner_id: &str) -> PluginError {
    PluginError::invalid_definition(format!(
        "{owner_kind} '{owner_id}' property ABI exceeds the {} byte budget",
        MAX_PROPERTY_BLOCK_BYTES
    ))
}

fn header_abi_size(ty: &PropertyType) -> usize {
    match ty {
        PropertyType::Array { .. } => 8,
        PropertyType::Value(value_type) => abi_size(value_type),
    }
}

fn value_string_count(ty: &PropertyValueType) -> usize {
    match ty {
        PropertyValueType::Scalar(ty) => usize::from(matches!(ty, ScalarPropertyType::String)),
        PropertyValueType::Tuple(tuple) => tuple
            .scalars()
            .iter()
            .filter(|ty| matches!(ty, ScalarPropertyType::String))
            .count(),
    }
}

fn type_has_dynamic_data(ty: &PropertyType) -> bool {
    let value_type = match ty {
        PropertyType::Value(value_type)
        | PropertyType::Array {
            element_type: value_type,
            ..
        } => value_type,
    };
    matches!(ty, PropertyType::Array { .. }) || value_string_count(value_type) > 0
}

fn max_dynamic_size(ty: &PropertyType) -> Result<usize, PluginError> {
    let value_type = match ty {
        PropertyType::Value(value_type)
        | PropertyType::Array {
            element_type: value_type,
            ..
        } => value_type,
    };
    let payload = value_string_count(value_type) * aligned_size(MAX_STRING_BYTES);
    match ty {
        PropertyType::Value(_) => Ok(payload),
        PropertyType::Array {
            element_type,
            max_items,
            ..
        } => (abi_size(element_type) + payload)
            .checked_mul(*max_items as usize)
            .ok_or_else(|| PluginError::invalid_definition("property ABI maximum size overflows")),
    }
}
fn value_payload_size(value: &PropertyValue) -> usize {
    match value {
        PropertyValue::String(value) => aligned_size(value.len()),
        PropertyValue::Tuple(values) => values.iter().map(value_payload_size).sum(),
        _ => 0,
    }
}
fn dynamic_value_size(ty: &PropertyType, value: &PropertyValue) -> Result<usize, PluginError> {
    match (ty, value) {
        (PropertyType::Array { element_type, .. }, PropertyValue::Array(values)) => {
            values.iter().try_fold(0usize, |total, element| {
                total
                    .checked_add(abi_size(element_type) + value_payload_size(element.value()))
                    .ok_or_else(|| PluginError::invalid_definition("property ABI size overflows"))
            })
        }
        _ => Ok(value_payload_size(value)),
    }
}

const fn aligned_size(size: usize) -> usize {
    size.saturating_add(size_of::<u32>() - 1) / size_of::<u32>() * size_of::<u32>()
}

fn generate_property_interface(
    fields: &[CompiledPropertyField],
    names: PropertyInterfaceNames,
) -> String {
    let PropertyInterfaceNames {
        struct_name,
        load_function,
        raw_load_function,
        accessor_prefix,
        takes_instance_index,
    } = names;
    let tuple_names = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let value_type = match &field.ty {
                PropertyType::Value(value_type)
                | PropertyType::Array {
                    element_type: value_type,
                    ..
                } => value_type,
            };
            matches!(value_type, PropertyValueType::Tuple(_))
                .then(|| format!("{struct_name}Tuple{index}"))
        })
        .collect::<Vec<_>>();
    let mut source = String::new();
    if fields.iter().any(|field| {
        let value_type = match &field.ty {
            PropertyType::Value(value_type)
            | PropertyType::Array {
                element_type: value_type,
                ..
            } => value_type,
        };
        value_string_count(value_type) > 0
    }) {
        source.push_str("struct ZeriumString {\n    _offset: u32,\n    byte_len: u32,\n};\n\n");
        source.push_str(
            "fn zerium_string_byte(raw: ZeriumRawProperties, value: ZeriumString, index: u32) -> u32 {\n",
        );
        source.push_str("    if index >= value.byte_len { return 0u; }\n");
        source.push_str("    let byte_offset = value._offset + index;\n");
        source.push_str("    let word = zerium_raw_u32(raw, byte_offset & 0xfffffffcu);\n");
        source.push_str("    return (word >> ((byte_offset & 3u) * 8u)) & 0xffu;\n}\n\n");
    }
    for (field, tuple_name) in fields.iter().zip(&tuple_names) {
        let value_type = match &field.ty {
            PropertyType::Value(value_type)
            | PropertyType::Array {
                element_type: value_type,
                ..
            } => value_type,
        };
        let (PropertyValueType::Tuple(tuple), Some(tuple_name)) = (value_type, tuple_name) else {
            continue;
        };
        source.push_str(&format!("struct {tuple_name} {{\n"));
        for (index, scalar_type) in tuple.scalars().iter().enumerate() {
            source.push_str(&format!(
                "    v{index}: {},\n",
                scalar_wgsl_type(scalar_type)
            ));
        }
        source.push_str("};\n\n");
    }
    source.push_str(&format!(
        "struct {struct_name} {{\n    _raw: ZeriumRawProperties,\n"
    ));
    for (field, tuple_name) in fields.iter().zip(&tuple_names) {
        if matches!(field.ty, PropertyType::Array { .. }) {
            source.push_str(&format!("    {}_len: u32,\n", field.id));
        } else {
            let value_type = match &field.ty {
                PropertyType::Value(value_type)
                | PropertyType::Array {
                    element_type: value_type,
                    ..
                } => value_type,
            };
            source.push_str(&format!(
                "    {}: {},\n",
                field.id,
                value_wgsl_type(value_type, tuple_name.as_deref())
            ));
        }
    }
    let arguments = if takes_instance_index {
        "instance_index: u32"
    } else {
        ""
    };
    let raw_arguments = if takes_instance_index {
        "instance_index"
    } else {
        ""
    };
    source.push_str(&format!(
        "}};\n\nfn {load_function}({arguments}) -> {struct_name} {{\n"
    ));
    source.push_str(&format!(
        "    let raw = {raw_load_function}({raw_arguments});\n"
    ));
    source.push_str(&format!("    return {struct_name}(raw"));
    for (field, tuple_name) in fields.iter().zip(&tuple_names) {
        let load = if matches!(field.ty, PropertyType::Array { .. }) {
            format!("zerium_raw_u32(raw, {}u)", field.offset + 4)
        } else {
            let value_type = match &field.ty {
                PropertyType::Value(value_type)
                | PropertyType::Array {
                    element_type: value_type,
                    ..
                } => value_type,
            };
            value_wgsl_load_at(
                value_type,
                tuple_name.as_deref(),
                "raw",
                &format!("{}u", field.offset),
            )
        };
        source.push_str(", ");
        source.push_str(&load);
    }
    source.push_str(");\n}\n");

    for (field, tuple_name) in fields.iter().zip(&tuple_names) {
        let PropertyType::Array { element_type, .. } = &field.ty else {
            continue;
        };
        source.push_str(&format!(
            "\nfn {accessor_prefix}_{id}_get(properties: {struct_name}, index: u32) -> {ty} {{\n",
            id = field.id,
            ty = value_wgsl_type(element_type, tuple_name.as_deref()),
        ));
        source.push_str(&format!(
            "    if index >= properties.{id}_len {{ return {zero}; }}\n",
            id = field.id,
            zero = value_wgsl_zero(element_type, tuple_name.as_deref()),
        ));
        source.push_str(&format!(
            "    let byte_offset = zerium_raw_u32(properties._raw, {}u) + index * {}u;\n",
            field.offset,
            abi_size(element_type),
        ));
        source.push_str(&format!(
            "    return {};\n}}\n",
            value_wgsl_load_at(
                element_type,
                tuple_name.as_deref(),
                "properties._raw",
                "byte_offset"
            )
        ));
    }
    source
}

const fn scalar_abi_size(ty: &ScalarPropertyType) -> usize {
    let word_count = match ty {
        ScalarPropertyType::F32
        | ScalarPropertyType::I32
        | ScalarPropertyType::U32
        | ScalarPropertyType::Bool
        | ScalarPropertyType::Enum(_) => 1,
        ScalarPropertyType::Color => 4,
        ScalarPropertyType::String => 2,
    };
    word_count * size_of::<u32>()
}

fn abi_size(ty: &PropertyValueType) -> usize {
    match ty {
        PropertyValueType::Scalar(ty) => scalar_abi_size(ty),
        PropertyValueType::Tuple(tuple) => tuple.scalars().iter().map(scalar_abi_size).sum(),
    }
}

const fn scalar_wgsl_type(ty: &ScalarPropertyType) -> &'static str {
    match ty {
        ScalarPropertyType::F32 => "f32",
        ScalarPropertyType::I32 => "i32",
        ScalarPropertyType::U32 | ScalarPropertyType::Enum(_) => "u32",
        ScalarPropertyType::Bool => "bool",
        ScalarPropertyType::Color => "vec4<f32>",
        ScalarPropertyType::String => "ZeriumString",
    }
}

fn value_wgsl_type(ty: &PropertyValueType, tuple_name: Option<&str>) -> String {
    match ty {
        PropertyValueType::Scalar(ty) => scalar_wgsl_type(ty).to_owned(),
        PropertyValueType::Tuple(_) => tuple_name
            .expect("tuple fields have a generated WGSL type")
            .to_owned(),
    }
}

const fn scalar_wgsl_zero(ty: &ScalarPropertyType) -> &'static str {
    match ty {
        ScalarPropertyType::F32 => "0.0",
        ScalarPropertyType::I32 => "0i",
        ScalarPropertyType::U32 | ScalarPropertyType::Enum(_) => "0u",
        ScalarPropertyType::Bool => "false",
        ScalarPropertyType::Color => "vec4(0.0)",
        ScalarPropertyType::String => "ZeriumString(0u, 0u)",
    }
}

fn value_wgsl_zero(ty: &PropertyValueType, tuple_name: Option<&str>) -> String {
    match ty {
        PropertyValueType::Scalar(ty) => scalar_wgsl_zero(ty).to_owned(),
        PropertyValueType::Tuple(tuple) => format!(
            "{}({})",
            tuple_name.expect("tuple fields have a generated WGSL type"),
            tuple
                .scalars()
                .iter()
                .map(|scalar_type| scalar_wgsl_zero(scalar_type))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn scalar_wgsl_load_at(ty: &ScalarPropertyType, raw: &str, offset: &str) -> String {
    match ty {
        ScalarPropertyType::F32 => format!("zerium_raw_f32({raw}, {offset})"),
        ScalarPropertyType::I32 => format!("zerium_raw_i32({raw}, {offset})"),
        ScalarPropertyType::U32 | ScalarPropertyType::Enum(_) => {
            format!("zerium_raw_u32({raw}, {offset})")
        }
        ScalarPropertyType::Bool => format!("zerium_raw_bool({raw}, {offset})"),
        ScalarPropertyType::Color => format!(
            "vec4(zerium_raw_f32({raw}, {offset}), zerium_raw_f32({raw}, {offset} + 4u), \
             zerium_raw_f32({raw}, {offset} + 8u), zerium_raw_f32({raw}, {offset} + 12u))"
        ),
        ScalarPropertyType::String => format!(
            "ZeriumString(zerium_raw_u32({raw}, {offset}), \
             zerium_raw_u32({raw}, {offset} + 4u))"
        ),
    }
}

fn value_wgsl_load_at(
    ty: &PropertyValueType,
    tuple_name: Option<&str>,
    raw: &str,
    offset: &str,
) -> String {
    match ty {
        PropertyValueType::Scalar(ty) => scalar_wgsl_load_at(ty, raw, offset),
        PropertyValueType::Tuple(tuple) => {
            let mut byte_offset = 0;
            let scalars = tuple
                .scalars()
                .iter()
                .map(|scalar_type| {
                    let load_offset = if byte_offset == 0 {
                        offset.to_owned()
                    } else {
                        format!("{offset} + {byte_offset}u")
                    };
                    byte_offset += scalar_abi_size(scalar_type);
                    scalar_wgsl_load_at(scalar_type, raw, &load_offset)
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{}({scalars})",
                tuple_name.expect("tuple fields have a generated WGSL type")
            )
        }
    }
}

fn pack_scalar(
    bytes: &mut Vec<u8>,
    offset: usize,
    ty: &ScalarPropertyType,
    value: &PropertyValue,
) -> Result<(), PluginError> {
    match (ty, value) {
        (ScalarPropertyType::F32, PropertyValue::F32(value)) => {
            write_u32(bytes, offset, value.to_bits());
        }
        (ScalarPropertyType::I32, PropertyValue::I32(value)) => {
            write_u32(bytes, offset, *value as u32);
        }
        (ScalarPropertyType::U32, PropertyValue::U32(value))
        | (ScalarPropertyType::Enum(_), PropertyValue::Enum(value)) => {
            write_u32(bytes, offset, *value);
        }
        (ScalarPropertyType::Bool, PropertyValue::Bool(value)) => {
            write_u32(bytes, offset, u32::from(*value));
        }
        (ScalarPropertyType::Color, PropertyValue::Color(values)) => {
            for (index, value) in values.iter().enumerate() {
                write_u32(bytes, offset + index * 4, value.to_bits());
            }
        }
        (ScalarPropertyType::String, PropertyValue::String(value)) => {
            let (data_offset, byte_len) = append_bytes(bytes, value.as_bytes())?;
            write_u32(bytes, offset, data_offset);
            write_u32(bytes, offset + 4, byte_len);
        }
        _ => unreachable!("property value type was validated before packing"),
    }
    Ok(())
}

fn pack_value(
    bytes: &mut Vec<u8>,
    offset: usize,
    ty: &PropertyValueType,
    value: &PropertyValue,
) -> Result<(), PluginError> {
    match (ty, value) {
        (PropertyValueType::Scalar(ty), value) => pack_scalar(bytes, offset, ty, value)?,
        (PropertyValueType::Tuple(tuple), PropertyValue::Tuple(values)) => {
            let mut byte_offset = offset;
            for (scalar_type, value) in tuple.scalars().iter().zip(values) {
                let ty = scalar_type;
                pack_scalar(bytes, byte_offset, ty, value)?;
                byte_offset += scalar_abi_size(ty);
            }
        }
        _ => unreachable!("property value type was validated before packing"),
    }
    Ok(())
}

fn append_bytes(bytes: &mut Vec<u8>, value: &[u8]) -> Result<(u32, u32), PluginError> {
    let offset = u32::try_from(bytes.len())
        .map_err(|_| PluginError::invalid_definition("property data exceeds u32"))?;
    let byte_len = u32::try_from(value.len())
        .map_err(|_| PluginError::invalid_definition("string value exceeds u32"))?;
    let aligned_end = bytes
        .len()
        .checked_add(value.len())
        .map(aligned_size)
        .ok_or_else(|| PluginError::invalid_definition("property data size overflows"))?;
    if aligned_end > MAX_PROPERTY_BLOCK_BYTES {
        return Err(PluginError::invalid_definition(
            "property data exceeds the ABI byte budget",
        ));
    }
    bytes.extend_from_slice(value);
    bytes.resize(aligned_end, 0);
    Ok((offset, byte_len))
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
