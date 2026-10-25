//! Named, typed calculations with explicit, stable input connections.

use super::SceneDefinition;
use crate::{
    property::{PropertyValue, PropertyValues},
    script::{Script, from_dynamic, to_dynamic},
    timeline::{PropertyAddress, SceneId},
};
use rhai::Scope;
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex, OnceLock},
};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SceneCalculationInput {
    pub name: String,
    pub variable_id: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SceneCalculation {
    pub inputs: Vec<SceneCalculationInput>,
    pub script: Script,
}

/// Results are local to one scene instance and one sample. Failed calculations
/// have no value; their dependants fail too, while unrelated outputs still work.
#[derive(Default, Debug)]
pub struct SceneEvaluation {
    pub values: HashMap<String, Result<PropertyValue, CalculationFailure>>,
}

pub(crate) fn valid_input_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && Script::new(format!("let {name} = 0; {name}"))
            .validate()
            .is_ok()
}

/// Derived state belongs to an immutable scene revision. Clones share it until
/// a command obtains mutable variables, then detach without touching history.
#[derive(Default)]
pub(super) struct SceneRuntime {
    plan: OnceLock<Result<VariablePlan, String>>,
    pub(super) bindings: OnceLock<crate::timeline::model::scene::binding::BindingIndex>,
    defaults: OnceLock<Arc<SceneEvaluation>>,
    calculations: Mutex<HashMap<String, VecDeque<CalculationResult>>>,
}

struct VariablePlan {
    order: Vec<usize>,
    indices: HashMap<String, usize>,
}

struct CalculationResult {
    inputs: Vec<PropertyValue>,
    result: Result<PropertyValue, String>,
}

// Rhai can distinguish -0.0 from 0.0, so numeric cache keys compare bits.
fn same_value(left: &PropertyValue, right: &PropertyValue) -> bool {
    match (left, right) {
        (PropertyValue::F32(left), PropertyValue::F32(right)) => left.to_bits() == right.to_bits(),
        (PropertyValue::Color(left), PropertyValue::Color(right)) => left
            .iter()
            .zip(right)
            .all(|(left, right)| left.to_bits() == right.to_bits()),
        _ => left == right,
    }
}

/// One demand-driven evaluation scope. Cross-scene arguments are resolved only
/// when a connected output reads them; local results are shared by its consumers.
type ArgumentResolver<'a> = dyn Fn(&crate::timeline::model::scene::SceneVariable) -> Result<PropertyValue, CalculationFailure>
    + 'a;

pub(in crate::timeline) struct VariableEvaluator<'a> {
    scene: &'a SceneDefinition,
    argument: Box<ArgumentResolver<'a>>,
    values: RefCell<HashMap<String, Result<PropertyValue, CalculationFailure>>>,
}

impl<'a> VariableEvaluator<'a> {
    pub(in crate::timeline) fn new(
        scene: &'a SceneDefinition,
        argument: impl Fn(
            &crate::timeline::model::scene::SceneVariable,
        ) -> Result<PropertyValue, CalculationFailure>
        + 'a,
    ) -> Self {
        Self {
            scene,
            argument: Box::new(argument),
            values: RefCell::default(),
        }
    }

    pub(in crate::timeline) fn get(&self, id: &str) -> Result<PropertyValue, CalculationFailure> {
        if let Some(value) = self.values.borrow().get(id).cloned() {
            return value;
        }
        let plan = self
            .scene
            .variable_plan()
            .as_ref()
            .map_err(|error| CalculationFailure::InvalidGraph(error.clone()))?;
        let index = plan
            .indices
            .get(id)
            .ok_or_else(|| CalculationFailure::MissingVariable(id.to_owned()))?;
        let mut pending = vec![(*index, false)];
        while let Some((index, ready)) = pending.pop() {
            let variable = &self.scene.variables[index];
            if self.values.borrow().contains_key(variable.id()) {
                continue;
            }
            if !ready {
                pending.push((index, true));
                if let Some(calculation) = variable.calculation() {
                    for input in calculation.inputs.iter().rev() {
                        pending.push((plan.indices[&input.variable_id], false));
                    }
                }
                continue;
            }
            let result = if let Some(calculation) = variable.calculation() {
                let inputs = calculation
                    .inputs
                    .iter()
                    .map(|input| {
                        self.values
                            .borrow()
                            .get(&input.variable_id)
                            .expect("dependency was evaluated")
                            .clone()
                            .map_err(|cause| CalculationFailure::Input {
                                name: input.name.clone(),
                                variable_id: input.variable_id.clone(),
                                cause: Arc::new(cause),
                            })
                    })
                    .collect::<Result<Vec<_>, _>>();
                inputs.and_then(|inputs| {
                    self.scene
                        .runtime
                        .evaluate_calculation(variable, &inputs)
                        .map_err(CalculationFailure::Script)
                })
            } else {
                (self.argument)(variable).and_then(|value| {
                    variable.apply_argument_limits(&value).ok_or_else(|| {
                        CalculationFailure::InvalidArgument(variable.id().to_owned())
                    })
                })
            };
            self.values
                .borrow_mut()
                .insert(variable.id().to_owned(), result);
        }
        self.values
            .borrow()
            .get(id)
            .expect("requested output was evaluated")
            .clone()
    }
}

impl SceneDefinition {
    /// UI order does not affect dependency order. This validation is shared by
    /// editing and loading; script errors remain saveable while being authored.
    pub fn validate_calculations(&self) -> Result<(), String> {
        self.variable_plan()
            .as_ref()
            .map(|_| ())
            .map_err(Clone::clone)
    }

    fn variable_plan(&self) -> &Result<VariablePlan, String> {
        self.runtime
            .plan
            .get_or_init(|| self.compile_variable_plan())
    }

    fn compile_variable_plan(&self) -> Result<VariablePlan, String> {
        let mut ids = HashSet::new();
        for variable in &self.variables {
            variable.validate_argument()?;
            if variable.id().trim().is_empty() || variable.label().trim().is_empty() {
                return Err("Scene variables must have an ID and name".to_owned());
            }
            if !ids.insert(variable.id()) {
                return Err("Duplicate scene variable ID".to_owned());
            }
            if let Some(calculation) = variable.calculation() {
                use crate::property::ScalarPropertyType;
                if !matches!(
                    variable.ty(),
                    ScalarPropertyType::F32
                        | ScalarPropertyType::I32
                        | ScalarPropertyType::U32
                        | ScalarPropertyType::Bool
                        | ScalarPropertyType::Color
                        | ScalarPropertyType::String
                ) {
                    return Err("Unsupported calculation output type".to_owned());
                }
                let mut aliases = HashSet::new();
                for input in &calculation.inputs {
                    if !valid_input_name(&input.name) || !aliases.insert(&input.name) {
                        return Err(format!("Invalid or duplicate input name '{}'", input.name));
                    }
                    if self.variable(&input.variable_id).is_none() {
                        return Err(format!(
                            "Input '{}' references a missing variable",
                            input.name
                        ));
                    }
                }
            }
        }
        let (order, pending) = self.calculation_order();
        if pending.is_empty() {
            Ok(VariablePlan {
                order,
                indices: self
                    .variables
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (v.id().to_owned(), i))
                    .collect(),
            })
        } else {
            Err("Calculation inputs contain a cycle".to_owned())
        }
    }

    fn calculation_order(&self) -> (Vec<usize>, Vec<usize>) {
        let by_id = self
            .variables
            .iter()
            .enumerate()
            .map(|(i, v)| (v.id(), i))
            .collect::<HashMap<_, _>>();
        let mut remaining = vec![0; self.variables.len()];
        let mut dependants = vec![Vec::new(); self.variables.len()];
        for (index, variable) in self.variables.iter().enumerate() {
            if let Some(calculation) = variable.calculation() {
                for input in &calculation.inputs {
                    if let Some(&dependency) = by_id.get(input.variable_id.as_str()) {
                        remaining[index] += 1;
                        dependants[dependency].push(index);
                    }
                }
            }
        }
        let mut ready = remaining
            .iter()
            .enumerate()
            .filter_map(|(i, &n)| (n == 0).then_some(i))
            .collect::<VecDeque<_>>();
        let mut order = Vec::with_capacity(self.variables.len());
        while let Some(index) = ready.pop_front() {
            order.push(index);
            for &dependent in &dependants[index] {
                remaining[dependent] -= 1;
                if remaining[dependent] == 0 {
                    ready.push_back(dependent);
                }
            }
        }
        let pending = remaining
            .iter()
            .enumerate()
            .filter_map(|(i, &n)| (n != 0).then_some(i))
            .collect();
        (order, pending)
    }

    /// Copy the dependency graph required by selected items into a new scene.
    pub(in crate::timeline) fn variables_for_grouping(
        &self,
        selected: &HashSet<crate::timeline::ItemId>,
    ) -> Vec<crate::timeline::model::scene::SceneVariable> {
        let mut required = self
            .variables
            .iter()
            .filter(|variable| {
                variable
                    .bindings
                    .iter()
                    .any(|binding| selected.contains(&binding.item_id()))
            })
            .map(|variable| variable.id().to_owned())
            .collect::<HashSet<_>>();
        let mut pending = required.iter().cloned().collect::<Vec<_>>();
        while let Some(id) = pending.pop() {
            if let Some(calculation) = self
                .variable(&id)
                .and_then(|variable| variable.calculation())
            {
                for input in &calculation.inputs {
                    if required.insert(input.variable_id.clone()) {
                        pending.push(input.variable_id.clone());
                    }
                }
            }
        }
        self.variables
            .iter()
            .filter(|variable| required.contains(variable.id()))
            .cloned()
            .map(|mut variable| {
                variable
                    .bindings
                    .retain(|binding| selected.contains(&binding.item_id()));
                variable
            })
            .collect()
    }

    pub(in crate::timeline) fn variable_evaluator<'a>(
        &'a self,
        inputs: Option<&'a PropertyValues>,
    ) -> VariableEvaluator<'a> {
        VariableEvaluator::new(self, move |variable| {
            let schema = variable
                .argument_schema()
                .expect("argument source owns its schema");
            Ok(schema.resolve_value(inputs.and_then(|values| values.property(variable.id()))))
        })
    }

    /// Scene settings display every output using the definition's defaults.
    /// Placed instances use demand-driven evaluation and per-calculation caches.
    pub fn evaluate_default_variables(&self) -> Arc<SceneEvaluation> {
        self.runtime
            .defaults
            .get_or_init(|| Arc::new(self.evaluate_all_default_variables()))
            .clone()
    }

    fn evaluate_all_default_variables(&self) -> SceneEvaluation {
        let evaluation = self.variable_evaluator(None);
        // Visiting in dependency order keeps the explicit "evaluate all" UI path
        // iterative for long chains. Ordinary property readers request one output.
        let order = match self.variable_plan() {
            Ok(plan) => plan.order.clone(),
            Err(_) => (0..self.variables.len()).collect(),
        };
        SceneEvaluation {
            values: order
                .into_iter()
                .map(|i| {
                    let id = self.variables[i].id();
                    (id.to_owned(), evaluation.get(id))
                })
                .collect(),
        }
    }
}

impl SceneRuntime {
    fn evaluate_calculation(
        &self,
        variable: &crate::timeline::model::scene::SceneVariable,
        inputs: &[PropertyValue],
    ) -> Result<PropertyValue, String> {
        {
            let cached = self
                .calculations
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let Some(previous) = cached.get(variable.id()).and_then(|cached| {
                cached.iter().find(|previous| {
                    previous.inputs.len() == inputs.len()
                        && previous
                            .inputs
                            .iter()
                            .zip(inputs)
                            .all(|(a, b)| same_value(a, b))
                })
            }) {
                return previous.result.clone();
            }
        }
        let calculation = variable.calculation().expect("calculation source");
        let mut scope = Scope::new();
        for (input, value) in calculation.inputs.iter().zip(inputs) {
            scope.push_constant_dynamic(input.name.clone(), to_dynamic(value));
        }
        // Execute outside the cache lock: audio, rendering and UI readers do not
        // serialize behind a script that another consumer is currently running.
        let result = calculation
            .script
            .evaluate(&mut scope)
            .and_then(|value| from_dynamic(value, variable.ty()));
        let mut all = self
            .calculations
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let cached = all.entry(variable.id().to_owned()).or_default();
        if cached.len() == 8 {
            cached.pop_front();
        }
        cached.push_back(CalculationResult {
            inputs: inputs.to_vec(),
            result: result.clone(),
        });
        result
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum CalculationFailure {
    #[error("{0}")]
    InvalidGraph(String),
    #[error("Missing variable '{0}'")]
    MissingVariable(String),
    #[error("Missing instance argument '{0}'")]
    MissingArgument(String),
    #[error("Invalid argument '{0}'")]
    InvalidArgument(String),
    #[error("{0}")]
    Script(String),
    #[error("Input '{name}': {cause}")]
    Input {
        name: String,
        variable_id: String,
        #[source]
        cause: Arc<CalculationFailure>,
    },
    #[error(transparent)]
    Argument(#[from] Arc<CalculationDiagnostic>),
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("Scene '{scene_name}' variable '{variable_label}' (item {item}): {cause}", item = address.item_id.get())]
pub struct CalculationDiagnostic {
    pub scene_id: SceneId,
    pub scene_name: String,
    pub variable_id: String,
    pub variable_label: String,
    pub address: PropertyAddress,
    #[source]
    pub cause: CalculationFailure,
}
