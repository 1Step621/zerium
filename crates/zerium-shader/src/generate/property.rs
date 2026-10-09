use crate::ShaderProperty;
use zerium_core::{
    plugin::{ShaderKind, abi_size, scalar_abi_size, value_string_count},
    property::{PropertyDefinition, ScalarPropertyType, ValueSchema},
};

pub(super) fn interface(fields: &[ShaderProperty], kind: ShaderKind) -> String {
    if fields.is_empty() {
        return String::new();
    }
    let stores_raw = fields
        .iter()
        .any(|field| matches!(field.schema.definition(), PropertyDefinition::Array { .. }));
    let (raw_load_function, arguments, raw_arguments) = match kind {
        ShaderKind::Item => ("item_props", "instance_index: u32", "instance_index"),
        ShaderKind::Effect | ShaderKind::Compute | ShaderKind::Temporal => ("effect_props", "", ""),
    };
    let typed_fields = fields.iter().enumerate().map(|(index, field)| {
        let value_type = field.schema.value_schema();
        let tuple_name =
            matches!(value_type, ValueSchema::Tuple(_)).then(|| format!("ZeriumPropsTuple{index}"));
        (field, tuple_name)
    });
    let mut source = format!(
        "import package::generated::host::_props::{{ZeriumRawProps, {raw_load_function}, read_u32, read_i32, read_f32, read_bool}};\n\n"
    );
    if fields.iter().any(|field| {
        let value_type = field.schema.value_schema();
        value_string_count(value_type) > 0
    }) {
        source.push_str("struct ZeriumStr {\n    _raw: ZeriumRawProps,\n    _offset: u32,\n    byte_len: u32,\n};\n\n");
        source.push_str("fn str_byte(value: ZeriumStr, index: u32) -> u32 {\n");
        source.push_str("    if index >= value.byte_len { return 0u; }\n");
        source.push_str("    let byte_offset = value._offset + index;\n");
        source.push_str("    let word = read_u32(value._raw, byte_offset & 0xfffffffcu);\n");
        source.push_str("    return (word >> ((byte_offset & 3u) * 8u)) & 0xffu;\n}\n\n");
    }
    for (field, tuple_name) in typed_fields.clone() {
        let value_type = field.schema.value_schema();
        let (ValueSchema::Tuple(tuple), Some(tuple_name)) = (value_type, tuple_name) else {
            continue;
        };
        source.push_str(&format!("struct {tuple_name} {{\n"));
        for (index, scalar_type) in tuple
            .iter()
            .enumerate()
            .filter(|(_, scalar)| scalar.ty.is_shader_value())
        {
            source.push_str(&format!(
                "    v{index}: {},\n",
                scalar_type_name(&scalar_type.ty)
            ));
        }
        source.push_str("};\n\n");
    }
    source.push_str("struct ZeriumProps {\n");
    if stores_raw {
        source.push_str("    _raw: ZeriumRawProps,\n");
    }
    for (field, tuple_name) in typed_fields.clone() {
        if matches!(field.schema.definition(), PropertyDefinition::Array { .. }) {
            source.push_str(&format!("    {}_len: u32,\n", field.schema.id()));
        } else {
            let value_type = field.schema.value_schema();
            source.push_str(&format!(
                "    {}: {},\n",
                field.schema.id(),
                value_type_name(value_type, tuple_name.as_deref())
            ));
        }
    }
    source.push_str(&format!("}};\n\nfn props({arguments}) -> ZeriumProps {{\n"));
    source.push_str(&format!(
        "    let raw = {raw_load_function}({raw_arguments});\n"
    ));
    let mut loads = Vec::new();
    if stores_raw {
        loads.push("raw".to_owned());
    }
    for (field, tuple_name) in typed_fields.clone() {
        let load = if matches!(field.schema.definition(), PropertyDefinition::Array { .. }) {
            format!("read_u32(raw, {}u)", field.offset + 4)
        } else {
            let value_type = field.schema.value_schema();
            value_load(
                value_type,
                tuple_name.as_deref(),
                "raw",
                &format!("{}u", field.offset),
            )
        };
        loads.push(load);
    }
    source.push_str(&format!(
        "    return ZeriumProps({});\n}}\n",
        loads.join(", ")
    ));

    for (field, tuple_name) in typed_fields {
        let PropertyDefinition::Array {
            element: element_type,
            ..
        } = field.schema.definition()
        else {
            continue;
        };
        source.push_str(&format!(
            "\nfn get_{id}(properties: ZeriumProps, index: u32) -> {ty} {{\n",
            id = field.schema.id(),
            ty = value_type_name(element_type, tuple_name.as_deref()),
        ));
        source.push_str(&format!(
            "    if index >= properties.{id}_len {{ return {zero}; }}\n",
            id = field.schema.id(),
            zero = value_zero(element_type, tuple_name.as_deref()),
        ));
        source.push_str(&format!(
            "    let byte_offset = read_u32(properties._raw, {}u) + index * {}u;\n",
            field.offset,
            abi_size(element_type),
        ));
        source.push_str(&format!(
            "    return {};\n}}\n",
            value_load(
                element_type,
                tuple_name.as_deref(),
                "properties._raw",
                "byte_offset"
            )
        ));
    }
    source
}

fn scalar_type_name(ty: &ScalarPropertyType) -> &'static str {
    match ty {
        ScalarPropertyType::File => {
            unreachable!("file properties have no shader representation")
        }
        ScalarPropertyType::F32 => "f32",
        ScalarPropertyType::I32 => "i32",
        ScalarPropertyType::U32 | ScalarPropertyType::Enum(_) => "u32",
        ScalarPropertyType::Bool => "bool",
        ScalarPropertyType::Color => "vec4<f32>",
        ScalarPropertyType::String => "ZeriumStr",
    }
}

fn value_type_name(ty: &ValueSchema, tuple_name: Option<&str>) -> String {
    match ty {
        ValueSchema::Scalar(scalar) => scalar_type_name(&scalar.ty).to_owned(),
        ValueSchema::Tuple(_) => tuple_name
            .expect("tuple fields have a generated shader type")
            .to_owned(),
    }
}

fn scalar_zero(ty: &ScalarPropertyType) -> &'static str {
    match ty {
        ScalarPropertyType::File => {
            unreachable!("file properties have no shader representation")
        }
        ScalarPropertyType::F32 => "0.0",
        ScalarPropertyType::I32 => "0i",
        ScalarPropertyType::U32 | ScalarPropertyType::Enum(_) => "0u",
        ScalarPropertyType::Bool => "false",
        ScalarPropertyType::Color => "vec4(0.0)",
        ScalarPropertyType::String => "ZeriumStr(ZeriumRawProps(0u, 0u), 0u, 0u)",
    }
}

fn value_zero(ty: &ValueSchema, tuple_name: Option<&str>) -> String {
    match ty {
        ValueSchema::Scalar(scalar) => scalar_zero(&scalar.ty).to_owned(),
        ValueSchema::Tuple(tuple) => format!(
            "{}({})",
            tuple_name.expect("tuple fields have a generated shader type"),
            tuple
                .iter()
                .filter(|scalar| scalar.ty.is_shader_value())
                .map(|scalar| scalar_zero(&scalar.ty))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn scalar_load(ty: &ScalarPropertyType, raw: &str, offset: &str) -> String {
    match ty {
        ScalarPropertyType::File => {
            unreachable!("file properties have no shader representation")
        }
        ScalarPropertyType::F32 => format!("read_f32({raw}, {offset})"),
        ScalarPropertyType::I32 => format!("read_i32({raw}, {offset})"),
        ScalarPropertyType::U32 | ScalarPropertyType::Enum(_) => {
            format!("read_u32({raw}, {offset})")
        }
        ScalarPropertyType::Bool => format!("read_bool({raw}, {offset})"),
        ScalarPropertyType::Color => format!(
            "vec4(read_f32({raw}, {offset}), read_f32({raw}, {offset} + 4u), \
             read_f32({raw}, {offset} + 8u), read_f32({raw}, {offset} + 12u))"
        ),
        ScalarPropertyType::String => format!(
            "ZeriumStr({raw}, read_u32({raw}, {offset}), \
             read_u32({raw}, {offset} + 4u))"
        ),
    }
}

fn value_load(ty: &ValueSchema, tuple_name: Option<&str>, raw: &str, offset: &str) -> String {
    match ty {
        ValueSchema::Scalar(scalar) => scalar_load(&scalar.ty, raw, offset),
        ValueSchema::Tuple(tuple) => {
            let mut byte_offset = 0;
            let scalars = tuple
                .iter()
                .filter(|scalar| scalar.ty.is_shader_value())
                .map(|scalar_type| {
                    let load_offset = if byte_offset == 0 {
                        offset.to_owned()
                    } else {
                        format!("{offset} + {byte_offset}u")
                    };
                    byte_offset += scalar_abi_size(&scalar_type.ty);
                    scalar_load(&scalar_type.ty, raw, &load_offset)
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{}({scalars})",
                tuple_name.expect("tuple fields have a generated shader type")
            )
        }
    }
}
