//! Scene definitions, arguments, calculations, and instance expansion.
mod binding;
mod calculation;
mod variable;
pub(in crate::timeline) use self::{binding::ResolvedSceneBinding, calculation::VariableEvaluator};
pub use self::{
    binding::{SceneBindingOwner, SceneBindingTarget},
    calculation::{
        CalculationDiagnostic, CalculationFailure, SceneCalculation, SceneCalculationInput,
        SceneEvaluation,
    },
    variable::{SceneVariable, SceneVariablePreset, SceneVariableSource},
};

pub(crate) use self::binding::{apply_scene_binding_to_item, resolve_scene_binding};

use super::project::ProjectSettingsError;
use crate::timeline::{
    Frame, FrameDuration, FrameRate, ItemId, LayerId, SceneId, TimelineDocument, TimelineItem,
    TimelineTime,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct SceneDefinition {
    pub id: SceneId,
    pub name: String,
    variables: Vec<SceneVariable>,
    runtime: Arc<calculation::SceneRuntime>,
    document: Arc<TimelineDocument>,
    stretch_point: Frame,
}

impl SceneDefinition {
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
    pub(in crate::timeline) fn expanded_items(
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
    pub(in crate::timeline) fn active_expanded_item_ids(
        &self,
        instance: &TimelineItem,
        time: TimelineTime,
    ) -> Vec<ItemId> {
        self.document
            .active_source_item_ids_at_time(self.expansion(instance).source_time(time))
    }

    fn expansion(&self, instance: &TimelineItem) -> SceneExpansionPlan {
        SceneExpansionPlan::new(
            self.duration(),
            instance.duration(self.document.frame_rate()),
            self.stretch_point,
            self.document.frame_rate(),
        )
    }

    /// Expand one placement without enumerating unrelated definition items.
    pub(in crate::timeline) fn expanded_item(
        &self,
        instance: &TimelineItem,
        id: ItemId,
    ) -> Option<(LayerId, TimelineItem)> {
        let mut item = self.document.item(id)?.clone();
        let layer = self.document.item_layer(id)?;
        self.expansion(instance).apply(&mut item);
        Some((layer, item))
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

    pub(in crate::timeline) fn variable_mut(&mut self, id: &str) -> Option<&mut SceneVariable> {
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

    pub fn hidden_layer_ids(&self) -> impl Iterator<Item = LayerId> + '_ {
        self.document.hidden_layer_ids()
    }

    pub(in crate::timeline) fn document(&self) -> &TimelineDocument {
        &self.document
    }

    pub(in crate::timeline) fn retimed(
        &self,
        frame_rate: FrameRate,
    ) -> Result<Self, ProjectSettingsError> {
        let mut scene = self.clone();
        if self.document.frame_rate() != frame_rate {
            let stretch_point = crate::timeline::model::document::retime_frame(
                self.stretch_point(),
                self.document.frame_rate(),
                frame_rate,
            )?;
            scene.document = Arc::new(self.document.retimed(frame_rate)?);
            scene.set_stretch_point(stretch_point);
        }
        Ok(scene)
    }

    pub(in crate::timeline) fn document_mut(&mut self) -> &mut TimelineDocument {
        Arc::make_mut(&mut self.document)
    }

    pub(crate) fn from_project(
        id: SceneId,
        name: String,
        variables: Vec<SceneVariable>,
        document: TimelineDocument,
    ) -> Self {
        let duration = document.end_frame_exclusive();
        let stretch_point = Frame::new(duration.get() / 2);
        Self {
            id,
            name,
            variables,
            runtime: Arc::default(),
            document: Arc::new(document),
            stretch_point,
        }
    }
}

struct SceneExpansionPlan {
    point: Frame,
    extra: u64,
    frame_rate: FrameRate,
}

impl SceneExpansionPlan {
    fn new(
        source_duration: FrameDuration,
        instance_duration: FrameDuration,
        point: Frame,
        frame_rate: FrameRate,
    ) -> Self {
        Self {
            frame_rate,
            point: Frame::new(point.get().min(source_duration.get() - 1)),
            extra: instance_duration
                .get()
                .saturating_sub(source_duration.get()),
        }
    }

    /// Inverse lookup for occupied source intervals only. This is deliberately
    /// not the clock used to sample animation or media inside an expanded item.
    fn source_time(&self, time: TimelineTime) -> TimelineTime {
        let frame = time.frames();
        TimelineTime::from_frames(if frame > self.point.get() as f64 {
            (frame - self.extra as f64).max(self.point.get() as f64)
        } else {
            frame
        })
    }

    fn apply(&self, item: &mut TimelineItem) {
        item.expand_at(self.point, self.extra, self.frame_rate);
    }
}
