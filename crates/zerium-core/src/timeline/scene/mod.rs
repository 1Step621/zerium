//! Scene definitions, arguments, calculations, and instance expansion.
mod binding;
mod calculation;
mod variable;
pub(in crate::timeline) use calculation::VariableEvaluator;
pub use calculation::{SceneCalculation, SceneCalculationInput, SceneEvaluation};
mod expansion;

pub(in crate::timeline) use binding::ResolvedSceneBinding;
pub(crate) use binding::apply_scene_binding_to_item;
pub use binding::{SceneBindingOwner, SceneBindingTarget, resolve_scene_binding};
pub(in crate::timeline) use variable::unique_scene_variable_name;
pub use variable::{SceneVariable, SceneVariablePreset, SceneVariableSource};

use super::{
    Frame, FrameDuration, FrameRate, ItemId, LayerId, SceneId, TimelineDocument, TimelineItem,
    TimelineTime,
    properties::{SceneBindings, resolve_property},
    property_address::resolve_property_schema,
    settings::ProjectSettingsError,
};
use std::{collections::HashMap, sync::Arc};

#[derive(Clone)]
pub struct SceneDefinition {
    pub id: SceneId,
    pub name: String,
    variables: Vec<SceneVariable>,
    runtime: Arc<calculation::SceneRuntime>,
    document: Arc<TimelineDocument>,
    stretch_point: Frame,
    next_variable_ordinal: Option<u64>,
}

impl SceneDefinition {
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
                .variables
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
                let value = resolve_property(Some(&arguments), item, effect, &property, None)
                    .expect("property owner")
                    .value;
                item.property_overrides_mut(effect)
                    .expect("resolved property owner must exist")
                    .set(&property, value)
                    .expect("resolved values preserve the property contract");
            }
        }
    }

    pub fn duration(&self) -> FrameDuration {
        FrameDuration::new_saturating(self.document.end_frame_exclusive().get())
    }

    pub fn stretch_point(&self) -> Frame {
        Frame::new(self.stretch_point.get().min(self.duration().get() - 1))
    }

    pub(crate) fn set_stretch_point(&mut self, point: Frame) -> bool {
        let point = Frame::new(point.get().min(self.duration().get() - 1));
        if self.stretch_point == point {
            return false;
        }
        self.stretch_point = point;
        true
    }

    /// Placement and clocks are independent of property and diagnostic resolution.
    pub(super) fn expanded_items(
        &self,
        instance: &TimelineItem,
        time: Option<TimelineTime>,
    ) -> Vec<(LayerId, TimelineItem)> {
        let expansion = self.expansion(instance);
        let mut items = match time {
            Some(time) => self
                .document
                .active_source_items_at_time(expansion.source_time(time)),
            None => self.document.source_items(),
        };
        for (_, item) in &mut items {
            expansion.apply(item);
        }
        items
    }

    /// Enumerate occupied placements without cloning or expanding their contents.
    pub(super) fn active_expanded_item_ids(
        &self,
        instance: &TimelineItem,
        time: TimelineTime,
    ) -> Vec<ItemId> {
        self.document
            .active_source_item_ids_at_time(self.expansion(instance).source_time(time))
    }

    fn expansion(&self, instance: &TimelineItem) -> expansion::SceneExpansionPlan {
        expansion::SceneExpansionPlan::new(
            self.duration(),
            instance.duration(self.document.frame_rate()),
            self.stretch_point,
            self.document.frame_rate(),
        )
    }

    /// Expand one placement without enumerating unrelated definition items.
    pub(super) fn expanded_item(
        &self,
        instance: &TimelineItem,
        id: ItemId,
    ) -> Option<(LayerId, TimelineItem)> {
        let mut item = self.document.item(id)?.clone();
        self.expansion(instance).apply(&mut item);
        Some((self.document.item_layer(id)?, item))
    }

    pub fn items(&self) -> impl Iterator<Item = &TimelineItem> {
        self.document.items()
    }

    pub fn variables(&self) -> &[SceneVariable] {
        &self.variables
    }

    pub(in crate::timeline) fn variables_mut(&mut self) -> &mut Vec<SceneVariable> {
        self.runtime = Arc::default();
        &mut self.variables
    }

    pub fn arguments(&self) -> impl Iterator<Item = &SceneVariable> {
        self.variables
            .iter()
            .filter(|variable| variable.calculation().is_none())
    }

    pub fn argument(&self, id: &str) -> Option<&SceneVariable> {
        self.arguments().find(|argument| argument.id() == id)
    }

    pub fn variable(&self, id: &str) -> Option<&SceneVariable> {
        self.variables.iter().find(|variable| variable.id() == id)
    }

    pub fn variable_mut(&mut self, id: &str) -> Option<&mut SceneVariable> {
        self.variables_mut()
            .iter_mut()
            .find(|variable| variable.id() == id)
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

    pub(super) fn retimed(&self, frame_rate: FrameRate) -> Result<Self, ProjectSettingsError> {
        let mut scene = self.clone();
        if self.document.frame_rate() != frame_rate {
            let stretch_point = super::document::retime_frame(
                self.stretch_point(),
                self.document.frame_rate(),
                frame_rate,
            )?;
            scene.document = Arc::new(self.document.retimed(frame_rate)?);
            scene.set_stretch_point(stretch_point);
        }
        Ok(scene)
    }

    pub(super) fn document_mut(&mut self) -> &mut TimelineDocument {
        Arc::make_mut(&mut self.document)
    }

    pub(super) fn allocate_variable_id(&mut self) -> Option<(String, u64)> {
        let ordinal = self.next_variable_ordinal?;
        self.next_variable_ordinal = ordinal.checked_add(1).filter(|id| *id != u64::MAX);
        let identity = super::ProjectId::generate();
        Some((
            format!("variable_{:016x}{:016x}", identity.high(), identity.low()),
            ordinal,
        ))
    }

    pub fn from_project(
        id: SceneId,
        name: String,
        variables: Vec<SceneVariable>,
        document: TimelineDocument,
    ) -> Self {
        let next_variable_ordinal = u64::try_from(variables.len())
            .ok()
            .and_then(|n| n.checked_add(1));
        let duration = document.end_frame_exclusive();
        let stretch_point = Frame::new(duration.get() / 2);
        Self {
            id,
            name,
            variables,
            runtime: Arc::default(),
            document: Arc::new(document),
            stretch_point,
            next_variable_ordinal,
        }
    }
}
