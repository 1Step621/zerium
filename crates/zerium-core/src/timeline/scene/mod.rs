//! Scene definitions, their exposed arguments, and instance expansion.
mod argument;
mod binding;
mod expansion;

pub(in crate::timeline) use argument::unique_scene_argument_name;
pub use argument::{SceneArgument, SceneArgumentPreset};
pub(crate) use binding::apply_scene_binding_to_item;
pub use binding::{SceneBindingOwner, SceneBindingTarget, resolve_scene_binding};

use super::{
    Frame, FrameDuration, ItemId, LayerId, SceneId, TimelineDocument, TimelineItem, TimelineTime,
    properties::{SceneArguments, resolve_item},
    property_address::resolve_property_schema,
};
use std::collections::HashMap;

#[derive(Clone)]
pub struct SceneDefinition {
    pub id: SceneId,
    pub name: String,
    pub arguments: Vec<SceneArgument>,
    document: TimelineDocument,
    stretch_point: Frame,
    next_argument_id: Option<u64>,
}

impl SceneDefinition {
    /// Apply arguments to detached items. None selects the scene's defaults when
    /// editing or visiting the definition independently of a placed instance.
    pub(crate) fn apply_arguments<'a>(
        &self,
        scenes: &HashMap<SceneId, SceneDefinition>,
        instance: Option<&TimelineItem>,
        items: impl IntoIterator<Item = &'a mut TimelineItem>,
    ) {
        let arguments = SceneArguments::new(scenes, self, instance.map(|item| &item.properties));
        for item in items {
            let resolved = resolve_item(scenes, Some(arguments), item, None);
            // Only connected inputs become stored values. Other scene inputs
            // must keep inheriting defaults when this detached item is copied.
            for binding in self
                .arguments
                .iter()
                .flat_map(|argument| &argument.bindings)
                .filter(|binding| binding.item_id() == resolved.id)
            {
                let effect = binding.owner().effect_id();
                let Some(property) =
                    resolve_property_schema(scenes, &resolved, effect, binding.property_id())
                else {
                    continue;
                };
                let value = resolved
                    .property_values(effect)
                    .and_then(|values| values.property(property.id()))
                    .expect("resolved property owner must exist");
                item.property_values_mut(effect)
                    .expect("resolved property owner must exist")
                    .set(property, value.clone())
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

    /// Expand a resolved instance and apply its arguments to detached children.
    pub(super) fn instance_items(
        &self,
        scenes: &HashMap<SceneId, SceneDefinition>,
        instance: &TimelineItem,
        time: Option<TimelineTime>,
    ) -> Vec<(LayerId, TimelineItem)> {
        let expansion = expansion::SceneExpansionPlan::new(
            self.duration(),
            instance.duration,
            self.stretch_point,
            self.document.frame_rate(),
        );
        let mut items = match time {
            Some(time) => self
                .document
                .active_source_items_at_time(expansion.source_time(time)),
            None => self.document.source_items(),
        };
        for (_, item) in &mut items {
            expansion.apply(item);
        }
        self.apply_arguments(
            scenes,
            Some(instance),
            items.iter_mut().map(|(_, item)| item),
        );
        items
    }

    pub fn items(&self) -> impl Iterator<Item = &TimelineItem> {
        self.document.items()
    }

    pub fn arguments(&self) -> impl Iterator<Item = &SceneArgument> {
        self.arguments.iter()
    }

    pub fn argument(&self, id: &str) -> Option<&SceneArgument> {
        self.arguments
            .iter()
            .find(|argument| argument.schema.id() == id)
    }

    pub fn argument_mut(&mut self, id: &str) -> Option<&mut SceneArgument> {
        self.arguments
            .iter_mut()
            .find(|argument| argument.schema.id() == id)
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

    pub(super) fn document_mut(&mut self) -> &mut TimelineDocument {
        &mut self.document
    }

    pub(super) fn allocate_argument_id(&mut self) -> Option<(String, u64)> {
        let ordinal = self.next_argument_id?;
        self.next_argument_id = ordinal.checked_add(1).filter(|id| *id != u64::MAX);
        Some((format!("argument_{ordinal}"), ordinal))
    }

    pub fn from_project(
        id: SceneId,
        name: String,
        arguments: Vec<SceneArgument>,
        document: TimelineDocument,
    ) -> Self {
        let next_argument_id = arguments
            .iter()
            .filter_map(|argument| argument.schema.id().strip_prefix("argument_"))
            .filter_map(|id| id.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .filter(|id| *id != u64::MAX);
        let duration = document.end_frame_exclusive();
        let stretch_point = Frame::new(duration.get() / 2);
        Self {
            id,
            name,
            arguments,
            document,
            stretch_point,
            next_argument_id,
        }
    }
}
