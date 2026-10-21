//! Resolve defaults, scene variables and animation through one property pipeline.

use std::{cell::RefCell, collections::HashMap, sync::Arc};

use super::{CalculationDiagnostic, CalculationFailure};
use crate::property::{PropertySchema, PropertyValue, PropertyValues};

use super::{
    EffectInstanceId, PropertyAddress, SceneDefinition, SceneId, TimelineItem, TimelineTime,
    property_address::property_schemas, scene::resolve_scene_binding,
};

/// A property's usable preview value and diagnostics from the same resolution.
/// Failed bindings retain their destination's authored value.
pub(super) struct ResolvedProperty {
    pub value: PropertyValue,
    pub diagnostics: Vec<CalculationDiagnostic>,
}

impl ResolvedProperty {
    /// A scene argument propagates a failed input to its downstream consumers.
    fn into_result(self) -> Result<PropertyValue, CalculationFailure> {
        match self.diagnostics.into_iter().next() {
            Some(error) => Err(CalculationFailure::Argument(Arc::new(error))),
            None => Ok(self.value),
        }
    }
}

pub(super) struct ResolvedItem {
    pub value: super::EvaluatedItem,
    pub diagnostics: Vec<CalculationDiagnostic>,
}

/// Evaluated scene variables shared by all connected properties in one sample.
pub(super) struct SceneBindings<'a> {
    scenes: &'a HashMap<SceneId, SceneDefinition>,
    scene: &'a SceneDefinition,
    evaluation: super::scene::VariableEvaluator<'a>,
    targets:
        RefCell<HashMap<super::SceneBindingTarget, Option<super::scene::ResolvedSceneBinding>>>,
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
            targets: RefCell::default(),
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
            targets: RefCell::default(),
            evaluation: super::scene::VariableEvaluator::new(scene, move |variable| {
                resolve_property(
                    parent,
                    instance,
                    None,
                    variable.argument_schema().expect("instance argument"),
                    time,
                )
                .ok_or_else(|| CalculationFailure::MissingArgument(variable.id().to_owned()))?
                .into_result()
            }),
        }
    }

    fn resolve(&self, address: &PropertyAddress, resolved: &mut ResolvedProperty) {
        for (variable, binding) in self.scene.bindings_for(address) {
            let input = match self.evaluation.get(variable.id()) {
                Ok(input) => input,
                Err(error) => {
                    resolved.diagnostics.push(CalculationDiagnostic {
                        scene_id: self.scene.id,
                        scene_name: self.scene.name.clone(),
                        variable_id: variable.id().to_owned(),
                        variable_label: variable.label().to_owned(),
                        address: binding.address().clone(),
                        cause: error,
                    });
                    continue;
                }
            };
            let mut targets = self.targets.borrow_mut();
            let Some(target) = targets
                .entry(binding.clone())
                .or_insert_with(|| resolve_scene_binding(self.scenes, self.scene, binding))
                .as_ref()
            else {
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
    let evaluated = evaluate_item(scenes, arguments, source, time);
    let mut materialized = source.clone();
    materialized.properties = evaluated.properties;
    for (effect, values) in materialized.effects.iter_mut().zip(evaluated.effects) {
        effect.properties = values.properties;
    }
    materialized
}

pub(super) fn evaluate_item(
    scenes: &HashMap<SceneId, SceneDefinition>,
    arguments: Option<&SceneBindings<'_>>,
    source: &TimelineItem,
    time: Option<TimelineTime>,
) -> super::EvaluatedItem {
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
    let values = |effect, diagnostics: &mut Vec<CalculationDiagnostic>| {
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
    let mut diagnostics = Vec::new();
    let properties = if instance_arguments || source.scene_id().is_none() {
        values(None, &mut diagnostics)
    } else {
        source.properties.clone()
    };
    let effects = source
        .effects
        .iter()
        .map(|effect| super::EvaluatedEffect {
            id: effect.id,
            plugin_id: effect.plugin_id.clone(),
            effect_id: effect.effect_id.clone(),
            properties: values(Some(effect.id), &mut diagnostics),
            schema: effect.schema.clone(),
            aspect_ratio: effect.aspect_ratio,
        })
        .collect();
    ResolvedItem {
        value: super::EvaluatedItem {
            runtime_id: source.id,
            source_id: source.id,
            placement: super::EvaluatedClip {
                start: source.start,
                duration: source.duration,
            },
            blend_mode: source.blend_mode,
            kind: source.kind.clone(),
            properties,
            effects,
            aspect_ratio: source.aspect_ratio,
        },
        diagnostics,
    }
}
