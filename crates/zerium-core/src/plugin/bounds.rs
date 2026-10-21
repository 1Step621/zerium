//! Rhai programs returning logical output rectangles.
use rhai::{Array, Dynamic, Engine, Map, Scope};
use serde::Deserialize;
use std::collections::BTreeMap;

use super::PluginError;
use crate::{
    property::{PropertySchema, PropertyValues},
    script::{Script, number, to_dynamic},
};

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OutputBoundsSchema {
    pub script: Script,
}

impl OutputBoundsSchema {
    pub fn evaluate(&self, scope: &mut Scope<'_>) -> Result<[[f64; 2]; 2], String> {
        let value = self.script.evaluate(scope)?;
        let bounds = value
            .try_cast::<Map>()
            .ok_or("Bounds must return a map with min and max arrays")?;
        let min = numbers::<2>(bounds.get("min").ok_or("Bounds must contain min")?)?;
        let max = numbers::<2>(bounds.get("max").ok_or("Bounds must contain max")?)?;
        if (0..2).any(|axis| min[axis] > max[axis]) {
            return Err("Bounds minimum must not exceed maximum".to_owned());
        }
        Ok([min, max])
    }

    pub(super) fn validate(
        &self,
        owner: &str,
        id: &str,
        properties: &[PropertySchema],
        inputs: &[super::TextureInput],
        overrides: &BTreeMap<String, f64>,
    ) -> Result<(), PluginError> {
        let defaults = PropertyValues::from_properties(properties);
        let mut rectangles = ["input", "source", "viewport"]
            .into_iter()
            .map(|name| (name.to_owned(), [-960., -540.], [960., 540.]))
            .collect::<Vec<_>>();
        rectangles.extend(
            inputs
                .iter()
                .map(|input| (format!("images::{}", input.id()), [-1., -1.], [1., 1.])),
        );
        let mut scope = program_context(
            rectangles
                .iter()
                .map(|(name, min, max)| (name.as_str(), *min, *max)),
            &defaults,
            overrides,
        );
        self.evaluate(&mut scope).map(|_| ()).map_err(|error| {
            PluginError::invalid_definition(format!(
                "{owner} '{id}' bounds program is invalid: {error}"
            ))
        })
    }
}

fn point(x: f64, y: f64) -> Dynamic {
    Dynamic::from_map(
        [
            ("x".into(), Dynamic::from_float(x)),
            ("y".into(), Dynamic::from_float(y)),
        ]
        .into_iter()
        .collect(),
    )
}

pub fn program_context<'a>(
    rectangles: impl IntoIterator<Item = (&'a str, [f64; 2], [f64; 2])>,
    values: &PropertyValues,
    overrides: &BTreeMap<String, f64>,
) -> Scope<'static> {
    let mut scope = Scope::new();
    let mut images = Map::new();
    for (name, min, max) in rectangles {
        let rectangle = Dynamic::from_map(
            [
                ("min".into(), point(min[0], min[1])),
                ("max".into(), point(max[0], max[1])),
                (
                    "center".into(),
                    point((min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5),
                ),
                ("size".into(), point(max[0] - min[0], max[1] - min[1])),
            ]
            .into_iter()
            .collect(),
        );
        if let Some(id) = name.strip_prefix("images::") {
            images.insert(id.into(), rectangle);
        } else {
            scope.push_constant_dynamic(name.to_owned(), rectangle);
        }
    }
    scope.push_constant("images", images);
    scope.push_constant(
        "p",
        values
            .iter()
            .map(|(id, value)| (id.into(), to_dynamic(value)))
            .collect::<Map>(),
    );
    scope.push_constant(
        "o",
        overrides
            .iter()
            .map(|(id, value)| (id.as_str().into(), Dynamic::from_float(*value)))
            .collect::<Map>(),
    );
    scope
}

pub(crate) fn register_functions(engine: &mut Engine) {
    engine.register_fn(
        "projected_rect",
        |min: Dynamic,
         max: Dynamic,
         rotation: Dynamic,
         center: Dynamic,
         focal: Dynamic,
         near: Dynamic|
         -> Result<Map, Box<rhai::EvalAltResult>> {
            projected_rect(&[min, max, rotation, center, focal, near])
                .map(|[min, max]| {
                    [
                        (
                            "min".into(),
                            Dynamic::from_array(min.into_iter().map(Dynamic::from_float).collect()),
                        ),
                        (
                            "max".into(),
                            Dynamic::from_array(max.into_iter().map(Dynamic::from_float).collect()),
                        ),
                    ]
                    .into_iter()
                    .collect()
                })
                .map_err(Into::into)
        },
    );
    engine.register_fn(
        "placement_center",
        |position: Dynamic,
         size: Dynamic,
         origin: Dynamic|
         -> Result<f64, Box<rhai::EvalAltResult>> {
            let position = number(&position)?;
            let size = number(&size)?;
            let origin = number(&origin)?;
            if !(0. ..=2.).contains(&origin) || origin.fract() != 0. {
                return Err("placement origin must be an integer in 0..=2".into());
            }
            Ok(position + (1. - origin) * size * 0.5)
        },
    );
}

fn invalid(message: &str) -> String {
    format!("projected rectangle: {message}")
}

fn numbers<const N: usize>(value: &Dynamic) -> Result<[f64; N], String> {
    let values = value
        .clone()
        .try_cast::<Array>()
        .ok_or("Expected an array")?;
    if values.len() != N {
        return Err(format!("Expected {N} components"));
    }
    let mut result = [0.; N];
    for (output, input) in result.iter_mut().zip(&values) {
        *output = number(input)?;
    }
    Ok(result)
}

fn projected_rect(arguments: &[Dynamic; 6]) -> Result<[[f64; 2]; 2], String> {
    let min: [f64; 2] = numbers(&arguments[0])?;
    let max: [f64; 2] = numbers(&arguments[1])?;
    let rotation: [f64; 3] = numbers(&arguments[2])?;
    let center: [f64; 2] = numbers(&arguments[3])?;
    let focal = crate::script::number(&arguments[4])?;
    let near_ratio = crate::script::number(&arguments[5])?;
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
    let mut include = |vertex: [f64; 3]| -> Result<(), String> {
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
