//! Resolve defaults, scene arguments and animation through one property pipeline.

use std::collections::HashMap;

use crate::property::{PropertySchema, PropertyValue, PropertyValues};

use super::{
    EffectInstanceId, PropertyAddress, SceneDefinition, SceneId, TimelineItem, TimelineTime,
    property_address::property_schemas, scene::resolve_scene_binding,
};

/// Arguments of a scene definition, optionally supplied by a placed instance.
#[derive(Clone, Copy)]
pub(super) struct SceneArguments<'a> {
    scenes: &'a HashMap<SceneId, SceneDefinition>,
    scene: &'a SceneDefinition,
    values: Option<&'a PropertyValues>,
}

impl<'a> SceneArguments<'a> {
    pub(super) fn new(
        scenes: &'a HashMap<SceneId, SceneDefinition>,
        scene: &'a SceneDefinition,
        values: Option<&'a PropertyValues>,
    ) -> Self {
        Self {
            scenes,
            scene,
            values,
        }
    }

    fn resolve(self, address: &PropertyAddress, mut value: PropertyValue) -> PropertyValue {
        for argument in &self.scene.arguments {
            let input = self
                .values
                .and_then(|values| values.property(argument.schema.id()))
                .unwrap_or_else(|| argument.schema.default_value());
            for binding in argument.bindings.iter().filter(|binding| {
                binding.matches(address.item_id, address.effect_id, &address.property_id)
            }) {
                let Some(resolved) = resolve_scene_binding(self.scenes, self.scene, binding) else {
                    continue;
                };
                let input = resolved
                    .schema
                    .constrained_value(input)
                    .expect("validated scene argument must satisfy its target type");
                value = value
                    .replaced_at(binding.element_id(), binding.scalar_index(), input)
                    .expect("validated scene binding must remain applicable during evaluation");
            }
        }
        value
    }
}

pub(super) fn resolve_property(
    arguments: Option<SceneArguments<'_>>,
    item: &TimelineItem,
    effect: Option<EffectInstanceId>,
    property: &PropertySchema,
    time: Option<TimelineTime>,
) -> Option<PropertyValue> {
    let mut value = property.resolve_value(item.property_values(effect)?.property(property.id()));
    if let Some(arguments) = arguments {
        value = arguments.resolve(
            &PropertyAddress {
                item_id: item.id,
                effect_id: effect,
                property_id: property.id().to_owned(),
                element_id: None,
                scalar_index: None,
            },
            value,
        );
    }
    if let Some(time) = time {
        value = item
            .animations(effect)?
            .evaluate_property(value, property, |track| {
                item.animation_clock(track).progress_at(time)
            });
    }
    Some(value)
}

pub(super) fn resolve_item(
    scenes: &HashMap<SceneId, SceneDefinition>,
    arguments: Option<SceneArguments<'_>>,
    source: &TimelineItem,
    time: Option<TimelineTime>,
) -> TimelineItem {
    let values = |effect| {
        let mut values = PropertyValues::default();
        for property in property_schemas(scenes, source, effect) {
            let value = resolve_property(arguments, source, effect, property, time)
                .expect("property owner comes from this item");
            values
                .set(property, value)
                .expect("resolved values preserve the property contract");
        }
        values
    };
    let mut item = source.clone();
    item.properties = values(None);
    for effect in &mut item.effects {
        effect.properties = values(Some(effect.id));
    }
    item
}
