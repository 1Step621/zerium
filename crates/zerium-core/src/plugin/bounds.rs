//! Plugin declarations for the logical area produced by a visual operation.

use evalexpr::{
    ContextWithMutableFunctions, ContextWithMutableVariables, EvalexprError, Function,
    HashMapContext, Node, Value,
};
use serde::Deserialize;
use std::collections::HashSet;

use super::PluginError;
use crate::property::{PropertySchema, PropertyValue, PropertyValues};

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OutputBoundsSchema {
    pub min: [Node; 2],
    pub max: [Node; 2],
}

impl OutputBoundsSchema {
    pub(super) fn validate(
        &self,
        owner: &str,
        id: &str,
        properties: &[PropertySchema],
    ) -> Result<(), PluginError> {
        let Self { min, max, .. } = self;
        validate_program(owner, id, min, max, properties)
    }
}

fn validate_program(
    owner: &str,
    id: &str,
    min: &[Node; 2],
    max: &[Node; 2],
    properties: &[PropertySchema],
) -> Result<(), PluginError> {
    let defaults = PropertyValues::from_properties(properties);
    let mut names = HashSet::new();
    for (name, value) in defaults.iter() {
        let aliases = property_variables(name, value);
        for (alias, _) in aliases {
            if !names.insert(alias) {
                return Err(PluginError::invalid_definition(format!(
                    "{owner} '{id}' bounds program has ambiguous property variables"
                )));
            }
        }
    }
    let mut context = program_context(
        [-1.0, -1.0],
        [1.0, 1.0],
        [-960.0, -540.0],
        [960.0, 540.0],
        &defaults,
    );
    let mut result = [[0.0; 2]; 2];
    for (side, expressions) in [min, max].into_iter().enumerate() {
        for (axis, expression) in expressions.iter().enumerate() {
            result[side][axis] = expression
                .eval_number_with_context_mut(&mut context)
                .map_err(|error| {
                    PluginError::invalid_definition(format!(
                        "{owner} '{id}' bounds program expression is invalid: {error}"
                    ))
                })?;
            if !result[side][axis].is_finite() {
                return Err(PluginError::invalid_definition(format!(
                    "{owner} '{id}' bounds program expression must be finite at default values"
                )));
            }
        }
    }
    if (0..2).any(|axis| result[0][axis] > result[1][axis]) {
        return Err(PluginError::invalid_definition(format!(
            "{owner} '{id}' bounds program has reversed bounds at default values"
        )));
    }
    Ok(())
}

fn bound_scalar(value: &PropertyValue) -> Option<f64> {
    match value {
        PropertyValue::Enum(value) => Some(f64::from(*value)),
        _ => value.numeric_scalar(),
    }
}

fn property_variables<'a>(
    id: &'a str,
    value: &'a PropertyValue,
) -> impl Iterator<Item = (String, f64)> + 'a {
    let (scalars, tuple) = match value {
        PropertyValue::Tuple(values) => (values.as_slice(), true),
        value => (std::slice::from_ref(value), false),
    };
    scalars
        .iter()
        .enumerate()
        .filter_map(move |(index, value)| {
            let value = bound_scalar(value)?;
            let name = if tuple {
                format!("p::{id}::v{index}")
            } else {
                format!("p::{id}")
            };
            Some((name, value))
        })
}

pub fn program_context(
    input_min: [f64; 2],
    input_max: [f64; 2],
    viewport_min: [f64; 2],
    viewport_max: [f64; 2],
    values: &PropertyValues,
) -> HashMapContext {
    let mut context = HashMapContext::new();
    context
        .set_function(
            "placement_center".to_owned(),
            Function::new(|arguments: &Value| {
                let arguments = arguments.as_fixed_len_tuple(3)?;
                let position = arguments[0].as_number()?;
                let size = arguments[1].as_number()?;
                let origin = arguments[2].as_number()?;
                if !(0. ..=2.).contains(&origin) || origin.fract() != 0. {
                    return Err(EvalexprError::CustomMessage(
                        "placement origin must be an integer in 0..=2".to_owned(),
                    ));
                }
                Ok(Value::from_float(position + (1. - origin) * size * 0.5))
            }),
        )
        .expect("bounds function names are static identifiers");
    for (prefix, min, max) in [
        ("input", input_min, input_max),
        ("viewport", viewport_min, viewport_max),
    ] {
        for (axis, name) in ["x", "y"].into_iter().enumerate() {
            for (kind, value) in [
                ("min", min[axis]),
                ("max", max[axis]),
                ("center", (min[axis] + max[axis]) * 0.5),
                ("size", max[axis] - min[axis]),
            ] {
                set_bound_variable(&mut context, format!("{prefix}::{kind}::{name}"), value);
            }
        }
    }
    for (id, value) in values.iter() {
        for (name, value) in property_variables(id, value) {
            set_bound_variable(&mut context, name, value);
        }
    }
    context
}

fn set_bound_variable(context: &mut HashMapContext, name: String, value: f64) {
    context
        .set_value(name, Value::from_float(value))
        .expect("bounds variable names are validated plugin identifiers");
}
