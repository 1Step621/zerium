use crate::property::{PropertyElementId, PropertyPath, PropertyValue};

use std::collections::HashMap;

use super::{
    TimelineItem,
    ids::{EffectInstanceId, ItemId, SceneId},
    scene::SceneDefinition,
};
use crate::property::PropertySchema;

/// The property contract of an item or effect, including scene instance inputs.
pub(super) fn property_schemas<'a>(
    scenes: &'a HashMap<SceneId, SceneDefinition>,
    item: &'a TimelineItem,
    effect_id: Option<EffectInstanceId>,
) -> impl Iterator<Item = &'a PropertySchema> {
    let properties = match effect_id {
        Some(id) => item
            .effects
            .iter()
            .find(|effect| effect.id == id)
            .map(|effect| effect.schema().properties()),
        None => item.schema().map(|schema| schema.properties()),
    };
    let scene = effect_id
        .is_none()
        .then(|| item.scene_id())
        .flatten()
        .and_then(|id| scenes.get(&id));
    properties.into_iter().flatten().chain(
        scene
            .into_iter()
            .flat_map(|scene| scene.arguments.iter().map(|argument| &argument.schema)),
    )
}

pub(super) fn resolve_property_schema<'a>(
    scenes: &'a HashMap<SceneId, SceneDefinition>,
    item: &'a TimelineItem,
    effect_id: Option<EffectInstanceId>,
    property_id: &str,
) -> Option<&'a PropertySchema> {
    property_schemas(scenes, item, effect_id).find(|property| property.id() == property_id)
}

/// Identifies a property value independently of any inspector row or widget.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PropertyAddress {
    pub item_id: ItemId,
    pub effect_id: Option<EffectInstanceId>,
    pub property_id: String,
    pub element_id: Option<PropertyElementId>,
    pub scalar_index: Option<usize>,
}

impl PropertyAddress {
    pub fn path(&self) -> PropertyPath {
        PropertyPath::new(&self.property_id, self.element_id, self.scalar_index)
    }

    pub fn schema<'a>(
        &self,
        editor: &'a super::TimelineEditor,
    ) -> Option<&'a crate::property::PropertySchema> {
        editor.property_schema(self.item_id, self.effect_id, &self.property_id)
    }

    pub fn value<'a>(&self, item: &'a super::TimelineItem) -> Option<&'a PropertyValue> {
        item.property_values(self.effect_id)?
            .property(&self.property_id)?
            .scalar(self.element_id, self.scalar_index)
    }
}
