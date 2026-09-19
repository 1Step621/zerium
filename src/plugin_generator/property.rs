use crate::domain::{
    plugin::{ShaderKind, abi_size, scalar_abi_size, value_string_count},
    property::{PropertyType, PropertyValueType, ScalarPropertyType},
};

#[derive(Clone, PartialEq, Eq)]
struct InterfaceField {
    id: String,
    ty: PropertyType,
    offset: usize,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Layout {
    fields: Vec<InterfaceField>,
}

impl Layout {
    pub(crate) fn from_declarations<'a>(
        declarations: impl IntoIterator<Item = (&'a str, &'a PropertyType)>,
    ) -> Self {
        let mut offset = 0;
        let fields = declarations
            .into_iter()
            .map(|(id, ty)| {
                let field = InterfaceField {
                    id: id.to_owned(),
                    ty: ty.clone(),
                    offset,
                };
                offset += header_abi_size(ty);
                field
            })
            .collect();
        Self { fields }
    }

    pub(crate) fn retain_compatible(&mut self, other: &Self) {
        self.fields.retain(|field| other.fields.contains(field));
    }

    pub(crate) fn interface(&self, kind: ShaderKind) -> String {
        generate_property_interface(&self.fields, kind)
    }
}

fn generate_property_interface(fields: &[InterfaceField], kind: ShaderKind) -> String {
    let struct_name = "ZeriumProps";
    let load_function = "props";
    let accessor_prefix = "get";
    let (raw_load_function, takes_instance_index) = match kind {
        ShaderKind::Item => ("item_props", true),
        ShaderKind::Effect | ShaderKind::Compute | ShaderKind::Temporal => ("effect_props", false),
    };
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
        source.push_str("struct ZeriumStr {\n    _offset: u32,\n    byte_len: u32,\n};\n\n");
        source
            .push_str("fn str_byte(raw: ZeriumRawProps, value: ZeriumStr, index: u32) -> u32 {\n");
        source.push_str("    if index >= value.byte_len { return 0u; }\n");
        source.push_str("    let byte_offset = value._offset + index;\n");
        source.push_str("    let word = read_u32(raw, byte_offset & 0xfffffffcu);\n");
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
                scalar_type_name(scalar_type)
            ));
        }
        source.push_str("};\n\n");
    }
    source.push_str(&format!(
        "struct {struct_name} {{\n    _raw: ZeriumRawProps,\n"
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
                value_type_name(value_type, tuple_name.as_deref())
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
            format!("read_u32(raw, {}u)", field.offset + 4)
        } else {
            let value_type = match &field.ty {
                PropertyType::Value(value_type)
                | PropertyType::Array {
                    element_type: value_type,
                    ..
                } => value_type,
            };
            value_load(
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
            "\nfn {accessor_prefix}_{id}(properties: {struct_name}, index: u32) -> {ty} {{\n",
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

fn header_abi_size(ty: &PropertyType) -> usize {
    match ty {
        PropertyType::Array { .. } => 8,
        PropertyType::Value(value_type) => abi_size(value_type),
    }
}

const fn scalar_type_name(ty: &ScalarPropertyType) -> &'static str {
    match ty {
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

const fn scalar_zero(ty: &ScalarPropertyType) -> &'static str {
    match ty {
        ScalarPropertyType::F32 => "0.0",
        ScalarPropertyType::I32 => "0i",
        ScalarPropertyType::U32 | ScalarPropertyType::Enum(_) => "0u",
        ScalarPropertyType::Bool => "false",
        ScalarPropertyType::Color => "vec4(0.0)",
        ScalarPropertyType::String => "ZeriumStr(0u, 0u)",
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
                .map(scalar_zero)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn scalar_load(ty: &ScalarPropertyType, raw: &str, offset: &str) -> String {
    match ty {
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
            "ZeriumStr(read_u32({raw}, {offset}), \
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
