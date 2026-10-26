use crate::{
    property::PropertyValue,
    timeline::{
        AspectRatio, EffectInstanceId, ItemId, PropertyAddress, TimelineEditError, TimelineEditor,
        TimelineTime,
        editing::{
            gesture::{EditContext, EditSession, UndoPolicy},
            history::{EditImpact, HistoryKey},
        },
        model::document::ItemEdit,
    },
};
use std::{collections::HashMap, ops::RangeInclusive};

enum PropertyEdit {
    Set(PropertyValue),
    Reset(Option<PropertyValue>),
}

impl TimelineEditor {
    pub fn update_property(&mut self, address: &PropertyAddress, value: PropertyValue) -> bool {
        self.edit_property(address, value).unwrap_or(false)
    }

    pub fn edit_property(
        &mut self,
        address: &PropertyAddress,
        value: PropertyValue,
    ) -> Result<bool, TimelineEditError> {
        self.apply_property_edit(address, PropertyEdit::Set(value))
    }

    pub fn reset_property(
        &mut self,
        address: &PropertyAddress,
        default: Option<PropertyValue>,
    ) -> Result<bool, TimelineEditError> {
        self.apply_property_edit(address, PropertyEdit::Reset(default))
    }

    fn apply_property_edit(
        &mut self,
        address: &PropertyAddress,
        edit: PropertyEdit,
    ) -> Result<bool, TimelineEditError> {
        if !self.is_item_selected(address.item_id) {
            return Ok(false);
        }
        let mut candidate = self.active_document().prepare_item_edit(address.item_id)?;
        if !self.prepare_property_edit(address, edit, &mut candidate) {
            return Ok(false);
        }
        let key = HistoryKey::Property(
            address.item_id,
            address.effect_id,
            address.property_id.clone(),
        );
        self.commit_item_edits(Some(key), [candidate])
    }

    pub(super) fn prepare_property_value(
        &self,
        address: &PropertyAddress,
        value: PropertyValue,
        candidate: &mut crate::timeline::model::document::ItemEdit,
    ) -> bool {
        self.prepare_property_edit(address, PropertyEdit::Set(value), candidate)
    }

    fn prepare_property_edit(
        &self,
        address: &PropertyAddress,
        edit: PropertyEdit,
        candidate: &mut crate::timeline::model::document::ItemEdit,
    ) -> bool {
        let id = address.item_id;
        let effect = address.effect_id;
        if !self.is_item_selected(id) {
            return false;
        }
        let item = &candidate.item;
        let Some(schema) = self.property_schema(address) else {
            return false;
        };
        let Some(current) = item.authored_property(effect, schema) else {
            return false;
        };
        let is_root = address.element_id.is_none() && address.scalar_index.is_none();
        let (root_reset, inherit) = match &edit {
            PropertyEdit::Reset(default) => (is_root, is_root && default.is_none()),
            PropertyEdit::Set(_) => (false, false),
        };
        let value = match edit {
            PropertyEdit::Set(value) => Some(value),
            PropertyEdit::Reset(default) => default
                .unwrap_or_else(|| schema.default_value())
                .element(address.element_id)
                .and_then(|value| match address.scalar_index {
                    Some(index) => value.scalar_at(Some(index)),
                    None => Some(value),
                })
                .cloned(),
        };
        let Some(mut next) = value
            .and_then(|value| current.replaced_at(address.element_id, address.scalar_index, value))
        else {
            return false;
        };
        if !schema.is_editable(address.scalar_index) {
            return false;
        }
        if let Some(ratio) = item
            .aspect_ratio(effect)
            .filter(|_| !root_reset)
            .filter(|_| {
                item.aspect_lock_property(effect)
                    .is_some_and(|property| property.id() == address.property_id)
            })
        {
            if !schema.is_editable(None) {
                return false;
            }
            let Some(constrained) = ratio.constrain(schema, &next, address.scalar_index) else {
                return false;
            };
            next = constrained;
        }
        if !schema.accepts_value(&next)
            || !self.value_preserves_active_bindings(id, effect, &address.property_id, &next)
        {
            return false;
        }
        if root_reset
            && item.aspect_ratio(effect).is_some()
            && item
                .aspect_lock_property(effect)
                .is_some_and(|property| property.id() == schema.id())
        {
            candidate
                .item
                .set_aspect_ratio(effect, AspectRatio::from_value(&next));
        }
        candidate.set_property(
            effect,
            schema,
            (!inherit).then_some(next),
            self.frame_rate(),
        )
    }

    pub fn update_aspect_ratio_locked(
        &mut self,
        id: ItemId,
        effect: Option<EffectInstanceId>,
        locked: bool,
    ) -> bool {
        if !self.is_item_selected(id) {
            return false;
        }
        let item = self.item(id).expect("selected item must exist");
        let Some(property) = item
            .aspect_lock_property(effect)
            .filter(|property| property.is_editable(None))
        else {
            return false;
        };
        let ratio = if locked {
            let Some(ratio) = item.aspect_ratio(effect).or_else(|| {
                AspectRatio::from_value(&item.property_overrides(effect)?.resolve(property))
            }) else {
                return false;
            };
            Some(ratio)
        } else {
            None
        };
        self.edit_project_if_changed(
            EditImpact::Values,
            Some(HistoryKey::AspectRatioLock(id, effect)),
            |editor| {
                editor
                    .active_document_mut()
                    .item_mut(id)
                    .expect("prepared item must exist")
                    .set_aspect_ratio(effect, ratio)
            },
        )
    }
}

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
    session: EditSession,
}

impl ValueEdit {
    /// Read the current value of a target resolved when this operation began.
    pub fn value(&self, editor: &TimelineEditor, index: usize) -> Option<PropertyValue> {
        self.is_current(editor).then_some(())?;
        self.targets.get(index)?.value(editor)
    }

    pub fn is_current(&self, editor: &TimelineEditor) -> bool {
        self.session.is_current(editor)
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
        let policy = if explicit_gesture {
            UndoPolicy::Gesture
        } else {
            UndoPolicy::Coalesced(targets[0].history_key())
        };
        Some(ValueEdit {
            session: EditSession::new(self, policy),
            targets,
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
        let key = edit.targets[0].history_key();
        self.update_edit_session(&mut edit.session, |editor| {
            editor
                .commit_item_edits(Some(key), candidates)
                .unwrap_or(false)
        })
    }
}
