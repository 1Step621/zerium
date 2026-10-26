use crate::{
    property::{PropertyElementId, PropertyOverrides, PropertyPath, PropertySchema},
    timeline::{
        TimelineItem,
        model::{EffectInstanceId, ItemId, SceneId, scene::SceneDefinition},
    },
};

use std::collections::HashMap;

/// The property contract of an item or effect, including scene instance inputs.
pub(in crate::timeline) fn property_schemas<'a>(
    scenes: &'a HashMap<SceneId, SceneDefinition>,
    item: &'a TimelineItem,
    effect_id: Option<EffectInstanceId>,
) -> impl Iterator<Item = &'a PropertySchema> {
    let properties = item.property_owner(effect_id).map(|owner| owner.schemas);
    let scene = effect_id.is_none().then(|| item.scene_id()).flatten();
    owner_property_schemas(scenes, properties, scene)
}

pub(in crate::timeline) fn owner_property_schemas<'a>(
    scenes: &'a HashMap<SceneId, SceneDefinition>,
    properties: Option<&'a [PropertySchema]>,
    scene: Option<SceneId>,
) -> impl Iterator<Item = &'a PropertySchema> {
    properties.into_iter().flatten().chain(
        scene
            .and_then(|id| scenes.get(&id))
            .into_iter()
            .flat_map(|scene| {
                scene
                    .arguments()
                    .filter_map(|argument| argument.argument_schema())
            }),
    )
}

pub(in crate::timeline) fn resolve_property_schema<'a>(
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
}

/// Creation accepts ordinary authored values; preference storage belongs to the app.
pub(in crate::timeline) fn validate_initial_properties(
    values: &PropertyOverrides,
    schemas: &[PropertySchema],
) -> Result<(), crate::timeline::TimelineEditError> {
    for (id, value) in values.iter() {
        let valid = schemas
            .iter()
            .find(|schema| schema.id() == id)
            .is_some_and(|schema| schema.is_editable(None) && schema.accepts_value(value));
        if !valid {
            return Err(crate::timeline::TimelineEditError::InvalidPropertyValue(
                id.to_owned(),
            ));
        }
    }
    Ok(())
}
