//! Shared, bounded Rhai runtime for scene calculations and plugin geometry.
use std::{
    fmt,
    sync::{Arc, OnceLock},
};

use rhai::{AST, Array, Dynamic, Engine, ImmutableString, Scope};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::property::{PropertyValue, ScalarPropertyType};

fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut engine = Engine::new();
        engine.set_max_operations(50_000);
        engine.set_max_expr_depths(64, 64);
        engine.set_max_call_levels(32);
        engine.set_max_array_size(4096);
        engine.set_max_map_size(4096);
        engine.set_max_string_size(16_384);
        engine.disable_symbol("eval");
        engine.on_print(|_| {});
        engine.on_debug(|_, _, _| {});
        engine.register_fn("exp2", |x: f64| x.exp2());
        engine.register_fn("pow", |x: f64, y: f64| x.powf(y));
        engine.register_fn("lerp", |a: f64, b: f64, t: f64| a + (b - a) * t);
        engine.register_fn("clamp", |x: f64, a: f64, b: f64| x.max(a).min(b));
        engine.register_fn("rgba", |r: f64, g: f64, b: f64, a: f64| -> Array {
            [r, g, b, a].into_iter().map(Dynamic::from_float).collect()
        });
        crate::plugin::register_script_functions(&mut engine);
        engine
    })
}

struct CompiledScript {
    source: String,
    ast: Result<AST, String>,
}

/// Source is persisted; compiled code is shared by snapshots and undo history.
/// Invalid code remains editable and produces diagnostics instead of panicking.
#[derive(Clone)]
pub struct Script(Arc<CompiledScript>);

impl Script {
    pub fn new(source: impl Into<String>) -> Self {
        let source = source.into();
        let ast = engine().compile(&source).map_err(|error| error.to_string());
        Self(Arc::new(CompiledScript { source, ast }))
    }

    pub fn source(&self) -> &str {
        &self.0.source
    }

    pub fn validate(&self) -> Result<(), String> {
        self.0.ast.as_ref().map(|_| ()).map_err(Clone::clone)
    }

    pub fn evaluate(&self, scope: &mut Scope<'_>) -> Result<Dynamic, String> {
        let ast = self.0.ast.as_ref().map_err(Clone::clone)?;
        engine()
            .eval_ast_with_scope(scope, ast)
            .map_err(|error| error.to_string())
    }
}

impl fmt::Debug for Script {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source().fmt(f)
    }
}
impl PartialEq for Script {
    fn eq(&self, other: &Self) -> bool {
        self.source() == other.source()
    }
}
impl Serialize for Script {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.source())
    }
}
impl<'de> Deserialize<'de> for Script {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::new)
    }
}

pub fn to_dynamic(value: &PropertyValue) -> Dynamic {
    match value {
        PropertyValue::F32(x) => Dynamic::from_float(f64::from(*x)),
        PropertyValue::I32(x) => Dynamic::from_int(i64::from(*x)),
        PropertyValue::U32(x) | PropertyValue::Enum(x) => Dynamic::from_int(i64::from(*x)),
        PropertyValue::Bool(x) => Dynamic::from_bool(*x),
        PropertyValue::String(x) => Dynamic::from(x.clone()),
        PropertyValue::Color(x) => Dynamic::from_array(
            x.iter()
                .map(|x| Dynamic::from_float(f64::from(*x)))
                .collect(),
        ),
        PropertyValue::Tuple(x) => Dynamic::from_array(x.iter().map(to_dynamic).collect()),
        PropertyValue::Array(x) => {
            Dynamic::from_array(x.iter().map(|x| to_dynamic(x.value())).collect())
        }
        PropertyValue::File(Some(path)) => Dynamic::from(path.to_string_lossy().into_owned()),
        PropertyValue::File(None) => Dynamic::UNIT,
    }
}

pub fn number(value: &Dynamic) -> Result<f64, String> {
    let number = if value.is_int() {
        value.as_int().map(|x| x as f64)
    } else {
        value.as_float()
    }
    .map_err(|_| "Expected a number".to_owned())?;
    if number.is_finite() {
        Ok(number)
    } else {
        Err("Result must be finite".to_owned())
    }
}

pub(crate) fn from_dynamic(
    value: Dynamic,
    ty: &ScalarPropertyType,
) -> Result<PropertyValue, String> {
    let invalid = || format!("Result does not match output type {ty:?}");
    let result = match ty {
        ScalarPropertyType::F32 => {
            let x = number(&value)? as f32;
            if !x.is_finite() {
                return Err("Result is outside the finite f32 range".to_owned());
            }
            PropertyValue::F32(x)
        }
        ScalarPropertyType::I32 => PropertyValue::I32(
            value
                .as_int()
                .ok()
                .and_then(|x| i32::try_from(x).ok())
                .ok_or_else(invalid)?,
        ),
        ScalarPropertyType::U32 => PropertyValue::U32(
            value
                .as_int()
                .ok()
                .and_then(|x| u32::try_from(x).ok())
                .ok_or_else(invalid)?,
        ),
        ScalarPropertyType::Bool => PropertyValue::Bool(value.as_bool().map_err(|_| invalid())?),
        ScalarPropertyType::String => {
            let text = value
                .try_cast::<ImmutableString>()
                .ok_or_else(invalid)?
                .to_string();
            if text.len() > crate::property::MAX_STRING_BYTES {
                return Err(format!(
                    "Text result exceeds {} bytes",
                    crate::property::MAX_STRING_BYTES
                ));
            }
            PropertyValue::String(text)
        }
        ScalarPropertyType::Color => {
            let components = value.try_cast::<Array>().ok_or_else(invalid)?;
            if components.len() != 4 {
                return Err("Color result must contain four components".to_owned());
            }
            let mut color = [0.; 4];
            for (output, input) in color.iter_mut().zip(&components) {
                let x = number(input)? as f32;
                if !x.is_finite() {
                    return Err("Color components must be finite".to_owned());
                }
                *output = x;
            }
            PropertyValue::Color(color)
        }
        _ => return Err(invalid()),
    };
    Ok(result)
}
