//! Resolve defaults, scene variables and animation through one property pipeline.

use std::collections::HashMap;

use crate::property::{PropertySchema, PropertyValue, PropertyValues};

use super::{
    EffectInstanceId, PropertyAddress, SceneDefinition, SceneId, TimelineItem, TimelineTime,
    property_address::property_schemas, scene::resolve_scene_binding,
};

/// A property's usable preview value and diagnostics from the same resolution.
/// Failed bindings retain their destination's authored value.
pub(super) struct ResolvedProperty {
    pub value: PropertyValue,
    pub diagnostics: Vec<String>,
}

impl ResolvedProperty {
    /// A scene argument propagates a failed input to its downstream consumers.
    fn into_result(self) -> Result<PropertyValue, String> {
        match self.diagnostics.into_iter().next() {
            Some(error) => Err(error),
            None => Ok(self.value),
        }
    }
}

pub(super) struct ResolvedItem {
    pub value: TimelineItem,
    pub diagnostics: Vec<String>,
}

/// Evaluated scene variables shared by all connected properties in one sample.
pub(super) struct SceneBindings<'a> {
    scenes: &'a HashMap<SceneId, SceneDefinition>,
    scene: &'a SceneDefinition,
    evaluation: super::scene::VariableEvaluator<'a>,
}

impl<'a> SceneBindings<'a> {
    pub(super) fn new(
        scenes: &'a HashMap<SceneId, SceneDefinition>,
        scene: &'a SceneDefinition,
        values: Option<&'a PropertyValues>,
    ) -> Self {
        Self {
            scenes,
            scene,
            evaluation: scene.variable_evaluator(values),
        }
    }

    /// Resolve instance arguments only when a child output needs them. Errors
    /// follow the same connection, while previews retain destination fallbacks.
    pub(super) fn for_instance(
        scenes: &'a HashMap<SceneId, SceneDefinition>,
        scene: &'a SceneDefinition,
        instance: &'a TimelineItem,
        time: Option<TimelineTime>,
        parent: Option<&'a SceneBindings<'a>>,
    ) -> Self {
        Self {
            scenes,
            scene,
            evaluation: super::scene::VariableEvaluator::new(scene, move |variable| {
                resolve_property(
                    parent,
                    instance,
                    None,
                    variable.argument_schema().expect("instance argument"),
                    time,
                )
                .ok_or_else(|| format!("Missing instance argument '{}'", variable.id()))?
                .into_result()
            }),
        }
    }

    fn resolve(&self, address: &PropertyAddress, resolved: &mut ResolvedProperty) {
        for variable in self.scene.variables() {
            for binding in variable.bindings.iter().filter(|binding| {
                binding.matches(address.item_id, address.effect_id, &address.property_id)
            }) {
                let input = match self.evaluation.get(variable.id()) {
                    Ok(input) => input,
                    Err(error) => {
                        resolved.diagnostics.push(format!(
                            "Scene '{}' variable '{}' (item {}): {error}",
                            self.scene.name,
                            variable.label(),
                            address.item_id.get(),
                        ));
                        continue;
                    }
                };
                let Some(target) = resolve_scene_binding(self.scenes, self.scene, binding) else {
                    continue;
                };
                let Some(input) = target.schema.constrained_value(&input) else {
                    continue;
                };
                resolved.value = resolved
                    .value
                    .replaced_at(binding.element_id(), binding.scalar_index(), input)
                    .expect("validated scene binding must remain applicable during evaluation");
            }
        }
    }
}

pub(super) fn resolve_property(
    arguments: Option<&SceneBindings<'_>>,
    item: &TimelineItem,
    effect: Option<EffectInstanceId>,
    property: &PropertySchema,
    time: Option<TimelineTime>,
) -> Option<ResolvedProperty> {
    let mut resolved = ResolvedProperty {
        value: property.resolve_value(item.property_values(effect)?.property(property.id())),
        diagnostics: Vec::new(),
    };
    if let Some(arguments) = arguments {
        arguments.resolve(
            &PropertyAddress {
                item_id: item.id,
                effect_id: effect,
                property_id: property.id().to_owned(),
                element_id: None,
                scalar_index: None,
            },
            &mut resolved,
        );
    }
    if let Some(time) = time {
        resolved.value =
            item.animations(effect)?
                .evaluate_property(resolved.value, property, |track| {
                    item.animation_clock(track).local_frame_at(time)
                });
    }
    Some(resolved)
}

pub(super) fn resolve_item(
    scenes: &HashMap<SceneId, SceneDefinition>,
    arguments: Option<&SceneBindings<'_>>,
    source: &TimelineItem,
    time: Option<TimelineTime>,
) -> TimelineItem {
    resolve_item_properties(scenes, arguments, source, time, true).value
}

/// Scene arguments belong to child evaluation, not to the renderer's item ABI.
pub(super) fn resolve_render_item(
    scenes: &HashMap<SceneId, SceneDefinition>,
    arguments: Option<&SceneBindings<'_>>,
    source: &TimelineItem,
    time: TimelineTime,
) -> ResolvedItem {
    resolve_item_properties(scenes, arguments, source, Some(time), false)
}

fn resolve_item_properties(
    scenes: &HashMap<SceneId, SceneDefinition>,
    arguments: Option<&SceneBindings<'_>>,
    source: &TimelineItem,
    time: Option<TimelineTime>,
    instance_arguments: bool,
) -> ResolvedItem {
    let values = |effect, diagnostics: &mut Vec<String>| {
        let mut values = PropertyValues::default();
        let render = source.schema().and_then(|schema| schema.render());
        for property in property_schemas(scenes, source, effect).filter(|property| {
            instance_arguments
                || effect.is_some()
                || render.is_some_and(|render| {
                    render
                        .properties()
                        .iter()
                        .any(|used| used.id() == property.id())
                })
        }) {
            let resolved = resolve_property(arguments, source, effect, property, time)
                .expect("property owner comes from this item");
            diagnostics.extend(resolved.diagnostics);
            values
                .set(property, resolved.value)
                .expect("resolved values preserve the property contract");
        }
        values
    };
    let mut item = source.clone();
    let mut diagnostics = Vec::new();
    if instance_arguments || source.scene_id().is_none() {
        item.properties = values(None, &mut diagnostics);
    }
    for effect in &mut item.effects {
        effect.properties = values(Some(effect.id), &mut diagnostics);
    }
    ResolvedItem {
        value: item,
        diagnostics,
    }
}
