use super::SceneDefinition;
use crate::{
    property::{PropertyElementId, PropertyPath, PropertySchema, PropertyValue},
    timeline::{
        EffectInstanceId, ItemId, SceneId, TimelineItem,
        model::item::property::resolve_property_schema,
    },
};
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
#[serde(from = "BindingTargetFile", into = "BindingTargetFile")]
pub struct SceneBindingTarget {
    address: crate::timeline::PropertyAddress,
}

// Keep the project and clipboard wire format independent of runtime addressing.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BindingTargetFile {
    item_id: ItemId,
    owner: SceneBindingOwner,
    path: PropertyPath,
}

impl From<BindingTargetFile> for SceneBindingTarget {
    fn from(file: BindingTargetFile) -> Self {
        Self::new(
            file.item_id,
            file.owner,
            file.path.property_id(),
            file.path.element_id(),
            file.path.scalar_index(),
        )
    }
}

impl From<SceneBindingTarget> for BindingTargetFile {
    fn from(target: SceneBindingTarget) -> Self {
        Self {
            item_id: target.item_id(),
            owner: target.owner(),
            path: target.address.path(),
        }
    }
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
            address: crate::timeline::PropertyAddress {
                item_id,
                effect_id: owner.effect_id(),
                property_id: property_id.into(),
                element_id,
                scalar_index,
            },
        }
    }

    pub fn address(&self) -> &crate::timeline::PropertyAddress {
        &self.address
    }

    pub const fn item_id(&self) -> ItemId {
        self.address.item_id
    }

    pub fn owner(&self) -> SceneBindingOwner {
        SceneBindingOwner::from_effect(self.address.effect_id)
    }

    pub fn property_id(&self) -> &str {
        &self.address.property_id
    }

    pub const fn element_id(&self) -> Option<PropertyElementId> {
        self.address.element_id
    }

    pub const fn scalar_index(&self) -> Option<usize> {
        self.address.scalar_index
    }

    pub fn matches(
        &self,
        item_id: ItemId,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
    ) -> bool {
        self.item_id() == item_id
            && self.owner().effect_id() == effect_id
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
#[derive(Clone)]
pub(crate) struct ResolvedSceneBinding {
    pub schema: PropertySchema,
    pub animated: bool,
}

pub(crate) fn resolve_scene_binding(
    scenes: &HashMap<SceneId, SceneDefinition>,
    scene: &SceneDefinition,
    target: &SceneBindingTarget,
) -> Option<ResolvedSceneBinding> {
    let item = scene.document().item(target.item_id())?;
    let property_id = target.property_id();
    let schema = resolve_property_schema(scenes, item, target.owner().effect_id(), property_id)?;
    let owner = item.property_owner(target.owner().effect_id())?;
    let value = owner.values.resolve(schema);
    let animations = owner.animations;
    let animated = animations.track(&target.address.path()).is_some();
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
            let current = effect.properties.resolve(target_schema);
            let value = current.replaced_at(target.element_id(), target.scalar_index(), value)?;
            effect.properties.set(target_schema, value).ok()
        }),
        SceneBindingOwner::Item => {
            let item_schema = item.schema_arc().cloned();
            let target_schema = item_schema
                .as_deref()
                .and_then(|schema| schema.property(property_id))
                .unwrap_or(binding_schema);
            let current = item.properties.resolve(target_schema);
            let value = current.replaced_at(target.element_id(), target.scalar_index(), value)?;
            item.properties.set(target_schema, value).ok()
        }
    }
}

/// Ordered reverse connections, derived once for an immutable variable collection.
pub(super) type BindingIndex =
    HashMap<(ItemId, Option<EffectInstanceId>), HashMap<String, Vec<(usize, usize)>>>;

impl SceneDefinition {
    /// Connections to this property, in variable order. Element and scalar
    /// coordinates can be filtered by the caller when needed.
    pub fn bindings_for<'a>(
        &'a self,
        address: &crate::timeline::PropertyAddress,
    ) -> impl Iterator<
        Item = (
            &'a crate::timeline::model::scene::SceneVariable,
            &'a SceneBindingTarget,
        ),
    > {
        let index = self.runtime.bindings.get_or_init(|| {
            let mut index = BindingIndex::new();
            for (variable_index, variable) in self.variables.iter().enumerate() {
                for (binding_index, binding) in variable.bindings.iter().enumerate() {
                    index
                        .entry((binding.item_id(), binding.address().effect_id))
                        .or_default()
                        .entry(binding.property_id().to_owned())
                        .or_default()
                        .push((variable_index, binding_index));
                }
            }
            index
        });
        index
            .get(&(address.item_id, address.effect_id))
            .and_then(|properties| properties.get(address.property_id.as_str()))
            .into_iter()
            .flatten()
            .map(|&(variable, binding)| {
                let variable = &self.variables[variable];
                (variable, &variable.bindings[binding])
            })
    }
}
