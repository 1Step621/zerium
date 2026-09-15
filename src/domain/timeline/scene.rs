use std::collections::{HashMap, HashSet};

use crate::domain::animation::PropertyAnimations;
use crate::domain::property::materialized_property_values;
use crate::domain::property::{
    PropertyAnimatable, PropertyEditable, PropertyElementId, PropertySchema, PropertyType,
    PropertyValue, PropertyValueType, PropertyValues, ScalarPropertyType,
};

use super::{
    document::TimelineDocument,
    expression,
    ids::{EffectInstanceId, ItemId, LayerId, SceneId},
    item::{TimelineItem, TimelineItemKind},
    time::{Frame, FrameDuration, FrameRate},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PropertyOwner {
    Item,
    Effect(EffectInstanceId),
}

impl PropertyOwner {
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

pub(crate) type SceneBindingOwner = PropertyOwner;
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SceneBindingTarget {
    item_id: ItemId,
    owner: PropertyOwner,
    property_id: String,
    element_id: Option<PropertyElementId>,
    scalar_index: Option<usize>,
}

impl SceneBindingTarget {
    pub(crate) fn new(
        item_id: ItemId,
        owner: SceneBindingOwner,
        property_id: impl Into<String>,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
    ) -> Self {
        Self {
            item_id,
            owner,
            property_id: property_id.into(),
            element_id,
            scalar_index,
        }
    }

    pub(crate) const fn item_id(&self) -> ItemId {
        self.item_id
    }

    pub(crate) const fn owner(&self) -> SceneBindingOwner {
        self.owner
    }

    pub(crate) fn property_id(&self) -> &str {
        &self.property_id
    }

    pub(crate) const fn element_id(&self) -> Option<PropertyElementId> {
        self.element_id
    }

    pub(crate) const fn scalar_index(&self) -> Option<usize> {
        self.scalar_index
    }

    pub(crate) fn matches(
        &self,
        item_id: ItemId,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
    ) -> bool {
        self.item_id == item_id
            && self.owner.effect_id() == effect_id
            && self.property_id == property_id
    }

    pub(crate) fn conflicts_with_aspect_ratio_lock(
        &self,
        item: &TimelineItem,
        locked: bool,
    ) -> bool {
        locked
            && self.owner() == SceneBindingOwner::Item
            && self.scalar_index() == Some(1)
            && item
                .schema()
                .is_some_and(|schema| schema.is_size_property(self.property_id()))
    }

    pub(crate) fn conflicts_with_animation(
        &self,
        property_id: &str,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
    ) -> bool {
        if self.property_id != property_id || self.element_id != element_id {
            return false;
        }
        match (self.scalar_index, scalar_index) {
            (None, _) | (_, None) => true,
            (Some(bound), Some(animated)) => bound == animated,
        }
    }
}

pub(crate) fn project_scene_binding_value(
    schema: &PropertySchema,
    value: &PropertyValue,
    element_id: Option<PropertyElementId>,
    scalar_index: Option<usize>,
) -> Option<(PropertySchema, PropertyValue)> {
    let mut schema = schema.clone();
    let mut value = value.clone();

    if let Some(id) = element_id {
        let PropertyType::Array { element_type, .. } = schema.ty else {
            return None;
        };
        let PropertyValue::Array(values) = value else {
            return None;
        };
        value = values
            .iter()
            .find(|element| element.element_id() == id)?
            .value()
            .clone();
        schema.ty = PropertyType::Value(element_type);
    } else if matches!(schema.ty, PropertyType::Array { .. }) {
        return None;
    }

    if let Some(scalar_index) = scalar_index {
        let PropertyType::Value(PropertyValueType::Tuple(tuple)) = &schema.ty else {
            return None;
        };
        let PropertyValue::Tuple(values) = value else {
            return None;
        };
        value = values.get(scalar_index)?.clone();
        schema.ty = PropertyType::Value(PropertyValueType::Scalar(
            tuple.scalars().get(scalar_index)?.clone(),
        ));
        schema.constraints = schema.constraints.for_scalar(scalar_index).clone();
    } else {
        if !matches!(
            &schema.ty,
            PropertyType::Value(PropertyValueType::Scalar(_))
        ) {
            return None;
        }
    }

    schema.ui = schema.ui.to_scalar(scalar_index);

    if let Some(scalar_index) = scalar_index {
        schema.animatable = schema.animatable.to_scalar(scalar_index);
        schema.editable = schema.editable.to_scalar(scalar_index);
    }
    value = schema.constrained_value(&value)?;
    schema.default = value.clone();
    Some((schema, value))
}

pub(crate) fn apply_scene_binding_value(
    current: &PropertyValue,
    element_id: Option<PropertyElementId>,
    scalar_index: Option<usize>,
    value: PropertyValue,
) -> Option<PropertyValue> {
    let replace_scalar_index = |current: &PropertyValue| {
        let Some(scalar_index) = scalar_index else {
            return Some(value.clone());
        };
        let PropertyValue::Tuple(values) = current else {
            return None;
        };
        let mut values = values.clone();
        *values.get_mut(scalar_index)? = value.clone();
        Some(PropertyValue::Tuple(values))
    };

    let Some(id) = element_id else {
        return replace_scalar_index(current);
    };
    let PropertyValue::Array(values) = current else {
        return None;
    };
    let mut values = values.clone();
    let target = values
        .iter_mut()
        .find(|element| element.element_id() == id)?;
    *target.value_mut() = replace_scalar_index(target.value())?;
    Some(PropertyValue::Array(values))
}

pub(crate) fn scene_binding_is_animated(
    animations: &PropertyAnimations,
    property_id: &str,
    element_id: Option<PropertyElementId>,
    scalar_index: Option<usize>,
) -> bool {
    animations
        .property(property_id)
        .and_then(|property| property.element(element_id))
        .and_then(|element| element.scalar(scalar_index))
        .is_some()
}

/// A binding target resolved against one concrete scene item.
///
/// Keeping this resolution here gives editing, persistence validation, and
/// evaluation one definition of what a valid scene binding points at.
pub(crate) struct ResolvedSceneBinding {
    pub(crate) schema: PropertySchema,
    pub(crate) animated: bool,
}

pub(crate) fn resolve_property_schema<'a>(
    scenes: &'a HashMap<SceneId, SceneDefinition>,
    item: &'a TimelineItem,
    owner: PropertyOwner,
    property_id: &str,
) -> Option<&'a PropertySchema> {
    match owner {
        PropertyOwner::Effect(effect_id) => item
            .effects
            .iter()
            .find(|effect| effect.id == effect_id)?
            .schema()
            .property(property_id),
        PropertyOwner::Item => item
            .schema()
            .and_then(|schema| schema.property(property_id))
            .or_else(|| {
                scenes
                    .get(&item.scene_id()?)?
                    .input_argument(property_id)
                    .map(|argument| argument.schema.property())
            }),
    }
}

pub(crate) fn resolve_scene_binding(
    scenes: &HashMap<SceneId, SceneDefinition>,
    scene: &SceneDefinition,
    target: &SceneBindingTarget,
) -> Option<ResolvedSceneBinding> {
    let item = scene.document().item(target.item_id())?;
    let property_id = target.property_id();
    let schema = resolve_property_schema(scenes, item, target.owner(), property_id)?;
    let (value, animations) = match target.owner() {
        SceneBindingOwner::Effect(effect_id) => {
            let effect = item.effects.iter().find(|effect| effect.id == effect_id)?;
            (effect.properties.property(property_id)?, &effect.animations)
        }
        SceneBindingOwner::Item => {
            let value = item.properties.property(property_id).or_else(|| {
                let nested = scenes.get(&item.scene_id()?)?;
                nested
                    .input_argument(property_id)
                    .map(|argument| argument.schema.default_value())
            })?;
            (value, &item.animations)
        }
    };
    let animated = scene_binding_is_animated(
        animations,
        property_id,
        target.element_id(),
        target.scalar_index(),
    );
    let (schema, _) =
        project_scene_binding_value(schema, value, target.element_id(), target.scalar_index())?;
    Some(ResolvedSceneBinding { schema, animated })
}

pub(crate) fn apply_scene_binding_to_item(
    item: &mut TimelineItem,
    target: &SceneBindingTarget,
    binding_schema: &PropertySchema,
    value: PropertyValue,
) -> Option<bool> {
    let value = binding_schema.constrained_value(&value)?;
    let property_id = target.property_id();
    match target.owner() {
        SceneBindingOwner::Effect(effect_id) => item
            .effects
            .iter_mut()
            .find(|effect| effect.id == effect_id)
            .and_then(|effect| {
                let effect_schema = effect.schema.clone();
                let target_schema = effect_schema.property(property_id)?;
                let current = effect.properties.property(property_id)?;
                let value = apply_scene_binding_value(
                    current,
                    target.element_id(),
                    target.scalar_index(),
                    value,
                )?;
                effect.properties.set(target_schema, value).ok()
            }),
        SceneBindingOwner::Item => {
            let item_schema = item.schema_arc().cloned();
            let target_schema = item_schema
                .as_deref()
                .and_then(|schema| schema.property(property_id))
                .unwrap_or(binding_schema);
            let current = item.properties.property(property_id)?;
            let value = apply_scene_binding_value(
                current,
                target.element_id(),
                target.scalar_index(),
                value,
            )?;
            item.properties.set(target_schema, value).ok()
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SceneArgumentSchema {
    property: PropertySchema,
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
    pub(crate) const fn scalar(self) -> ScalarPropertyType {
        match self {
            Self::Number => ScalarPropertyType::F32,
            Self::SignedInteger => ScalarPropertyType::I32,
            Self::UnsignedInteger => ScalarPropertyType::U32,
            Self::Boolean => ScalarPropertyType::Bool,
            Self::Color => ScalarPropertyType::Color,
            Self::Text => ScalarPropertyType::String,
        }
    }
}

impl SceneArgumentSchema {
    pub(crate) fn from_property(mut property: PropertySchema) -> Option<Self> {
        if !matches!(
            property.ty,
            PropertyType::Value(PropertyValueType::Scalar(_))
        ) {
            return None;
        }
        let default = property.default_value().clone();
        property.default = property.constrained_value(&default)?;
        // A scene argument is its own editable input contract. The source
        // property's editability only controls direct edits on the bound
        // plugin property.
        property.editable = PropertyEditable::Scalar(true);
        property.scene_bindable = true;
        Some(Self { property })
    }

    pub(crate) fn property(&self) -> &PropertySchema {
        &self.property
    }

    pub(crate) fn id(&self) -> &str {
        self.property.id()
    }

    pub(crate) fn label(&self) -> &str {
        self.property.label()
    }

    pub(crate) fn ty(&self) -> &PropertyType {
        self.property.ty()
    }

    pub(crate) fn default_value(&self) -> &PropertyValue {
        self.property.default_value()
    }

    pub(crate) fn constrained_value(&self, value: &PropertyValue) -> Option<PropertyValue> {
        self.property.constrained_value(value)
    }

    pub(crate) fn rename(&mut self, label: String) {
        self.property.label = label;
    }

    pub(crate) fn with_default(&self, value: &PropertyValue) -> Option<Self> {
        let default = self.property.constrained_value(value)?;
        let mut next = self.clone();
        next.property.default = default;
        Some(next)
    }

    pub(crate) fn with_numeric_settings(
        &self,
        settings: crate::domain::property::NumericSettings,
    ) -> Option<Self> {
        let (default, constraints) = settings.into_parts();
        if !self.property.ty().allows(&default) {
            return None;
        }
        let mut next = self.clone();
        next.property.default = default;
        next.property.constraints = constraints;
        Some(next)
    }

    pub(crate) fn with_identity(mut self, id: String, label: String) -> Self {
        self.property.id = id;
        self.property.label = label;
        self
    }

    fn into_expression(mut self) -> Option<Self> {
        if self.ty() != &PropertyType::Value(PropertyValueType::Scalar(ScalarPropertyType::F32)) {
            return None;
        }
        self.property.editable = PropertyEditable::Scalar(false);
        self.property.animatable = PropertyAnimatable::Scalar(false);
        self.property.scene_bindable = false;
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
                != &PropertyType::Value(PropertyValueType::Scalar(ScalarPropertyType::F32))
    }) {
        return false;
    }
    let numeric_names = arguments
        .iter()
        .filter(|argument| {
            argument.schema.ty()
                == &PropertyType::Value(PropertyValueType::Scalar(ScalarPropertyType::F32))
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
        let properties = PropertyValues::default();
        Some(TimelineItem {
            id,
            start,
            duration,
            kind: TimelineItemKind::Scene { scene_id: self.id },
            assets: HashMap::new(),
            properties,
            animations: PropertyAnimations::default(),
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
    schema: &PropertySchema,
    value: PropertyValue,
) -> bool {
    if !schema.accepts_value(&value) {
        return false;
    }
    let previous = item.properties.remove(&schema.id);
    if &value == schema.default_value() {
        previous.is_some()
    } else {
        item.properties
            .set(schema, value.clone())
            .expect("validated scene override must satisfy its contract");
        previous.as_ref() != Some(&value)
    }
}

pub(crate) fn materialize_scene_instance_properties(
    item: &TimelineItem,
    scenes: &HashMap<SceneId, SceneDefinition>,
) -> Option<PropertyValues> {
    let scene = scenes.get(&item.scene_id()?)?;
    let schemas = scene
        .input_arguments()
        .map(|argument| argument.schema.property().clone())
        .collect::<Vec<_>>();
    Some(materialized_property_values(&item.properties, &schemas))
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
