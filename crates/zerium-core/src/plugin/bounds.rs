//! Plugin declarations for the logical area produced by a visual operation.

use evalexpr::{ContextWithMutableVariables, HashMapContext, Node, Value};
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
        let aliases = property_aliases(name, value);
        for alias in aliases {
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

fn property_aliases(id: &str, value: &PropertyValue) -> Vec<String> {
    match value {
        PropertyValue::F32(_) => vec![format!("p::{id}")],
        PropertyValue::Tuple(values) => values
            .iter()
            .enumerate()
            .filter(|(_, value)| matches!(value, PropertyValue::F32(_)))
            .map(|(index, _)| format!("p::{id}::v{index}"))
            .collect(),
        _ => Vec::new(),
    }
}

pub fn program_context(
    input_min: [f64; 2],
    input_max: [f64; 2],
    viewport_min: [f64; 2],
    viewport_max: [f64; 2],
    values: &PropertyValues,
) -> HashMapContext {
    let mut context = HashMapContext::new();
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
        match value {
            PropertyValue::F32(value) => {
                set_bound_variable(&mut context, format!("p::{id}"), f64::from(*value));
            }
            PropertyValue::Tuple(values) => {
                for (value, index) in values.iter().zip(0..) {
                    if let PropertyValue::F32(value) = value {
                        set_bound_variable(
                            &mut context,
                            format!("p::{id}::v{index}"),
                            f64::from(*value),
                        );
                    }
                }
            }
            _ => {}
        }
    }
    context
}

fn set_bound_variable(context: &mut HashMapContext, name: String, value: f64) {
    context
        .set_value(name, Value::from_float(value))
        .expect("bounds variable names are validated plugin identifiers");
}
