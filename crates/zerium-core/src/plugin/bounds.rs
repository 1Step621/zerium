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
        for (side, expressions) in [&self.min, &self.max].into_iter().enumerate() {
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
    for (name, side, axis) in [
        ("projected_rect_min_x", 0, 0),
        ("projected_rect_min_y", 0, 1),
        ("projected_rect_max_x", 1, 0),
        ("projected_rect_max_y", 1, 1),
    ] {
        context
            .set_function(
                name.to_owned(),
                Function::new(move |arguments| {
                    projected_rect(arguments).map(|bounds| Value::from_float(bounds[side][axis]))
                }),
            )
            .expect("bounds function names are static identifiers");
    }
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

fn invalid(message: &str) -> EvalexprError {
    EvalexprError::CustomMessage(format!("projected rectangle: {message}"))
}

fn number(value: &Value) -> Result<f64, EvalexprError> {
    let value = value.as_number()?;
    if !value.is_finite() {
        return Err(invalid("arguments must be finite"));
    }
    Ok(value)
}

fn numbers<const N: usize>(value: &Value) -> Result<[f64; N], EvalexprError> {
    let values = value.as_fixed_len_tuple(N)?;
    let mut result = [0.0; N];
    for (result, value) in result.iter_mut().zip(&values) {
        *result = number(value)?;
    }
    Ok(result)
}

fn projected_rect(arguments: &Value) -> Result<[[f64; 2]; 2], EvalexprError> {
    let arguments = arguments.as_fixed_len_tuple(6)?;
    let min: [f64; 2] = numbers(&arguments[0])?;
    let max: [f64; 2] = numbers(&arguments[1])?;
    let rotation: [f64; 3] = numbers(&arguments[2])?;
    let center: [f64; 2] = numbers(&arguments[3])?;
    let focal = number(&arguments[4])?;
    let near_ratio = number(&arguments[5])?;
    let near = focal * near_ratio;
    if (0..2).any(|axis| min[axis] > max[axis]) {
        return Err(invalid("minimum must not exceed maximum"));
    }
    if focal <= 0.0 || near_ratio <= 0.0 || !near.is_finite() || near <= 0.0 {
        return Err(invalid(
            "focal length and near depth must be finite and positive",
        ));
    }

    let [(sx, cx), (sy, cy), (sz, cz)] = rotation.map(|angle| angle.to_radians().sin_cos());
    // First two columns of Rz * Ry * Rx, matching the shader's z=0 plane.
    let columns = [
        [cz * cy, sz * cy, -sy],
        [cz * sy * sx - sz * cx, sz * sy * sx + cz * cx, cy * sx],
    ];
    let vertices = [
        [min[0], min[1]],
        [max[0], min[1]],
        [max[0], max[1]],
        [min[0], max[1]],
    ]
    .map(|point| {
        let [x, y] = [point[0] - center[0], point[1] - center[1]];
        [
            columns[0][0] * x + columns[1][0] * y,
            columns[0][1] * x + columns[1][1] * y,
            focal + columns[0][2] * x + columns[1][2] * y,
        ]
    });
    if vertices.iter().flatten().any(|value| !value.is_finite()) {
        return Err(invalid("rotated vertices must be finite"));
    }

    let mut bounds = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
    let mut visible = false;
    let mut include = |vertex: [f64; 3]| -> Result<(), EvalexprError> {
        for axis in 0..2 {
            let position = center[axis] + (focal / vertex[2]) * vertex[axis];
            if !position.is_finite() {
                return Err(invalid("projected bounds must be finite"));
            }
            bounds[0][axis] = bounds[0][axis].min(position);
            bounds[1][axis] = bounds[1][axis].max(position);
        }
        visible = true;
        Ok(())
    };

    let mut previous = vertices[3];
    for current in vertices {
        if (previous[2] >= near) != (current[2] >= near) {
            let depth_delta = current[2] - previous[2];
            if !depth_delta.is_finite() {
                return Err(invalid("clipped edge depth must be finite"));
            }
            let fraction = (near - previous[2]) / depth_delta;
            include([
                previous[0] + fraction * (current[0] - previous[0]),
                previous[1] + fraction * (current[1] - previous[1]),
                near,
            ])?;
        }
        if current[2] >= near {
            include(current)?;
        }
        previous = current;
    }
    Ok(if visible { bounds } else { [center, center] })
}
