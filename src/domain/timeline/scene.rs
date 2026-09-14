use std::collections::{HashMap, HashSet};

use crate::domain::animation::{AnimationChannel, ParameterAnimations};
use crate::domain::parameter::materialized_parameter_values;
use crate::domain::parameter::{
    ArrayElement, ParameterAnimatable, ParameterEditable, ParameterSchema, ParameterType,
    ParameterValue, ParameterValueType, ParameterValues, ScalarParameterType,
};

use super::{
    document::TimelineDocument,
    expression,
    ids::{EffectInstanceId, ItemId, LayerId, SceneId},
    item::{TimelineItem, TimelineItemKind},
    time::{Frame, FrameDuration, FrameRate},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ParameterOwner {
    Item,
    Effect(EffectInstanceId),
}

impl ParameterOwner {
    pub(crate) fn from_effect(effect_id: Option<EffectInstanceId>) -> Self {
        effect_id.map_or(Self::Item, Self::Effect)
    }

    pub(crate) const fn effect_id(self) -> Option<EffectInstanceId> {
        match self {
            Self::Item => None,
            Self::Effect(effect_id) => Some(effect_id),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ParameterValuePath {
    Whole,
    ArrayElement(usize),
    TupleElement(usize),
    ArrayTupleElement { array: usize, tuple: usize },
}

impl ParameterValuePath {
    pub(crate) fn from_elements(
        array_element: Option<usize>,
        tuple_element: Option<usize>,
    ) -> Self {
        match (array_element, tuple_element) {
            (None, None) => Self::Whole,
            (Some(element), None) => Self::ArrayElement(element),
            (None, Some(element)) => Self::TupleElement(element),
            (Some(array), Some(tuple)) => Self::ArrayTupleElement { array, tuple },
        }
    }

    pub(crate) const fn array_element(self) -> Option<usize> {
        match self {
            Self::Whole | Self::TupleElement(_) => None,
            Self::ArrayElement(element) => Some(element),
            Self::ArrayTupleElement { array, .. } => Some(array),
        }
    }

    pub(crate) const fn tuple_element(self) -> Option<usize> {
        match self {
            Self::Whole | Self::ArrayElement(_) => None,
            Self::TupleElement(element) => Some(element),
            Self::ArrayTupleElement { tuple, .. } => Some(tuple),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ParameterAddress {
    owner: ParameterOwner,
    parameter_id: String,
    value_path: ParameterValuePath,
}

impl ParameterAddress {
    fn new(
        owner: ParameterOwner,
        parameter_id: impl Into<String>,
        value_path: ParameterValuePath,
    ) -> Self {
        Self {
            owner,
            parameter_id: parameter_id.into(),
            value_path,
        }
    }
}

pub(crate) type SceneBindingOwner = ParameterOwner;
pub(crate) type SceneBindingValuePath = ParameterValuePath;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SceneBindingTarget {
    item_id: ItemId,
    address: ParameterAddress,
}

impl SceneBindingTarget {
    pub(crate) fn new(
        item_id: ItemId,
        owner: SceneBindingOwner,
        parameter_id: impl Into<String>,
        value_path: SceneBindingValuePath,
    ) -> Self {
        Self {
            item_id,
            address: ParameterAddress::new(owner, parameter_id, value_path),
        }
    }

    pub(crate) const fn item_id(&self) -> ItemId {
        self.item_id
    }

    pub(crate) const fn owner(&self) -> SceneBindingOwner {
        self.address.owner
    }

    pub(crate) fn parameter_id(&self) -> &str {
        &self.address.parameter_id
    }

    pub(crate) const fn value_path(&self) -> SceneBindingValuePath {
        self.address.value_path
    }

    pub(crate) fn addresses(
        &self,
        item_id: ItemId,
        effect_id: Option<EffectInstanceId>,
        parameter_id: &str,
    ) -> bool {
        self.item_id == item_id
            && self.address.owner.effect_id() == effect_id
            && self.address.parameter_id == parameter_id
    }

    pub(crate) fn conflicts_with_aspect_ratio_lock(
        &self,
        item: &TimelineItem,
        locked: bool,
    ) -> bool {
        locked
            && self.owner() == SceneBindingOwner::Item
            && self.value_path().tuple_element() == Some(1)
            && item
                .schema()
                .is_some_and(|schema| schema.is_size_parameter(self.parameter_id()))
    }

    pub(crate) fn conflicts_with_animation(
        &self,
        address: &crate::domain::animation::ParameterAnimationAddress,
        array_index: Option<usize>,
    ) -> bool {
        if self.address.value_path.array_element() != array_index {
            return false;
        }
        match (self.address.value_path.tuple_element(), address.channel) {
            (None, _) | (_, AnimationChannel::Scalar) => true,
            (Some(bound), AnimationChannel::TupleElement(animated)) => bound == animated,
        }
    }
}

pub(crate) fn project_scene_binding_value(
    schema: &ParameterSchema,
    value: &ParameterValue,
    value_path: SceneBindingValuePath,
) -> Option<(ParameterSchema, ParameterValue)> {
    let array_element = value_path.array_element();
    let tuple_element = value_path.tuple_element();
    let mut schema = schema.clone();
    let mut value = value.clone();

    if let Some(index) = array_element {
        let ParameterType::Array { element, .. } = schema.ty else {
            return None;
        };
        let ParameterValue::Array(values) = value else {
            return None;
        };
        value = values.get(index)?.value().clone();
        schema.ty = ParameterType::Value(element);
    } else if matches!(schema.ty, ParameterType::Array { .. }) {
        return None;
    }

    if let Some(element) = tuple_element {
        let ParameterValueType::Tuple(tuple) = schema.ty.value_type()? else {
            return None;
        };
        let ParameterValue::Tuple(values) = value else {
            return None;
        };
        value = values.get(element)?.clone();
        schema.ty = ParameterType::Value(ParameterValueType::Scalar(
            tuple.elements().get(element)?.clone(),
        ));
        schema.constraints = schema.constraints.for_element(element).clone();
    } else {
        if !matches!(schema.ty.value_type()?, ParameterValueType::Scalar(_)) {
            return None;
        }
    }

    schema.ui = schema.ui.to_scalar(tuple_element);

    if let Some(element) = tuple_element {
        schema.animatable = schema.animatable.to_scalar(element);
        schema.editable = schema.editable.to_scalar(element);
    }
    value = schema.constrained_value(&value)?;
    schema.default = value.clone();
    Some((schema, value))
}

pub(crate) fn apply_scene_binding_value(
    current: &ParameterValue,
    value_path: SceneBindingValuePath,
    value: ParameterValue,
) -> Option<ParameterValue> {
    let array_element = value_path.array_element();
    let tuple_element = value_path.tuple_element();
    let replace_tuple_element = |current: &ParameterValue| {
        let Some(element) = tuple_element else {
            return Some(value.clone());
        };
        let ParameterValue::Tuple(values) = current else {
            return None;
        };
        let mut values = values.clone();
        *values.get_mut(element)? = value.clone();
        Some(ParameterValue::Tuple(values))
    };

    let Some(element) = array_element else {
        return replace_tuple_element(current);
    };
    let ParameterValue::Array(values) = current else {
        return None;
    };
    let mut values = values.clone();
    let target = values.get_mut(element)?;
    *target.value_mut() = replace_tuple_element(target.value())?;
    Some(ParameterValue::Array(values))
}

pub(crate) fn scene_binding_is_animated(
    animations: &ParameterAnimations,
    parameter_id: &str,
    value: &ParameterValue,
    value_path: SceneBindingValuePath,
) -> bool {
    let array_element_id = match value_path.array_element() {
        Some(index) => match value {
            ParameterValue::Array(elements) => elements.get(index).map(ArrayElement::id),
            _ => return false,
        },
        None => None,
    };
    let tuple_element = value_path.tuple_element();
    match tuple_element {
        Some(element) => {
            animations.contains(&crate::domain::animation::ParameterAnimationAddress::new(
                parameter_id,
                array_element_id,
                AnimationChannel::TupleElement(element),
            ))
        }
        None => animations.contains(&crate::domain::animation::ParameterAnimationAddress::new(
            parameter_id,
            array_element_id,
            AnimationChannel::Scalar,
        )),
    }
}

/// A binding target resolved against one concrete scene item.
///
/// Keeping this resolution here gives editing, persistence validation, and
/// evaluation one definition of what a valid scene binding points at.
pub(crate) struct ResolvedSceneBinding {
    pub(crate) schema: ParameterSchema,
    pub(crate) animated: bool,
}

pub(crate) fn resolve_parameter_schema<'a>(
    scenes: &'a HashMap<SceneId, SceneDefinition>,
    item: &'a TimelineItem,
    owner: ParameterOwner,
    parameter_id: &str,
) -> Option<&'a ParameterSchema> {
    match owner {
        ParameterOwner::Effect(effect_id) => item
            .effects
            .iter()
            .find(|effect| effect.id == effect_id)?
            .schema()
            .parameter(parameter_id),
        ParameterOwner::Item => item
            .schema()
            .and_then(|schema| schema.parameter(parameter_id))
            .or_else(|| {
                scenes
                    .get(&item.scene_id()?)?
                    .input_argument(parameter_id)
                    .map(|argument| argument.schema.parameter())
            }),
    }
}

pub(crate) fn resolve_scene_binding(
    scenes: &HashMap<SceneId, SceneDefinition>,
    scene: &SceneDefinition,
    target: &SceneBindingTarget,
) -> Option<ResolvedSceneBinding> {
    let item = scene.document().item(target.item_id())?;
    let parameter_id = target.parameter_id();
    let schema = resolve_parameter_schema(scenes, item, target.owner(), parameter_id)?;
    let (value, animations) = match target.owner() {
        SceneBindingOwner::Effect(effect_id) => {
            let effect = item.effects.iter().find(|effect| effect.id == effect_id)?;
            (effect.parameters.get(parameter_id)?, &effect.animations)
        }
        SceneBindingOwner::Item => {
            let value = item.parameters.get(parameter_id).or_else(|| {
                let nested = scenes.get(&item.scene_id()?)?;
                nested
                    .input_argument(parameter_id)
                    .map(|argument| argument.schema.default_value())
            })?;
            (value, &item.animations)
        }
    };
    let animated = scene_binding_is_animated(animations, parameter_id, value, target.value_path());
    let (schema, _) = project_scene_binding_value(schema, value, target.value_path())?;
    Some(ResolvedSceneBinding { schema, animated })
}

pub(crate) fn apply_scene_binding_to_item(
    item: &mut TimelineItem,
    target: &SceneBindingTarget,
    binding_schema: &ParameterSchema,
    value: ParameterValue,
) -> Option<bool> {
    let value = binding_schema.constrained_value(&value)?;
    let parameter_id = target.parameter_id();
    match target.owner() {
        SceneBindingOwner::Effect(effect_id) => item
            .effects
            .iter_mut()
            .find(|effect| effect.id == effect_id)
            .and_then(|effect| {
                let effect_schema = effect.schema.clone();
                let target_schema = effect_schema.parameter(parameter_id)?;
                let current = effect.parameters.get(parameter_id)?;
                let value = apply_scene_binding_value(current, target.value_path(), value)?;
                effect.parameters.set(target_schema, value).ok()
            }),
        SceneBindingOwner::Item => {
            let item_schema = item.schema_arc().cloned();
            let target_schema = item_schema
                .as_deref()
                .and_then(|schema| schema.parameter(parameter_id))
                .unwrap_or(binding_schema);
            let current = item.parameters.get(parameter_id)?;
            let value = apply_scene_binding_value(current, target.value_path(), value)?;
            item.parameters.set(target_schema, value).ok()
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SceneArgumentSchema {
    parameter: ParameterSchema,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SceneArgumentPreset {
    Number,
    SignedInteger,
    UnsignedInteger,
    Boolean,
    Color,
    Text,
}

impl SceneArgumentPreset {
    pub(crate) const fn scalar(self) -> ScalarParameterType {
        match self {
            Self::Number => ScalarParameterType::F32,
            Self::SignedInteger => ScalarParameterType::I32,
            Self::UnsignedInteger => ScalarParameterType::U32,
            Self::Boolean => ScalarParameterType::Bool,
            Self::Color => ScalarParameterType::Color,
            Self::Text => ScalarParameterType::String,
        }
    }
}

impl SceneArgumentSchema {
    pub(crate) fn from_parameter(mut parameter: ParameterSchema) -> Option<Self> {
        if !matches!(
            parameter.ty,
            ParameterType::Value(ParameterValueType::Scalar(_))
        ) {
            return None;
        }
        let default = parameter.default_value().clone();
        parameter.default = parameter.constrained_value(&default)?;
        // A scene argument is its own editable input contract. The source
        // parameter's editability only controls direct edits on the bound
        // plugin parameter.
        parameter.editable = ParameterEditable::Scalar(true);
        parameter.scene_bindable = true;
        Some(Self { parameter })
    }

    pub(crate) fn parameter(&self) -> &ParameterSchema {
        &self.parameter
    }

    pub(crate) fn id(&self) -> &str {
        self.parameter.id()
    }

    pub(crate) fn label(&self) -> &str {
        self.parameter.label()
    }

    pub(crate) fn ty(&self) -> &ParameterType {
        self.parameter.ty()
    }

    pub(crate) fn default_value(&self) -> &ParameterValue {
        self.parameter.default_value()
    }

    pub(crate) fn constrained_value(&self, value: &ParameterValue) -> Option<ParameterValue> {
        self.parameter.constrained_value(value)
    }

    pub(crate) fn rename(&mut self, label: String) {
        self.parameter.label = label;
    }

    pub(crate) fn with_default(&self, value: &ParameterValue) -> Option<Self> {
        let default = self.parameter.constrained_value(value)?;
        let mut next = self.clone();
        next.parameter.default = default;
        Some(next)
    }

    pub(crate) fn with_numeric_settings(
        &self,
        settings: crate::domain::parameter::NumericSettings,
    ) -> Option<Self> {
        let (default, constraints) = settings.into_parts();
        if !self.parameter.ty().allows(&default) {
            return None;
        }
        let mut next = self.clone();
        next.parameter.default = default;
        next.parameter.constraints = constraints;
        Some(next)
    }

    pub(crate) fn with_identity(mut self, id: String, label: String) -> Self {
        self.parameter.id = id;
        self.parameter.label = label;
        self
    }

    fn into_expression(mut self) -> Option<Self> {
        if self.ty() != &ParameterType::Value(ParameterValueType::Scalar(ScalarParameterType::F32))
        {
            return None;
        }
        self.parameter.editable = ParameterEditable::Scalar(false);
        self.parameter.animatable = ParameterAnimatable::Scalar(false);
        self.parameter.scene_bindable = false;
        Some(self)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SceneArgument {
    pub(crate) schema: SceneArgumentSchema,
    pub(crate) bindings: Vec<SceneBindingTarget>,
    expression: Option<expression::CompiledExpression>,
}

impl SceneArgument {
    pub(crate) fn input(schema: SceneArgumentSchema, bindings: Vec<SceneBindingTarget>) -> Self {
        Self {
            schema,
            bindings,
            expression: None,
        }
    }

    pub(crate) fn computed(
        schema: SceneArgumentSchema,
        bindings: Vec<SceneBindingTarget>,
        expression: String,
    ) -> Option<Self> {
        Some(Self {
            schema: schema.into_expression()?,
            bindings,
            expression: Some(expression::CompiledExpression::compile(expression)?),
        })
    }

    pub(crate) fn expression(&self) -> Option<&str> {
        self.expression
            .as_ref()
            .map(expression::CompiledExpression::source)
    }

    pub(crate) fn set_expression(&mut self, expression: String) -> bool {
        let Some(current) = &mut self.expression else {
            return false;
        };
        let Some(expression) = expression::CompiledExpression::compile(expression) else {
            return false;
        };
        *current = expression;
        true
    }

    pub(crate) fn expression_references(&self, argument_id: &str) -> bool {
        self.expression
            .as_ref()
            .is_some_and(|expression| expression.dependencies().contains(argument_id))
    }

    pub(super) fn evaluate_expression(&self, values: &HashMap<String, f32>) -> Option<f32> {
        self.expression.as_ref()?.evaluate(values)
    }

    fn expression_dependencies(&self) -> Option<&HashSet<String>> {
        self.expression
            .as_ref()
            .map(expression::CompiledExpression::dependencies)
    }
}

pub(super) fn unique_scene_argument_name(
    arguments: &[SceneArgument],
    preferred: &str,
    fallback: &str,
) -> String {
    let mut normalized = String::new();
    for character in preferred.trim().chars() {
        if character == '_' || character.is_alphanumeric() {
            normalized.push(character);
        } else if !normalized.ends_with('_') {
            normalized.push('_');
        }
    }
    if normalized
        .chars()
        .next()
        .is_some_and(|character| character.is_numeric())
    {
        normalized.insert(0, '_');
    }
    while normalized.ends_with('_') {
        normalized.pop();
    }
    if !expression::valid_variable_name(&normalized) {
        normalized = fallback.to_owned();
    }
    let normalized_is_duplicate = |candidate: &str| {
        arguments.iter().any(|argument| {
            argument.schema.label() == candidate
                || expression::variable_name_for_label(argument.schema.label())
                    == Some(candidate.to_owned())
        })
    };
    if !normalized_is_duplicate(&normalized) {
        return normalized;
    }
    (2_u64..)
        .map(|suffix| format!("{normalized}_{suffix}"))
        .find(|candidate| !normalized_is_duplicate(candidate))
        .expect("a scene argument name suffix must eventually be available")
}

pub(crate) fn scene_argument_expressions_valid(arguments: &[SceneArgument]) -> bool {
    if arguments.iter().any(|argument| {
        argument.expression().is_some()
            && argument.schema.ty()
                != &ParameterType::Value(ParameterValueType::Scalar(ScalarParameterType::F32))
    }) {
        return false;
    }
    let numeric_names = arguments
        .iter()
        .filter(|argument| {
            argument.schema.ty()
                == &ParameterType::Value(ParameterValueType::Scalar(ScalarParameterType::F32))
        })
        .map(|argument| argument.schema.id().to_owned())
        .collect::<HashSet<_>>();
    let dependencies = arguments
        .iter()
        .filter_map(|argument| {
            let dependencies = argument.expression_dependencies()?.clone();
            dependencies
                .iter()
                .all(|dependency| numeric_names.contains(dependency))
                .then_some((argument.schema.id().to_owned(), dependencies))
        })
        .collect::<HashMap<_, _>>();
    if dependencies.len()
        != arguments
            .iter()
            .filter(|argument| argument.expression().is_some())
            .count()
    {
        return false;
    }

    fn visit(
        id: &str,
        dependencies: &HashMap<String, HashSet<String>>,
        visiting: &mut HashSet<String>,
        visited: &mut HashSet<String>,
    ) -> bool {
        if visited.contains(id) {
            return true;
        }
        if !visiting.insert(id.to_owned()) {
            return false;
        }
        if let Some(argument_dependencies) = dependencies.get(id) {
            for dependency in argument_dependencies {
                if dependencies.contains_key(dependency.as_str())
                    && !visit(dependency, dependencies, visiting, visited)
                {
                    return false;
                }
            }
        }
        visiting.remove(id);
        visited.insert(id.to_owned());
        true
    }

    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    dependencies
        .keys()
        .all(|id| visit(id, &dependencies, &mut visiting, &mut visited))
}

#[derive(Clone)]
pub(crate) struct SceneDefinition {
    pub id: SceneId,
    pub name: String,
    pub(crate) arguments: Vec<SceneArgument>,
    document: TimelineDocument,
    next_argument_id: u64,
}

impl SceneDefinition {
    pub(super) fn new(
        id: SceneId,
        name: String,
        frame_rate: FrameRate,
        items: Vec<(LayerId, TimelineItem)>,
    ) -> Self {
        Self {
            id,
            name,
            arguments: Vec::new(),
            document: TimelineDocument::from_items(frame_rate, items),
            next_argument_id: 1,
        }
    }

    pub(crate) fn duration(&self) -> FrameDuration {
        FrameDuration::new_saturating(
            self.document
                .items()
                .map(TimelineItem::end_exclusive)
                .max()
                .unwrap_or(Frame::new(1))
                .get()
                .max(1),
        )
    }

    pub(crate) fn items(&self) -> impl Iterator<Item = &TimelineItem> {
        self.document.items()
    }

    pub(crate) fn input_arguments(&self) -> impl Iterator<Item = &SceneArgument> {
        self.arguments
            .iter()
            .filter(|argument| argument.expression().is_none())
    }

    pub(crate) fn computed_arguments(&self) -> impl Iterator<Item = &SceneArgument> {
        self.arguments
            .iter()
            .filter(|argument| argument.expression().is_some())
    }

    pub(crate) fn input_argument(&self, id: &str) -> Option<&SceneArgument> {
        self.input_arguments()
            .find(|argument| argument.schema.id() == id)
    }

    pub(crate) fn argument(&self, id: &str) -> Option<&SceneArgument> {
        self.arguments
            .iter()
            .find(|argument| argument.schema.id() == id)
    }

    pub(crate) fn input_argument_mut(&mut self, id: &str) -> Option<&mut SceneArgument> {
        self.arguments
            .iter_mut()
            .find(|argument| argument.expression().is_none() && argument.schema.id() == id)
    }

    pub(crate) fn argument_mut(&mut self, id: &str) -> Option<&mut SceneArgument> {
        self.arguments
            .iter_mut()
            .find(|argument| argument.schema.id() == id)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.document.items().next().is_none()
    }

    pub(crate) fn item_layer(&self, id: ItemId) -> Option<LayerId> {
        self.document.item_layer(id)
    }

    pub(super) fn document(&self) -> &TimelineDocument {
        &self.document
    }

    pub(super) fn document_mut(&mut self) -> &mut TimelineDocument {
        &mut self.document
    }

    pub(super) fn allocate_argument_id(&mut self) -> (String, u64) {
        let ordinal = self.next_argument_id;
        self.next_argument_id = self.next_argument_id.saturating_add(1);
        (format!("argument_{ordinal}"), ordinal)
    }

    fn expression_replacements(&self) -> HashMap<String, String> {
        let labels = self
            .arguments
            .iter()
            .filter_map(|argument| {
                let label = expression::variable_name_for_label(argument.schema.label())?;
                Some((argument.schema.id().to_owned(), label))
            })
            .collect::<Vec<_>>();
        let mut counts = HashMap::new();
        for (_, label) in &labels {
            *counts.entry(label.clone()).or_insert(0_usize) += 1;
        }
        labels
            .into_iter()
            .filter(|(_, label)| counts.get(label.as_str()) == Some(&1))
            .map(|(id, label)| (label, id))
            .collect()
    }

    pub(crate) fn expression_from_display(&self, source: &str) -> Option<String> {
        expression::rewrite_variables(source, &self.expression_replacements())
    }

    pub(super) fn instantiate(
        &self,
        id: ItemId,
        start: Frame,
        duration: FrameDuration,
    ) -> Option<TimelineItem> {
        debug_assert_eq!(duration, self.duration());
        let parameters = ParameterValues::default();
        Some(TimelineItem {
            id,
            start,
            duration,
            kind: TimelineItemKind::Scene { scene_id: self.id },
            assets: HashMap::new(),
            parameters,
            animations: ParameterAnimations::default(),
            aspect_ratio_locked: false,
            effects: Vec::new(),
        })
    }

    pub(crate) fn from_project(
        id: SceneId,
        name: String,
        arguments: Vec<SceneArgument>,
        document: TimelineDocument,
    ) -> Self {
        let next_argument_id = arguments
            .iter()
            .filter_map(|argument| argument.schema.id().strip_prefix("argument_"))
            .filter_map(|id| id.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        Self {
            id,
            name,
            arguments,
            document,
            next_argument_id,
        }
    }
}

pub(crate) fn set_scene_instance_override(
    item: &mut TimelineItem,
    schema: &ParameterSchema,
    value: ParameterValue,
) -> bool {
    if !schema.accepts_value(&value) {
        return false;
    }
    let previous = item.parameters.remove(&schema.id);
    if &value == schema.default_value() {
        previous.is_some()
    } else {
        item.parameters
            .set(schema, value.clone())
            .expect("validated scene override must satisfy its contract");
        previous.as_ref() != Some(&value)
    }
}

pub(crate) fn materialize_scene_instance_parameters(
    item: &TimelineItem,
    scenes: &HashMap<SceneId, SceneDefinition>,
) -> Option<ParameterValues> {
    let scene = scenes.get(&item.scene_id()?)?;
    let schemas = scene
        .input_arguments()
        .map(|argument| argument.schema.parameter().clone())
        .collect::<Vec<_>>();
    Some(materialized_parameter_values(&item.parameters, &schemas))
}

pub(crate) fn display_scene_expression(arguments: &[SceneArgument], source: &str) -> String {
    let labels = arguments
        .iter()
        .filter_map(|argument| {
            let label = expression::variable_name_for_label(argument.schema.label())?;
            Some((argument.schema.id().to_owned(), label))
        })
        .collect::<Vec<_>>();
    let mut counts = HashMap::new();
    for (_, label) in &labels {
        *counts.entry(label.clone()).or_insert(0_usize) += 1;
    }
    let replacements = labels
        .into_iter()
        .filter(|(_, label)| counts.get(label.as_str()) == Some(&1))
        .collect();
    expression::rewrite_variables(source, &replacements).unwrap_or_else(|| source.to_owned())
}
