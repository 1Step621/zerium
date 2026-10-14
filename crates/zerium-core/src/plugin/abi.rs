use std::mem::size_of;

use super::PluginError;
use super::identifier::validate_wgsl_identifier;
use crate::property::{
    PropertyDefinition, PropertySchema, PropertyValue, PropertyValues, ScalarPropertyType,
    ValueSchema,
};

/// Property blocks are copied for every rendered instance/pass. Large data belongs in a separate
/// resource, not in this per-frame ABI.
pub(super) const MAX_PROPERTY_BLOCK_BYTES: usize = 8 * 1024 * 1024;

/// Declarations and their compiled shader offsets share one immutable contract.
#[derive(Clone, Debug, PartialEq)]
pub struct PropertyLayout {
    properties: Vec<PropertySchema>,
    fields: Box<[PropertyLayoutField]>,
    header_size: usize,
    worst_case_size: usize,
}

#[derive(Clone, Debug, PartialEq)]
struct PropertyLayoutField {
    property_index: usize,
    offset: usize,
}

impl PropertyLayout {
    pub(super) fn compile(
        owner_kind: &str,
        owner_id: &str,
        properties: Vec<PropertySchema>,
    ) -> Result<Self, PluginError> {
        let mut ids = std::collections::HashSet::new();
        let mut shader_fields = std::collections::HashSet::from(["_raw".to_owned()]);
        let mut fields = Vec::with_capacity(properties.len());
        let mut header_size = 0_usize;
        let mut worst_case_size = 0_usize;
        for (property_index, property) in properties.iter().enumerate() {
            let id = property.id();
            validate_wgsl_identifier("property", id)?;
            if !ids.insert(id) {
                return Err(PluginError::invalid_definition(format!(
                    "{owner_kind} '{owner_id}' has duplicate property ID '{id}'"
                )));
            }
            if !property
                .value_schema()
                .scalars()
                .iter()
                .any(|scalar| scalar.ty.is_shader_value())
            {
                continue;
            }
            let field = if matches!(property.definition(), PropertyDefinition::Array { .. }) {
                format!("{id}_len")
            } else {
                id.to_owned()
            };
            if !shader_fields.insert(field.clone()) {
                return Err(PluginError::invalid_definition(format!(
                    "{owner_kind} '{owner_id}' has conflicting shader property field '{field}'"
                )));
            }
            let size = match property.definition() {
                PropertyDefinition::Array { .. } => 8,
                PropertyDefinition::Value(schema) => abi_size(schema),
            };
            let next = header_size.checked_add(size).ok_or_else(|| {
                PluginError::invalid_definition(format!(
                    "{owner_kind} '{owner_id}' property layout overflow"
                ))
            })?;
            fields.push(PropertyLayoutField {
                property_index,
                offset: header_size,
            });
            header_size = next;
            let payload_size = max_dynamic_size(property)?;
            worst_case_size = worst_case_size
                .checked_add(size)
                .and_then(|total| total.checked_add(payload_size))
                .ok_or_else(|| property_budget_error(owner_kind, owner_id))?;
        }

        if worst_case_size > MAX_PROPERTY_BLOCK_BYTES {
            return Err(property_budget_error(owner_kind, owner_id));
        }

        Ok(Self {
            properties,
            fields: fields.into_boxed_slice(),
            header_size,
            worst_case_size,
        })
    }

    pub fn properties(&self) -> &[PropertySchema] {
        &self.properties
    }

    pub fn fields(&self) -> impl ExactSizeIterator<Item = (&PropertySchema, usize)> {
        self.fields
            .iter()
            .map(|field| (&self.properties[field.property_index], field.offset))
    }

    pub fn pack(
        &self,
        owner_kind: &str,
        owner_id: &str,
        values: &PropertyValues,
    ) -> Result<Vec<u8>, PluginError> {
        values.validate_for(owner_kind, owner_id, &self.properties)?;
        let actual_size = self.actual_size(owner_kind, owner_id, values)?;
        if actual_size > MAX_PROPERTY_BLOCK_BYTES || actual_size > self.worst_case_size {
            return Err(property_budget_error(owner_kind, owner_id));
        }

        let mut bytes = vec![0; self.header_size];
        bytes.reserve(actual_size - self.header_size);
        for (property, offset) in self.fields() {
            let value = values
                .property(property.id())
                .expect("all properties were validated before packing");
            match (property.definition(), value) {
                (
                    PropertyDefinition::Array {
                        element: element_schema,
                        ..
                    },
                    PropertyValue::Array(values),
                ) => {
                    let data_offset = u32::try_from(bytes.len())
                        .map_err(|_| property_budget_error(owner_kind, owner_id))?;
                    write_u32(&mut bytes, offset, data_offset);
                    write_u32(&mut bytes, offset + 4, values.len() as u32);
                    let element_size = abi_size(element_schema);
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
                            element_schema,
                            element.value(),
                        );
                    }
                }
                (PropertyDefinition::Value(schema), value) => {
                    pack_value(&mut bytes, offset, schema, value);
                }
                _ => unreachable!("property values were validated before packing"),
            }
        }
        debug_assert_eq!(bytes.len(), actual_size);
        Ok(bytes)
    }

    fn actual_size(
        &self,
        owner_kind: &str,
        owner_id: &str,
        values: &PropertyValues,
    ) -> Result<usize, PluginError> {
        let mut size = self.header_size;
        for (property, _) in self.fields() {
            let value = values
                .property(property.id())
                .expect("all properties were validated before packing");
            size = size
                .checked_add(dynamic_value_size(property, value)?)
                .ok_or_else(|| property_budget_error(owner_kind, owner_id))?;
            if size > MAX_PROPERTY_BLOCK_BYTES {
                return Err(property_budget_error(owner_kind, owner_id));
            }
        }
        Ok(size)
    }
}

fn property_budget_error(owner_kind: &str, owner_id: &str) -> PluginError {
    PluginError::invalid_definition(format!(
        "{owner_kind} '{owner_id}' property ABI exceeds the {} byte budget",
        MAX_PROPERTY_BLOCK_BYTES
    ))
}

fn max_dynamic_size(property: &PropertySchema) -> Result<usize, PluginError> {
    match property.definition() {
        PropertyDefinition::Value(_) => Ok(0),
        PropertyDefinition::Array {
            element, max_items, ..
        } => abi_size(element)
            .checked_mul(*max_items as usize)
            .ok_or_else(|| PluginError::invalid_definition("property ABI maximum size overflows")),
    }
}

pub const fn scalar_abi_size(ty: &ScalarPropertyType) -> usize {
    let word_count = match ty {
        ScalarPropertyType::File | ScalarPropertyType::String => 0,
        ScalarPropertyType::F32
        | ScalarPropertyType::I32
        | ScalarPropertyType::U32
        | ScalarPropertyType::Bool
        | ScalarPropertyType::Enum(_) => 1,
        ScalarPropertyType::Color => 4,
    };
    word_count * size_of::<u32>()
}

pub fn abi_size(schema: &ValueSchema) -> usize {
    schema
        .scalars()
        .iter()
        .map(|scalar| scalar_abi_size(&scalar.ty))
        .sum()
}

fn dynamic_value_size(
    property: &PropertySchema,
    value: &PropertyValue,
) -> Result<usize, PluginError> {
    match (property.definition(), value) {
        (PropertyDefinition::Array { element, .. }, PropertyValue::Array(values)) => values
            .len()
            .checked_mul(abi_size(element))
            .ok_or_else(|| PluginError::invalid_definition("property ABI size overflows")),
        _ => Ok(0),
    }
}

fn pack_scalar(bytes: &mut [u8], offset: usize, ty: &ScalarPropertyType, value: &PropertyValue) {
    match (ty, value) {
        (ScalarPropertyType::File, PropertyValue::File(_))
        | (ScalarPropertyType::String, PropertyValue::String(_)) => {}
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
        _ => unreachable!("property value type was validated before packing"),
    }
}

fn pack_value(bytes: &mut [u8], offset: usize, schema: &ValueSchema, value: &PropertyValue) {
    match (schema, value) {
        (ValueSchema::Scalar(scalar), value) => pack_scalar(bytes, offset, &scalar.ty, value),
        (ValueSchema::Tuple(tuple), PropertyValue::Tuple(values)) => {
            let mut byte_offset = offset;
            for (scalar, value) in tuple.iter().zip(values) {
                pack_scalar(bytes, byte_offset, &scalar.ty, value);
                byte_offset += scalar_abi_size(&scalar.ty);
            }
        }
        _ => unreachable!("property value type was validated before packing"),
    }
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
