use std::mem::size_of;

use super::PluginError;
use super::identifier::validate_wgsl_identifier;
use crate::domain::parameter::{
    MAX_STRING_BYTES, ParameterType, ParameterValue, ParameterValueType, ScalarParameterType,
};

/// Parameter blocks are copied for every rendered instance/pass. Large data belongs in a separate
/// resource, not in this per-frame ABI.
pub(super) const MAX_PARAMETER_BLOCK_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(super) struct ParameterInterfaceNames {
    pub(super) struct_name: &'static str,
    pub(super) load_function: &'static str,
    pub(super) raw_load_function: &'static str,
    pub(super) accessor_prefix: &'static str,
    pub(super) takes_instance_index: bool,
}

pub(super) struct ParameterAbiField<'a> {
    pub(super) id: &'a str,
    pub(super) ty: &'a ParameterType,
    /// Pass-local immutable values are encoded into the ABI header once at schema load.
    pub(super) static_value: Option<&'a ParameterValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct CompiledParameterAbi {
    fields: Box<[CompiledParameterField]>,
    header_template: Box<[u8]>,
    worst_case_size: usize,
    interface: String,
}

#[derive(Clone, Debug, PartialEq)]
struct CompiledParameterField {
    id: Box<str>,
    ty: ParameterType,
    offset: usize,
    runtime: bool,
}

impl CompiledParameterField {
    fn value_type(&self) -> &ParameterValueType {
        match &self.ty {
            ParameterType::Array { element, .. } => element,
            ParameterType::Value(_) => self
                .ty
                .value_type()
                .expect("non-array parameters have a value type"),
        }
    }
}

impl CompiledParameterAbi {
    pub(super) fn compile<'a>(
        owner_kind: &str,
        owner_id: &str,
        declarations: impl IntoIterator<Item = ParameterAbiField<'a>>,
        names: ParameterInterfaceNames,
    ) -> Result<Self, PluginError> {
        let declarations = declarations.into_iter().collect::<Vec<_>>();
        validate_parameter_names(
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
                    "{owner_kind} '{owner_id}' parameter layout overflow"
                ))
            })?;
            if declaration.static_value.is_some_and(|value| {
                !value.matches_type(declaration.ty) || type_has_dynamic_data(declaration.ty)
            }) {
                return Err(PluginError::invalid_definition(format!(
                    "{owner_kind} '{owner_id}' pass constant '{}' must be a fixed-size value matching its type",
                    declaration.id
                )));
            }
            fields.push(CompiledParameterField {
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
                    .ok_or_else(|| parameter_budget_error(owner_kind, owner_id))?;
            }
        }
        if worst_case_size > MAX_PARAMETER_BLOCK_BYTES {
            return Err(parameter_budget_error(owner_kind, owner_id));
        }

        let mut header_template = vec![0; header_size];
        for (declaration, field) in declarations.iter().zip(&fields) {
            if let Some(value) = declaration.static_value {
                let ty = field
                    .ty
                    .value_type()
                    .expect("dynamic pass constants were rejected");
                pack_value(&mut header_template, field.offset, ty, value)?;
            }
        }
        let interface = generate_parameter_interface(&fields, names);
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
        mut value_for: impl FnMut(&str, &ParameterType) -> Result<&'a ParameterValue, PluginError>,
    ) -> Result<Vec<u8>, PluginError> {
        let actual_size = self.actual_size(owner_kind, owner_id, &mut value_for)?;
        if actual_size > MAX_PARAMETER_BLOCK_BYTES || actual_size > self.worst_case_size {
            return Err(parameter_budget_error(owner_kind, owner_id));
        }

        let mut bytes = Vec::with_capacity(actual_size);
        bytes.extend_from_slice(&self.header_template);
        for field in &self.fields {
            if !field.runtime {
                continue;
            }
            let value = value_for(&field.id, &field.ty)?;
            match (&field.ty, value) {
                (ParameterType::Array { element, .. }, ParameterValue::Array(values)) => {
                    let data_offset = u32::try_from(bytes.len())
                        .map_err(|_| parameter_budget_error(owner_kind, owner_id))?;
                    write_u32(&mut bytes, field.offset, data_offset);
                    write_u32(&mut bytes, field.offset + 4, values.len() as u32);
                    let element_size = abi_size(element);
                    let data_size = values
                        .len()
                        .checked_mul(element_size)
                        .ok_or_else(|| parameter_budget_error(owner_kind, owner_id))?;
                    let data_end = bytes
                        .len()
                        .checked_add(data_size)
                        .ok_or_else(|| parameter_budget_error(owner_kind, owner_id))?;
                    bytes.resize(data_end, 0);
                    for (index, value) in values.iter().enumerate() {
                        pack_value(
                            &mut bytes,
                            data_offset as usize + index * element_size,
                            element,
                            value,
                        )?;
                    }
                }
                (ty, value) => {
                    let value_type = ty.value_type().ok_or_else(|| {
                        PluginError::invalid_definition(format!(
                            "{owner_kind} '{owner_id}' parameter '{}' has an invalid ABI type",
                            field.id
                        ))
                    })?;
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
        value_for: &mut impl FnMut(&str, &ParameterType) -> Result<&'a ParameterValue, PluginError>,
    ) -> Result<usize, PluginError> {
        let mut size = self.header_template.len();
        for field in &self.fields {
            if !field.runtime {
                continue;
            }
            let value = value_for(&field.id, &field.ty)?;
            if !value.matches_type(&field.ty) {
                return Err(PluginError::invalid_definition(format!(
                    "{owner_kind} '{owner_id}' parameter '{}' value does not match its type",
                    field.id
                )));
            }
            size = size
                .checked_add(dynamic_value_size(&field.ty, value)?)
                .ok_or_else(|| parameter_budget_error(owner_kind, owner_id))?;
            if size > MAX_PARAMETER_BLOCK_BYTES {
                return Err(parameter_budget_error(owner_kind, owner_id));
            }
        }
        Ok(size)
    }
}

pub(super) fn validate_parameter_names<'a>(
    owner_kind: &str,
    owner_id: &str,
    parameters: impl IntoIterator<Item = (&'a str, &'a ParameterType)>,
) -> Result<(), PluginError> {
    let mut ids = std::collections::HashSet::new();
    let mut fields = std::collections::HashSet::from(["_raw".to_owned()]);
    for (id, ty) in parameters {
        validate_wgsl_identifier("parameter", id)?;
        if !ids.insert(id) {
            return Err(PluginError::invalid_definition(format!(
                "{owner_kind} '{owner_id}' has duplicate parameter ID '{id}'"
            )));
        }
        let field = if matches!(ty, ParameterType::Array { .. }) {
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

fn parameter_budget_error(owner_kind: &str, owner_id: &str) -> PluginError {
    PluginError::invalid_definition(format!(
        "{owner_kind} '{owner_id}' parameter ABI exceeds the {} byte budget",
        MAX_PARAMETER_BLOCK_BYTES
    ))
}

fn header_abi_size(ty: &ParameterType) -> usize {
    match ty {
        ParameterType::Array { .. } => 8,
        ParameterType::Value(_) => abi_size(
            ty.value_type()
                .expect("non-array parameter has a value type"),
        ),
    }
}

fn value_string_count(ty: &ParameterValueType) -> usize {
    match ty {
        ParameterValueType::Scalar(ty) => usize::from(matches!(ty, ScalarParameterType::String)),
        ParameterValueType::Tuple(tuple) => tuple
            .elements()
            .iter()
            .filter(|ty| matches!(ty, ScalarParameterType::String))
            .count(),
    }
}

fn type_has_dynamic_data(ty: &ParameterType) -> bool {
    matches!(ty, ParameterType::Array { .. }) || value_string_count(ty.element_type()) > 0
}

fn max_dynamic_size(ty: &ParameterType) -> Result<usize, PluginError> {
    let payload = value_string_count(ty.element_type()) * aligned_size(MAX_STRING_BYTES);
    match ty {
        ParameterType::Value(_) => Ok(payload),
        ParameterType::Array {
            element, max_items, ..
        } => (abi_size(element) + payload)
            .checked_mul(*max_items as usize)
            .ok_or_else(|| PluginError::invalid_definition("parameter ABI maximum size overflows")),
    }
}
fn value_payload_size(value: &ParameterValue) -> usize {
    match value {
        ParameterValue::String(value) => aligned_size(value.len()),
        ParameterValue::Tuple(values) => values.iter().map(value_payload_size).sum(),
        _ => 0,
    }
}
fn dynamic_value_size(ty: &ParameterType, value: &ParameterValue) -> Result<usize, PluginError> {
    match (ty, value) {
        (ParameterType::Array { element, .. }, ParameterValue::Array(values)) => {
            values.iter().try_fold(0usize, |total, value| {
                total
                    .checked_add(abi_size(element) + value_payload_size(value))
                    .ok_or_else(|| PluginError::invalid_definition("parameter ABI size overflows"))
            })
        }
        _ => Ok(value_payload_size(value)),
    }
}

const fn aligned_size(size: usize) -> usize {
    size.saturating_add(size_of::<u32>() - 1) / size_of::<u32>() * size_of::<u32>()
}

fn generate_parameter_interface(
    fields: &[CompiledParameterField],
    names: ParameterInterfaceNames,
) -> String {
    let ParameterInterfaceNames {
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
            matches!(field.value_type(), ParameterValueType::Tuple(_))
                .then(|| format!("{struct_name}Tuple{index}"))
        })
        .collect::<Vec<_>>();
    let mut source = String::new();
    if fields
        .iter()
        .any(|field| value_string_count(field.value_type()) > 0)
    {
        source.push_str("struct ZeriumString {\n    _offset: u32,\n    byte_len: u32,\n};\n\n");
        source.push_str(
            "fn zerium_string_byte(raw: ZeriumRawParams, value: ZeriumString, index: u32) -> u32 {\n",
        );
        source.push_str("    if index >= value.byte_len { return 0u; }\n");
        source.push_str("    let byte_offset = value._offset + index;\n");
        source.push_str("    let word = zerium_raw_u32(raw, byte_offset & 0xfffffffcu);\n");
        source.push_str("    return (word >> ((byte_offset & 3u) * 8u)) & 0xffu;\n}\n\n");
    }
    for (field, tuple_name) in fields.iter().zip(&tuple_names) {
        let (ParameterValueType::Tuple(tuple), Some(tuple_name)) = (field.value_type(), tuple_name)
        else {
            continue;
        };
        source.push_str(&format!("struct {tuple_name} {{\n"));
        for (index, element) in tuple.elements().iter().enumerate() {
            source.push_str(&format!("    v{index}: {},\n", scalar_wgsl_type(element)));
        }
        source.push_str("};\n\n");
    }
    source.push_str(&format!(
        "struct {struct_name} {{\n    _raw: ZeriumRawParams,\n"
    ));
    for (field, tuple_name) in fields.iter().zip(&tuple_names) {
        if matches!(field.ty, ParameterType::Array { .. }) {
            source.push_str(&format!("    {}_len: u32,\n", field.id));
        } else {
            source.push_str(&format!(
                "    {}: {},\n",
                field.id,
                value_wgsl_type(field.value_type(), tuple_name.as_deref())
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
        let load = if matches!(field.ty, ParameterType::Array { .. }) {
            format!("zerium_raw_u32(raw, {}u)", field.offset + 4)
        } else {
            value_wgsl_load_at(
                field.value_type(),
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
        let ParameterType::Array { element, .. } = &field.ty else {
            continue;
        };
        source.push_str(&format!(
            "\nfn {accessor_prefix}_{id}_get(params: {struct_name}, index: u32) -> {ty} {{\n",
            id = field.id,
            ty = value_wgsl_type(element, tuple_name.as_deref()),
        ));
        source.push_str(&format!(
            "    if index >= params.{id}_len {{ return {zero}; }}\n",
            id = field.id,
            zero = value_wgsl_zero(element, tuple_name.as_deref()),
        ));
        source.push_str(&format!(
            "    let byte_offset = zerium_raw_u32(params._raw, {}u) + index * {}u;\n",
            field.offset,
            abi_size(element),
        ));
        source.push_str(&format!(
            "    return {};\n}}\n",
            value_wgsl_load_at(element, tuple_name.as_deref(), "params._raw", "byte_offset")
        ));
    }
    source
}

const fn scalar_abi_size(ty: &ScalarParameterType) -> usize {
    let word_count = match ty {
        ScalarParameterType::F32
        | ScalarParameterType::I32
        | ScalarParameterType::U32
        | ScalarParameterType::Bool
        | ScalarParameterType::Enum(_) => 1,
        ScalarParameterType::Color => 4,
        ScalarParameterType::String => 2,
    };
    word_count * size_of::<u32>()
}

fn abi_size(ty: &ParameterValueType) -> usize {
    match ty {
        ParameterValueType::Scalar(ty) => scalar_abi_size(ty),
        ParameterValueType::Tuple(tuple) => tuple.elements().iter().map(scalar_abi_size).sum(),
    }
}

const fn scalar_wgsl_type(ty: &ScalarParameterType) -> &'static str {
    match ty {
        ScalarParameterType::F32 => "f32",
        ScalarParameterType::I32 => "i32",
        ScalarParameterType::U32 | ScalarParameterType::Enum(_) => "u32",
        ScalarParameterType::Bool => "bool",
        ScalarParameterType::Color => "vec4<f32>",
        ScalarParameterType::String => "ZeriumString",
    }
}

fn value_wgsl_type(ty: &ParameterValueType, tuple_name: Option<&str>) -> String {
    match ty {
        ParameterValueType::Scalar(ty) => scalar_wgsl_type(ty).to_owned(),
        ParameterValueType::Tuple(_) => tuple_name
            .expect("tuple fields have a generated WGSL type")
            .to_owned(),
    }
}

const fn scalar_wgsl_zero(ty: &ScalarParameterType) -> &'static str {
    match ty {
        ScalarParameterType::F32 => "0.0",
        ScalarParameterType::I32 => "0i",
        ScalarParameterType::U32 | ScalarParameterType::Enum(_) => "0u",
        ScalarParameterType::Bool => "false",
        ScalarParameterType::Color => "vec4(0.0)",
        ScalarParameterType::String => "ZeriumString(0u, 0u)",
    }
}

fn value_wgsl_zero(ty: &ParameterValueType, tuple_name: Option<&str>) -> String {
    match ty {
        ParameterValueType::Scalar(ty) => scalar_wgsl_zero(ty).to_owned(),
        ParameterValueType::Tuple(tuple) => format!(
            "{}({})",
            tuple_name.expect("tuple fields have a generated WGSL type"),
            tuple
                .elements()
                .iter()
                .map(|element| scalar_wgsl_zero(element))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn scalar_wgsl_load_at(ty: &ScalarParameterType, raw: &str, offset: &str) -> String {
    match ty {
        ScalarParameterType::F32 => format!("zerium_raw_f32({raw}, {offset})"),
        ScalarParameterType::I32 => format!("zerium_raw_i32({raw}, {offset})"),
        ScalarParameterType::U32 | ScalarParameterType::Enum(_) => {
            format!("zerium_raw_u32({raw}, {offset})")
        }
        ScalarParameterType::Bool => format!("zerium_raw_bool({raw}, {offset})"),
        ScalarParameterType::Color => format!(
            "vec4(zerium_raw_f32({raw}, {offset}), zerium_raw_f32({raw}, {offset} + 4u), \
             zerium_raw_f32({raw}, {offset} + 8u), zerium_raw_f32({raw}, {offset} + 12u))"
        ),
        ScalarParameterType::String => format!(
            "ZeriumString(zerium_raw_u32({raw}, {offset}), \
             zerium_raw_u32({raw}, {offset} + 4u))"
        ),
    }
}

fn value_wgsl_load_at(
    ty: &ParameterValueType,
    tuple_name: Option<&str>,
    raw: &str,
    offset: &str,
) -> String {
    match ty {
        ParameterValueType::Scalar(ty) => scalar_wgsl_load_at(ty, raw, offset),
        ParameterValueType::Tuple(tuple) => {
            let mut byte_offset = 0;
            let elements = tuple
                .elements()
                .iter()
                .map(|element| {
                    let load_offset = if byte_offset == 0 {
                        offset.to_owned()
                    } else {
                        format!("{offset} + {byte_offset}u")
                    };
                    byte_offset += scalar_abi_size(element);
                    scalar_wgsl_load_at(element, raw, &load_offset)
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{}({elements})",
                tuple_name.expect("tuple fields have a generated WGSL type")
            )
        }
    }
}

fn pack_scalar(
    bytes: &mut Vec<u8>,
    offset: usize,
    ty: &ScalarParameterType,
    value: &ParameterValue,
) -> Result<(), PluginError> {
    match (ty, value) {
        (ScalarParameterType::F32, ParameterValue::F32(value)) => {
            write_u32(bytes, offset, value.to_bits());
        }
        (ScalarParameterType::I32, ParameterValue::I32(value)) => {
            write_u32(bytes, offset, *value as u32);
        }
        (ScalarParameterType::U32, ParameterValue::U32(value))
        | (ScalarParameterType::Enum(_), ParameterValue::Enum(value)) => {
            write_u32(bytes, offset, *value);
        }
        (ScalarParameterType::Bool, ParameterValue::Bool(value)) => {
            write_u32(bytes, offset, u32::from(*value));
        }
        (ScalarParameterType::Color, ParameterValue::Color(values)) => {
            for (index, value) in values.iter().enumerate() {
                write_u32(bytes, offset + index * 4, value.to_bits());
            }
        }
        (ScalarParameterType::String, ParameterValue::String(value)) => {
            let (data_offset, byte_len) = append_bytes(bytes, value.as_bytes())?;
            write_u32(bytes, offset, data_offset);
            write_u32(bytes, offset + 4, byte_len);
        }
        _ => unreachable!("parameter value type was validated before packing"),
    }
    Ok(())
}

fn pack_value(
    bytes: &mut Vec<u8>,
    offset: usize,
    ty: &ParameterValueType,
    value: &ParameterValue,
) -> Result<(), PluginError> {
    match (ty, value) {
        (ParameterValueType::Scalar(ty), value) => pack_scalar(bytes, offset, ty, value)?,
        (ParameterValueType::Tuple(tuple), ParameterValue::Tuple(values)) => {
            let mut byte_offset = offset;
            for (element, value) in tuple.elements().iter().zip(values) {
                let ty = element;
                pack_scalar(bytes, byte_offset, ty, value)?;
                byte_offset += scalar_abi_size(ty);
            }
        }
        _ => unreachable!("parameter value type was validated before packing"),
    }
    Ok(())
}

fn append_bytes(bytes: &mut Vec<u8>, value: &[u8]) -> Result<(u32, u32), PluginError> {
    let offset = u32::try_from(bytes.len())
        .map_err(|_| PluginError::invalid_definition("parameter data exceeds u32"))?;
    let byte_len = u32::try_from(value.len())
        .map_err(|_| PluginError::invalid_definition("string value exceeds u32"))?;
    let aligned_end = bytes
        .len()
        .checked_add(value.len())
        .map(aligned_size)
        .ok_or_else(|| PluginError::invalid_definition("parameter data size overflows"))?;
    if aligned_end > MAX_PARAMETER_BLOCK_BYTES {
        return Err(PluginError::invalid_definition(
            "parameter data exceeds the ABI byte budget",
        ));
    }
    bytes.extend_from_slice(value);
    bytes.resize(aligned_end, 0);
    Ok((offset, byte_len))
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
