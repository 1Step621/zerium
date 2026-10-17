use super::contract::PropertyScope;

use zerium_core::{
    plugin::{PropertyLayout, abi_size, scalar_abi_size},
    property::{PropertyDefinition, PropertySchema, ScalarPropertyType, ValueSchema},
};

struct Field<'a> {
    schema: &'a PropertySchema,
    offset: usize,
    tuple_name: Option<String>,
}

impl Field<'_> {
    fn is_array(&self) -> bool {
        matches!(self.schema.definition(), PropertyDefinition::Array { .. })
    }

    fn type_name(&self) -> String {
        value_type_name(self.schema.value_schema(), self.tuple_name.as_deref())
    }

    fn load(&self, raw: &str, offset: &str) -> String {
        value_load(
            self.schema.value_schema(),
            self.tuple_name.as_deref(),
            raw,
            offset,
        )
    }

    fn zero(&self) -> String {
        value_zero(self.schema.value_schema(), self.tuple_name.as_deref())
    }
}

pub(super) fn interface(layout: &PropertyLayout, scope: PropertyScope) -> String {
    let fields = layout
        .fields()
        .enumerate()
        .map(|(index, (schema, offset))| Field {
            schema,
            offset,
            tuple_name: matches!(schema.value_schema(), ValueSchema::Tuple(_))
                .then(|| format!("ZeriumPropsTuple{index}")),
        })
        .collect::<Vec<_>>();
    if fields.is_empty() {
        return String::new();
    }

    let stores_raw = fields.iter().any(Field::is_array);
    let (raw_module, raw_load_function, arguments, raw_arguments) = match scope {
        PropertyScope::Item => (
            "item",
            "item_props",
            "instance_index: u32",
            "instance_index",
        ),
        PropertyScope::Pass => ("effect_state", "pass_props", "", ""),
    };
    let mut source = format!(
        r#"import package::generated::_internal::properties::{{
    ZeriumRawProps,
    read_bool,
    read_f32,
    read_i32,
    read_u32
}};
import package::generated::_internal::{raw_module}::{raw_load_function};

"#
    );
    append_tuple_structs(&mut source, &fields);
    append_props_struct(&mut source, &fields, stores_raw);
    append_props_function(
        &mut source,
        &fields,
        stores_raw,
        raw_load_function,
        arguments,
        raw_arguments,
    );
    append_array_accessors(&mut source, &fields);
    source
}

fn append_tuple_structs(source: &mut String, fields: &[Field<'_>]) {
    for field in fields {
        let value_type = field.schema.value_schema();
        let (ValueSchema::Tuple(tuple), Some(tuple_name)) = (value_type, &field.tuple_name) else {
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
        source.push_str("}\n\n");
    }
}

fn append_props_struct(source: &mut String, fields: &[Field<'_>], stores_raw: bool) {
    source.push_str("struct ZeriumProps {\n");
    if stores_raw {
        source.push_str("    _raw: ZeriumRawProps,\n");
    }
    for field in fields {
        if field.is_array() {
            source.push_str(&format!("    {}_len: u32,\n", field.schema.id()));
        } else {
            source.push_str(&format!(
                "    {}: {},\n",
                field.schema.id(),
                field.type_name()
            ));
        }
    }
    source.push_str("}\n\n");
}

fn append_props_function(
    source: &mut String,
    fields: &[Field<'_>],
    stores_raw: bool,
    raw_load_function: &str,
    arguments: &str,
    raw_arguments: &str,
) {
    source.push_str(&format!("fn props({arguments}) -> ZeriumProps {{\n"));
    source.push_str(&format!(
        "    let raw = {raw_load_function}({raw_arguments});\n"
    ));
    let mut loads = Vec::new();
    if stores_raw {
        loads.push("raw".to_owned());
    }
    for field in fields {
        let load = if field.is_array() {
            format!("read_u32(raw, {}u)", field.offset + 4)
        } else {
            field.load("raw", &format!("{}u", field.offset))
        };
        loads.push(load);
    }

    source.push_str(&format!(
        "    return ZeriumProps(\n        {},\n    );\n}}\n",
        loads.join(",\n        ")
    ));
}

fn append_array_accessors(source: &mut String, fields: &[Field<'_>]) {
    for field in fields {
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
            ty = field.type_name(),
        ));
        source.push_str(&format!(
            "    if index >= properties.{id}_len {{\n        return {zero};\n    }}\n",
            id = field.schema.id(),
            zero = field.zero(),
        ));
        source.push_str(&format!(
            "    let byte_offset = read_u32(properties._raw, {}u) + index * {}u;\n",
            field.offset,
            abi_size(element_type),
        ));
        source.push_str(&format!(
            "    return {};\n}}\n",
            field.load("properties._raw", "byte_offset")
        ));
    }
}

fn scalar_type_name(ty: &ScalarPropertyType) -> &'static str {
    match ty {
        ScalarPropertyType::File | ScalarPropertyType::String => {
            unreachable!("CPU-only properties have no shader representation")
        }
        ScalarPropertyType::F32 => "f32",
        ScalarPropertyType::I32 => "i32",
        ScalarPropertyType::U32 | ScalarPropertyType::Enum(_) => "u32",
        ScalarPropertyType::Bool => "bool",
        ScalarPropertyType::Color => "vec4<f32>",
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
        ScalarPropertyType::File | ScalarPropertyType::String => {
            unreachable!("CPU-only properties have no shader representation")
        }
        ScalarPropertyType::F32 => "0.0",
        ScalarPropertyType::I32 => "0i",
        ScalarPropertyType::U32 | ScalarPropertyType::Enum(_) => "0u",
        ScalarPropertyType::Bool => "false",
        ScalarPropertyType::Color => "vec4(0.0)",
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
        ScalarPropertyType::File | ScalarPropertyType::String => {
            unreachable!("CPU-only properties have no shader representation")
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
