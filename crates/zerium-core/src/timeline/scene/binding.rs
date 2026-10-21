use super::SceneDefinition;
use crate::property::{PropertyElementId, PropertyPath, PropertySchema, PropertyValue};
use crate::timeline::property_address::resolve_property_schema;
use crate::timeline::{EffectInstanceId, ItemId, SceneId, TimelineItem};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
            (
                effect.properties.property(property_id)?.clone(),
                &effect.animations,
            )
        }
        SceneBindingOwner::Item => {
            let value = item.properties.property(property_id).cloned().or_else(|| {
                let nested = scenes.get(&item.scene_id()?)?;
                nested.argument(property_id).and_then(|argument| {
                    argument
                        .argument_schema()
                        .map(|schema| schema.default_value())
                })
            })?;
            (value, &item.animations)
        }
    };
    let animated = animations.track(&target.path).is_some();
    let schema = schema.scalar_projection(&value, target.element_id(), target.scalar_index())?;
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
                .cloned()
                .unwrap_or_else(|| binding_schema.default_value());
            let value = current.replaced_at(target.element_id(), target.scalar_index(), value)?;
            item.properties.set(target_schema, value).ok()
        }
    }
}
