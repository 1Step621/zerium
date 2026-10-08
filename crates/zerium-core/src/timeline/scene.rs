use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::property::{
    PropertyElementId, PropertyPath, PropertySchema, PropertyType, PropertyValue,
    PropertyValueType, ScalarPropertyType,
};

use super::{
    document::TimelineDocument,
    ids::{EffectInstanceId, ItemId, LayerId, SceneId},
    item::TimelineItem,
    property_address::resolve_property_schema,
    time::{Frame, FrameDuration},
};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneBindingOwner {
    Item,
    Effect(EffectInstanceId),
}

impl SceneBindingOwner {
    pub fn from_effect(effect_id: Option<EffectInstanceId>) -> Self {
        effect_id.map_or(Self::Item, Self::Effect)
    }

    pub const fn effect_id(self) -> Option<EffectInstanceId> {
        match self {
            Self::Item => None,
            Self::Effect(effect_id) => Some(effect_id),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Hash, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SceneBindingTarget {
    item_id: ItemId,
    owner: SceneBindingOwner,
    path: PropertyPath,
}

impl SceneBindingTarget {
    pub fn new(
        item_id: ItemId,
        owner: SceneBindingOwner,
        property_id: impl Into<String>,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
    ) -> Self {
        Self {
            item_id,
            owner,
            path: PropertyPath::new(property_id, element_id, scalar_index),
        }
    }

    pub const fn item_id(&self) -> ItemId {
        self.item_id
    }

    pub const fn owner(&self) -> SceneBindingOwner {
        self.owner
    }

    pub fn property_id(&self) -> &str {
        self.path.property_id()
    }

    pub const fn element_id(&self) -> Option<PropertyElementId> {
        self.path.element_id()
    }

    pub const fn scalar_index(&self) -> Option<usize> {
        self.path.scalar_index()
    }

    pub fn matches(
        &self,
        item_id: ItemId,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
    ) -> bool {
        self.item_id == item_id
            && self.owner.effect_id() == effect_id
            && self.property_id() == property_id
    }

    pub fn conflicts_with_animation(
        &self,
        property_id: &str,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
    ) -> bool {
        if self.property_id() != property_id || self.element_id() != element_id {
            return false;
        }
        match (self.scalar_index(), scalar_index) {
            (None, _) | (_, None) => true,
            (Some(bound), Some(animated)) => bound == animated,
        }
    }
}

/// A binding target resolved against one concrete scene item.
///
/// Keeping this resolution here gives editing, persistence validation, and
/// evaluation one definition of what a valid scene binding points at.
pub struct ResolvedSceneBinding {
    pub schema: PropertySchema,
    pub animated: bool,
}

pub fn resolve_scene_binding(
    scenes: &HashMap<SceneId, SceneDefinition>,
    scene: &SceneDefinition,
    target: &SceneBindingTarget,
) -> Option<ResolvedSceneBinding> {
    let item = scene.document().item(target.item_id())?;
    let property_id = target.property_id();
    let schema = resolve_property_schema(scenes, item, target.owner().effect_id(), property_id)?;
    let (value, animations) = match target.owner() {
        SceneBindingOwner::Effect(effect_id) => {
            let effect = item.effect(effect_id)?;
            (effect.properties.property(property_id)?, &effect.animations)
        }
        SceneBindingOwner::Item => {
            let value = item.properties.property(property_id).or_else(|| {
                let nested = scenes.get(&item.scene_id()?)?;
                nested
                    .argument(property_id)
                    .map(|argument| argument.schema.default_value())
            })?;
            (value, &item.animations)
        }
    };
    let animated = animations.track(&target.path).is_some();
    let resolved = schema.resolve_scalar(value, target.element_id(), target.scalar_index())?;
    let mut schema = schema.clone();
    schema.ty = PropertyType::Value(PropertyValueType::Scalar(resolved.ty.clone()));
    schema.configurations = vec![resolved.configuration.clone()];
    schema.append_default = None;
    schema.default = schema.constrained_value(resolved.value)?;
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
        SceneBindingOwner::Effect(effect_id) => item.effect_mut(effect_id).and_then(|effect| {
            let effect_schema = effect.schema.clone();
            let target_schema = effect_schema.property(property_id)?;
            let current = effect.properties.property(property_id)?;
            let value = current.replaced_at(target.element_id(), target.scalar_index(), value)?;
            effect.properties.set(target_schema, value).ok()
        }),
        SceneBindingOwner::Item => {
            let item_schema = item.schema_arc().cloned();
            let target_schema = item_schema
                .as_deref()
                .and_then(|schema| schema.property(property_id))
                .unwrap_or(binding_schema);
            let current = item
                .properties
                .property(property_id)
                .unwrap_or(binding_schema.default_value());
            let value = current.replaced_at(target.element_id(), target.scalar_index(), value)?;
            item.properties.set(target_schema, value).ok()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneArgumentPreset {
    Number,
    SignedInteger,
    UnsignedInteger,
    Boolean,
    Color,
    Text,
    File,
}

impl SceneArgumentPreset {
    pub fn ty(self) -> PropertyType {
        let scalar = match self {
            Self::File => ScalarPropertyType::File,
            Self::Number => ScalarPropertyType::F32,
            Self::SignedInteger => ScalarPropertyType::I32,
            Self::UnsignedInteger => ScalarPropertyType::U32,
            Self::Boolean => ScalarPropertyType::Bool,
            Self::Color => ScalarPropertyType::Color,
            Self::Text => ScalarPropertyType::String,
        };
        PropertyType::Value(PropertyValueType::Scalar(scalar))
    }
}

impl PropertySchema {
    pub fn for_scene_argument(mut self) -> Option<Self> {
        if !matches!(self.ty, PropertyType::Value(PropertyValueType::Scalar(_))) {
            return None;
        }
        self.default = self.constrained_value(self.default_value())?;
        // A scene argument is its own editable input contract. The source
        // property's editability only controls direct edits on the bound
        // plugin property.
        self.configuration_mut(None).editable = true;
        for configuration in &mut self.configurations {
            configuration.scene_bindable = true;
        }
        Some(self)
    }

    pub fn with_scene_default(&self, value: &PropertyValue) -> Option<Self> {
        let default = self.constrained_value(value)?;
        let mut next = self.clone();
        next.default = default;
        Some(next)
    }

    pub fn with_scene_numeric_settings(
        &self,
        settings: crate::property::NumericSettings,
    ) -> Option<Self> {
        let (default, constraints) = settings.into_parts();
        if !self.ty().allows(&default) {
            return None;
        }
        let mut next = self.clone();
        next.default = default;
        next.configuration_mut(None).constraints = constraints;
        Some(next)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneArgument {
    pub schema: PropertySchema,
    pub bindings: Vec<SceneBindingTarget>,
}

impl SceneArgument {
    pub fn new(schema: PropertySchema, bindings: Vec<SceneBindingTarget>) -> Self {
        Self { schema, bindings }
    }
}

pub(super) fn unique_scene_argument_name(
    arguments: &[SceneArgument],
    preferred: &str,
    fallback: &str,
) -> String {
    let normalized = if preferred.trim().is_empty() {
        fallback
    } else {
        preferred.trim()
    };
    let normalized_is_duplicate = |candidate: &str| {
        arguments
            .iter()
            .any(|argument| argument.schema.label() == candidate)
    };
    if !normalized_is_duplicate(normalized) {
        return normalized.to_owned();
    }
    (2_u64..)
        .map(|suffix| format!("{normalized}_{suffix}"))
        .find(|candidate| !normalized_is_duplicate(candidate))
        .expect("a scene argument name suffix must eventually be available")
}

#[derive(Clone)]
pub struct SceneDefinition {
    pub id: SceneId,
    pub name: String,
    pub arguments: Vec<SceneArgument>,
    document: TimelineDocument,
    next_argument_id: Option<u64>,
}

impl SceneDefinition {
    /// Apply arguments to detached items. None selects the scene's defaults when
    /// editing or visiting the definition independently of a placed instance.
    pub(crate) fn apply_arguments<'a>(
        &self,
        scenes: &HashMap<SceneId, SceneDefinition>,
        instance: Option<&TimelineItem>,
        items: impl IntoIterator<Item = &'a mut TimelineItem>,
    ) {
        for item in items {
            let item_id = item.id;
            for argument in &self.arguments {
                let value = instance
                    .and_then(|instance| instance.properties.property(argument.schema.id()))
                    .unwrap_or_else(|| argument.schema.default_value());
                for binding in argument
                    .bindings
                    .iter()
                    .filter(|binding| binding.item_id() == item_id)
                {
                    let Some(resolved) = resolve_scene_binding(scenes, self, binding) else {
                        continue;
                    };
                    apply_scene_binding_to_item(item, binding, &resolved.schema, value.clone())
                        .expect("validated scene binding must remain applicable during evaluation");
                }
            }
        }
    }

    pub fn duration(&self) -> FrameDuration {
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

    pub fn items(&self) -> impl Iterator<Item = &TimelineItem> {
        self.document.items()
    }

    pub fn arguments(&self) -> impl Iterator<Item = &SceneArgument> {
        self.arguments.iter()
    }

    pub fn argument(&self, id: &str) -> Option<&SceneArgument> {
        self.arguments
            .iter()
            .find(|argument| argument.schema.id() == id)
    }

    pub fn argument_mut(&mut self, id: &str) -> Option<&mut SceneArgument> {
        self.arguments
            .iter_mut()
            .find(|argument| argument.schema.id() == id)
    }

    pub fn is_empty(&self) -> bool {
        self.document.items().next().is_none()
    }

    pub fn item_layer(&self, id: ItemId) -> Option<LayerId> {
        self.document.item_layer(id)
    }

    pub(super) fn document(&self) -> &TimelineDocument {
        &self.document
    }

    pub(super) fn document_mut(&mut self) -> &mut TimelineDocument {
        &mut self.document
    }

    pub(super) fn allocate_argument_id(&mut self) -> Option<(String, u64)> {
        let ordinal = self.next_argument_id?;
        self.next_argument_id = ordinal.checked_add(1).filter(|id| *id != u64::MAX);
        Some((format!("argument_{ordinal}"), ordinal))
    }

    pub fn from_project(
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
            .checked_add(1)
            .filter(|id| *id != u64::MAX);
        Self {
            id,
            name,
            arguments,
            document,
            next_argument_id,
        }
    }
}
