use std::mem::size_of;

use super::PluginError;
use super::identifier::validate_wgsl_identifier;
use crate::domain::property::{
    MAX_STRING_BYTES, PropertyType, PropertyValue, PropertyValueType, ScalarPropertyType,
};

/// Property blocks are copied for every rendered instance/pass. Large data belongs in a separate
/// resource, not in this per-frame ABI.
pub(super) const MAX_PROPERTY_BLOCK_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct CompiledPropertyAbi {
    fields: Box<[CompiledPropertyField]>,
    header_size: usize,
    worst_case_size: usize,
}

#[derive(Clone, Debug, PartialEq)]
struct CompiledPropertyField {
    id: Box<str>,
    ty: PropertyType,
    offset: usize,
}

impl CompiledPropertyAbi {
    pub(super) fn compile<'a>(
        owner_kind: &str,
        owner_id: &str,
        declarations: impl IntoIterator<Item = (&'a str, &'a PropertyType)>,
    ) -> Result<Self, PluginError> {
        let declarations = declarations.into_iter().collect::<Vec<_>>();
        validate_property_names(owner_kind, owner_id, declarations.iter().copied())?;

        let mut fields = Vec::with_capacity(declarations.len());
        let mut header_size = 0_usize;
        for &(id, ty) in &declarations {
            let size = header_abi_size(ty);
            let next = header_size.checked_add(size).ok_or_else(|| {
                PluginError::invalid_definition(format!(
                    "{owner_kind} '{owner_id}' property layout overflow"
                ))
            })?;
            fields.push(CompiledPropertyField {
                id: id.into(),
                ty: ty.clone(),
                offset: header_size,
            });
            header_size = next;
        }

        let mut worst_case_size = header_size;
        for field in &fields {
            worst_case_size = worst_case_size
                .checked_add(max_dynamic_size(&field.ty)?)
                .ok_or_else(|| property_budget_error(owner_kind, owner_id))?;
        }
        if worst_case_size > MAX_PROPERTY_BLOCK_BYTES {
            return Err(property_budget_error(owner_kind, owner_id));
        }

        Ok(Self {
            fields: fields.into_boxed_slice(),
            header_size,
            worst_case_size,
        })
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

        let mut bytes = vec![0; self.header_size];
        bytes.reserve(actual_size - self.header_size);
        for field in &self.fields {
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
        let mut size = self.header_size;
        for field in &self.fields {
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
                "{owner_kind} '{owner_id}' has conflicting shader property field '{field}'"
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

pub(crate) fn value_string_count(ty: &PropertyValueType) -> usize {
    match ty {
        PropertyValueType::Scalar(ty) => usize::from(matches!(ty, ScalarPropertyType::String)),
        PropertyValueType::Tuple(tuple) => tuple
            .scalars()
            .iter()
            .filter(|ty| matches!(ty, ScalarPropertyType::String))
            .count(),
    }
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
pub(crate) const fn scalar_abi_size(ty: &ScalarPropertyType) -> usize {
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

pub(crate) fn abi_size(ty: &PropertyValueType) -> usize {
    match ty {
        PropertyValueType::Scalar(ty) => scalar_abi_size(ty),
        PropertyValueType::Tuple(tuple) => tuple.scalars().iter().map(scalar_abi_size).sum(),
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
