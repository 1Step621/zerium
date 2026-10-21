//! Resolve value targets once; an operation owns their lifetime and Undo policy.

use std::{collections::HashMap, ops::RangeInclusive};

use crate::property::PropertyValue;
use crate::timeline::{
    PropertyAddress, TimelineEditor, TimelineTime, document::ItemEdit, history::HistoryKey,
};

use super::EditContext;

/// Immutable targets of a displayed key or interval endpoint. Equal neighbors
/// are resolved before editing, so crossing another value cannot recruit keys.
#[derive(Clone)]
pub struct AnimationValuePlan {
    pub value: PropertyValue,
    address: PropertyAddress,
    index: usize,
    segment: Option<usize>,
    targets: RangeInclusive<usize>,
    resolved_at: EditContext,
}

impl AnimationValuePlan {
    pub fn address(&self) -> &PropertyAddress {
        &self.address
    }

    pub fn index(&self) -> usize {
        self.index
    }

    fn same_scope(&self, other: &Self) -> bool {
        self.address == other.address && self.index == other.index && self.segment == other.segment
    }

    fn prepare_value(
        &self,
        editor: &TimelineEditor,
        candidate: &mut ItemEdit,
        value: &PropertyValue,
    ) -> bool {
        let address = &self.address;
        if !editor.property_schema(address).is_some_and(|schema| {
            schema.is_editable(address.scalar_index)
                && schema
                    .scalar_type(address.element_id, address.scalar_index)
                    .is_some_and(|ty| ty.allows(value))
                && schema
                    .configuration_constraints(address.scalar_index)
                    .allows(value)
        }) {
            return false;
        }
        let Some(track) = candidate
            .item
            .animations_mut(address.effect_id)
            .and_then(|animations| animations.track_mut(&address.path()))
        else {
            return false;
        };
        if track.stops().get(*self.targets.end()).is_none() {
            return false;
        }
        for index in self.targets.clone() {
            track.set_stop(index, value.clone());
        }
        true
    }
}

pub enum ValueEditTarget {
    Property(PropertyAddress),
    Animation(AnimationValuePlan),
}

impl ValueEditTarget {
    fn address(&self) -> &PropertyAddress {
        match self {
            Self::Property(address) => address,
            Self::Animation(plan) => &plan.address,
        }
    }

    fn history_key(&self) -> HistoryKey {
        match self {
            Self::Property(address) => HistoryKey::Property(
                address.item_id,
                address.effect_id,
                address.property_id.clone(),
            ),
            Self::Animation(plan) => {
                HistoryKey::AnimationStopValue(plan.address.clone(), plan.index)
            }
        }
    }

    fn value(&self, editor: &TimelineEditor) -> Option<PropertyValue> {
        match self {
            Self::Property(address) => editor
                .evaluated_property_value(address, TimelineTime::from_frame(editor.playhead())),
            Self::Animation(plan) => editor
                .animation_track(&plan.address)?
                .stops()
                .get(plan.index)
                .map(|stop| stop.value().clone()),
        }
    }

    fn prepare_value(
        &self,
        editor: &TimelineEditor,
        candidate: &mut ItemEdit,
        value: &PropertyValue,
    ) -> bool {
        match self {
            Self::Property(address) => {
                editor.prepare_property_value(address, value.clone(), candidate)
            }
            Self::Animation(plan) => plan.prepare_value(editor, candidate, value),
        }
    }
}

/// One value operation, shared by inspector inputs and multi-axis preview handles.
/// Plans stay fixed while successful updates advance only this operation's revision.
pub struct ValueEdit {
    targets: Vec<ValueEditTarget>,
    context: EditContext,
    key: HistoryKey,
    explicit_gesture: bool,
    has_changes: bool,
}

impl ValueEdit {
    /// Read the current value of a target resolved when this operation began.
    pub fn value(&self, editor: &TimelineEditor, index: usize) -> Option<PropertyValue> {
        self.is_current(editor).then_some(())?;
        self.targets.get(index)?.value(editor)
    }

    pub fn is_current(&self, editor: &TimelineEditor) -> bool {
        self.context == editor.edit_context()
            && (!self.explicit_gesture
                || !self.has_changes
                || editor
                    .history
                    .is_current_group(&(self.context.scene_id, self.key.clone())))
    }

    pub fn matches_animation(&self, plan: &AnimationValuePlan) -> bool {
        matches!(self.targets.as_slice(), [ValueEditTarget::Animation(current)] if current.same_scope(plan))
    }

    fn prepare_values(
        &self,
        editor: &TimelineEditor,
        values: &[PropertyValue],
    ) -> Option<Vec<ItemEdit>> {
        let mut candidates = HashMap::new();
        for (target, value) in self.targets.iter().zip(values) {
            let address = target.address();
            if !editor.is_item_selected(address.item_id) {
                return None;
            }
            if let std::collections::hash_map::Entry::Vacant(entry) =
                candidates.entry(address.item_id)
            {
                let Ok(candidate) = editor.active_document().prepare_item_edit(address.item_id)
                else {
                    return None;
                };
                entry.insert(candidate);
            }
            let candidate: &mut ItemEdit = candidates
                .get_mut(&address.item_id)
                .expect("candidate was prepared");
            if !target.prepare_value(editor, candidate, value) {
                return None;
            }
        }
        Some(candidates.into_values().collect())
    }
}

impl TimelineEditor {
    pub fn property_animation_stops(
        &self,
        address: &PropertyAddress,
        time: TimelineTime,
    ) -> Option<Vec<AnimationValuePlan>> {
        let item = self.item(address.item_id)?;
        let track = self.animation_track(address)?;
        let indices =
            track.stop_indices_for_segment(item.animation_clock(track).local_frame_at(time));
        let segment = (indices.len() == 2).then(|| indices[0]);
        indices
            .into_iter()
            .map(|index| {
                let mut plan = self.property_animation_stop(address, index)?;
                plan.segment = segment;
                plan.targets = track.value_edit_targets(index, segment)?;
                Some(plan)
            })
            .collect()
    }

    pub fn property_animation_stop(
        &self,
        address: &PropertyAddress,
        index: usize,
    ) -> Option<AnimationValuePlan> {
        let stop = self.animation_track(address)?.stops().get(index)?;
        Some(AnimationValuePlan {
            value: stop.value().clone(),
            address: address.clone(),
            index,
            segment: None,
            targets: index..=index,
            resolved_at: self.edit_context(),
        })
    }

    /// Separate inspector operations coalesce by authored key within the normal
    /// timeout; a pause within an operation never splits its Undo entry.
    pub fn begin_animation_value_edit(&self, plan: AnimationValuePlan) -> Option<ValueEdit> {
        self.begin_value_edit(vec![ValueEditTarget::Animation(plan)], false)
    }

    /// All values of a preview gesture share one explicit Undo boundary.
    pub fn begin_value_gesture(&self, targets: Vec<ValueEditTarget>) -> Option<ValueEdit> {
        self.begin_value_edit(targets, true)
    }

    fn begin_value_edit(
        &self,
        targets: Vec<ValueEditTarget>,
        explicit_gesture: bool,
    ) -> Option<ValueEdit> {
        let context = self.edit_context();
        if targets.is_empty() || targets.iter().any(|target| {
            !self.is_item_selected(target.address().item_id)
                || matches!(target, ValueEditTarget::Animation(plan) if plan.resolved_at != context)
        }) {
            return None;
        }
        let key = if explicit_gesture {
            HistoryKey::Gesture(context.revision)
        } else {
            targets[0].history_key()
        };
        Some(ValueEdit {
            targets,
            context,
            key,
            explicit_gesture,
            has_changes: false,
        })
    }

    pub fn update_value_edit(&mut self, edit: &mut ValueEdit, values: &[PropertyValue]) -> bool {
        if !edit.is_current(self)
            || self.history_group.is_some()
            || values.len() != edit.targets.len()
        {
            return false;
        }
        let Some(candidates) = edit.prepare_values(self, values) else {
            return false;
        };
        let key = edit.key.clone();
        if edit.explicit_gesture
            || (edit.has_changes
                && self
                    .history
                    .is_current_group(&(self.active_scene_id(), key.clone())))
        {
            self.history_group = Some(key.clone());
        }
        let changed = self
            .try_edit_project(Some(key), |editor| {
                let changed = editor.active_document_mut().commit_item_edits(candidates)?;
                Ok::<_, super::TimelineEditError>((changed, changed))
            })
            .unwrap_or(false);
        self.history_group = None;
        if changed {
            edit.context = self.edit_context();
            edit.has_changes = true;
        }
        changed
    }
}
