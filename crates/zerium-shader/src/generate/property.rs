use crate::ShaderProperty;
use zerium_core::{
    plugin::{ShaderKind, abi_size, scalar_abi_size, value_string_count},
    property::{PropertyType, PropertyValueType, ScalarPropertyType},
};

pub(super) fn interface(fields: &[ShaderProperty], kind: ShaderKind) -> String {
    if fields.is_empty() {
        return String::new();
    }
    let stores_raw = fields
        .iter()
        .any(|field| matches!(field.ty, PropertyType::Array { .. }));
    let (raw_load_function, arguments, raw_arguments) = match kind {
        ShaderKind::Item => ("item_props", "instance_index: u32", "instance_index"),
        ShaderKind::Effect | ShaderKind::Compute | ShaderKind::Temporal => ("effect_props", "", ""),
    };
    let typed_fields = fields.iter().enumerate().map(|(index, field)| {
        let value_type = field.ty.value_type();
        let tuple_name = matches!(value_type, PropertyValueType::Tuple(_))
            .then(|| format!("ZeriumPropsTuple{index}"));
        (field, tuple_name)
    });
    let mut source = format!(
        "import package::generated::host::_props::{{ZeriumRawProps, {raw_load_function}, read_u32, read_i32, read_f32, read_bool}};\n\n"
    );
    if fields.iter().any(|field| {
        let value_type = field.ty.value_type();
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
        let value_type = field.ty.value_type();
        let (PropertyValueType::Tuple(tuple), Some(tuple_name)) = (value_type, tuple_name) else {
            continue;
        };
        source.push_str(&format!("struct {tuple_name} {{\n"));
        for (index, scalar_type) in tuple
            .scalars()
            .iter()
            .enumerate()
            .filter(|(_, ty)| ty.is_shader_value())
        {
            source.push_str(&format!(
                "    v{index}: {},\n",
                scalar_type_name(scalar_type)
            ));
        }
        source.push_str("};\n\n");
    }
    source.push_str("struct ZeriumProps {\n");
    if stores_raw {
        source.push_str("    _raw: ZeriumRawProps,\n");
    }
    for (field, tuple_name) in typed_fields.clone() {
        if matches!(field.ty, PropertyType::Array { .. }) {
            source.push_str(&format!("    {}_len: u32,\n", field.id));
        } else {
            let value_type = field.ty.value_type();
            source.push_str(&format!(
                "    {}: {},\n",
                field.id,
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
        let load = if matches!(field.ty, PropertyType::Array { .. }) {
            format!("read_u32(raw, {}u)", field.offset + 4)
        } else {
            let value_type = field.ty.value_type();
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
        let PropertyType::Array { element_type, .. } = &field.ty else {
            continue;
        };
        source.push_str(&format!(
            "\nfn get_{id}(properties: ZeriumProps, index: u32) -> {ty} {{\n",
            id = field.id,
            ty = value_type_name(element_type, tuple_name.as_deref()),
        ));
        source.push_str(&format!(
            "    if index >= properties.{id}_len {{ return {zero}; }}\n",
            id = field.id,
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

fn value_type_name(ty: &PropertyValueType, tuple_name: Option<&str>) -> String {
    match ty {
        PropertyValueType::Scalar(ty) => scalar_type_name(ty).to_owned(),
        PropertyValueType::Tuple(_) => tuple_name
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

fn value_zero(ty: &PropertyValueType, tuple_name: Option<&str>) -> String {
    match ty {
        PropertyValueType::Scalar(ty) => scalar_zero(ty).to_owned(),
        PropertyValueType::Tuple(tuple) => format!(
            "{}({})",
            tuple_name.expect("tuple fields have a generated shader type"),
            tuple
                .scalars()
                .iter()
                .filter(|ty| ty.is_shader_value())
                .map(scalar_zero)
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

fn value_load(ty: &PropertyValueType, tuple_name: Option<&str>, raw: &str, offset: &str) -> String {
    match ty {
        PropertyValueType::Scalar(ty) => scalar_load(ty, raw, offset),
        PropertyValueType::Tuple(tuple) => {
            let mut byte_offset = 0;
            let scalars = tuple
                .scalars()
                .iter()
                .filter(|ty| ty.is_shader_value())
                .map(|scalar_type| {
                    let load_offset = if byte_offset == 0 {
                        offset.to_owned()
                    } else {
                        format!("{offset} + {byte_offset}u")
                    };
                    byte_offset += scalar_abi_size(scalar_type);
                    scalar_load(scalar_type, raw, &load_offset)
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
