//! Resolve defaults, scene variables and animation through one property pipeline.

use crate::{
    property::{PropertySchema, PropertyValue, PropertyValues},
    timeline::{
        CalculationDiagnostic, CalculationFailure, EffectInstanceId, PropertyAddress,
        SceneDefinition, SceneId, TimelineItem, TimelineTime,
        model::{
            item::property::{property_schemas, resolve_property_schema},
            scene::resolve_scene_binding,
        },
    },
};

use std::{cell::RefCell, collections::HashMap, sync::Arc};

/// A property's usable preview value and diagnostics from the same resolution.
/// Failed bindings retain their destination's authored value.
pub(in crate::timeline) struct ResolvedProperty {
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

pub(in crate::timeline) struct ResolvedItem {
    pub value: crate::timeline::EvaluatedItem,
    pub diagnostics: Vec<CalculationDiagnostic>,
}

/// Evaluated scene variables shared by all connected properties in one sample.
pub(in crate::timeline) struct SceneBindings<'a> {
    scenes: &'a HashMap<SceneId, SceneDefinition>,
    scene: &'a SceneDefinition,
    evaluation: crate::timeline::model::scene::VariableEvaluator<'a>,
    targets: RefCell<
        HashMap<
            crate::timeline::SceneBindingTarget,
            Option<crate::timeline::model::scene::ResolvedSceneBinding>,
        >,
    >,
}

impl<'a> SceneBindings<'a> {
    pub(in crate::timeline) fn new(
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
    pub(in crate::timeline) fn for_instance(
        scenes: &'a HashMap<SceneId, SceneDefinition>,
        scene: &'a SceneDefinition,
        instance: &'a TimelineItem,
        time: Option<TimelineTime>,
        parent: &'a SceneBindings<'a>,
    ) -> Self {
        Self {
            scenes,
            scene,
            targets: RefCell::default(),
            evaluation: crate::timeline::model::scene::VariableEvaluator::new(
                scene,
                move |variable| {
                    resolve_property(
                        parent,
                        instance,
                        None,
                        variable.argument_schema().expect("instance argument"),
                        time,
                    )
                    .ok_or_else(|| CalculationFailure::MissingArgument(variable.id().to_owned()))?
                    .into_result()
                },
            ),
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

pub(in crate::timeline) fn resolve_property(
    bindings: &SceneBindings<'_>,
    item: &TimelineItem,
    effect: Option<EffectInstanceId>,
    property: &PropertySchema,
    time: Option<TimelineTime>,
) -> Option<ResolvedProperty> {
    let mut resolved = ResolvedProperty {
        value: item.authored_property(effect, property)?,
        diagnostics: Vec::new(),
    };
    bindings.resolve(
        &PropertyAddress {
            item_id: item.id,
            effect_id: effect,
            property_id: property.id().to_owned(),
            element_id: None,
            scalar_index: None,
        },
        &mut resolved,
    );
    if let Some(time) = time {
        resolved.value =
            item.animations(effect)?
                .evaluate_property(resolved.value, property, |track| {
                    item.animation_clock(track).local_frame_at(time)
                });
    }
    Some(resolved)
}

pub(in crate::timeline) fn evaluate_item(
    scenes: &HashMap<SceneId, SceneDefinition>,
    bindings: &SceneBindings<'_>,
    source: &TimelineItem,
    time: Option<TimelineTime>,
    frame_rate: crate::timeline::FrameRate,
) -> crate::timeline::EvaluatedItem {
    resolve_item_properties(
        bindings,
        source,
        property_schemas(scenes, source, None),
        time,
        frame_rate,
    )
    .value
}

/// Scene arguments belong to child evaluation, not to the renderer's item ABI.
pub(in crate::timeline) fn resolve_render_item(
    bindings: &SceneBindings<'_>,
    source: &TimelineItem,
    time: TimelineTime,
    frame_rate: crate::timeline::FrameRate,
) -> ResolvedItem {
    let properties = source
        .schema()
        .and_then(|schema| schema.render())
        .into_iter()
        .flat_map(|render| render.properties());
    resolve_item_properties(bindings, source, properties, Some(time), frame_rate)
}

fn resolve_properties<'a>(
    bindings: &SceneBindings<'_>,
    source: &TimelineItem,
    effect: Option<EffectInstanceId>,
    properties: impl IntoIterator<Item = &'a PropertySchema>,
    time: Option<TimelineTime>,
    diagnostics: &mut Vec<CalculationDiagnostic>,
) -> PropertyValues {
    let mut values = PropertyValues::default();
    for property in properties {
        let resolved = resolve_property(bindings, source, effect, property, time)
            .expect("property owner comes from this item");
        diagnostics.extend(resolved.diagnostics);
        values
            .set(property, resolved.value)
            .expect("resolved values preserve the property contract");
    }
    values
}

fn resolve_item_properties<'a>(
    bindings: &SceneBindings<'_>,
    source: &TimelineItem,
    properties: impl IntoIterator<Item = &'a PropertySchema>,
    time: Option<TimelineTime>,
    frame_rate: crate::timeline::FrameRate,
) -> ResolvedItem {
    let mut diagnostics = Vec::new();
    let properties = resolve_properties(bindings, source, None, properties, time, &mut diagnostics);
    let effects = source
        .effects
        .iter()
        .map(|effect| crate::timeline::EvaluatedEffect {
            id: effect.id,
            plugin_id: effect.plugin_id.clone(),
            properties: resolve_properties(
                bindings,
                source,
                Some(effect.id),
                effect.schema().properties(),
                time,
                &mut diagnostics,
            ),
            schema: effect.schema.clone(),
        })
        .collect();
    ResolvedItem {
        value: crate::timeline::EvaluatedItem {
            runtime_id: source.id,
            source_id: source.id,
            placement: crate::timeline::EvaluatedClip {
                start: source.start(),
                duration: source.duration(frame_rate),
            },
            blend_mode: source.blend_mode,
            kind: source.kind.clone(),
            properties,
            playback: source.playback(),
            effects,
        },
        diagnostics,
    }
}

impl crate::timeline::SceneDefinition {
    /// Apply scene variables to detached items. None selects the scene's defaults when
    /// editing or visiting the definition independently of a placed instance.
    pub(crate) fn apply_variables<'a>(
        &self,
        scenes: &HashMap<SceneId, SceneDefinition>,
        instance: Option<&TimelineItem>,
        items: impl IntoIterator<Item = &'a mut TimelineItem>,
    ) {
        let values = instance.map(|item| {
            item.properties.materialize(
                self.arguments()
                    .filter_map(|argument| argument.argument_schema()),
            )
        });
        let arguments = SceneBindings::new(scenes, self, values.as_ref());
        for item in items {
            let id = item.id;
            // Only connected inputs become stored values. Other scene inputs
            // must keep inheriting defaults when this detached item is copied.
            for binding in self
                .variables()
                .iter()
                .flat_map(|argument| &argument.bindings)
                .filter(|binding| binding.item_id() == id)
            {
                let effect = binding.owner().effect_id();
                let Some(property) =
                    resolve_property_schema(scenes, item, effect, binding.property_id()).cloned()
                else {
                    continue;
                };
                let value = resolve_property(&arguments, item, effect, &property, None)
                    .expect("property owner")
                    .value;
                item.property_overrides_mut(effect)
                    .expect("resolved property owner must exist")
                    .set(&property, value)
                    .expect("resolved values preserve the property contract");
            }
        }
    }
}
