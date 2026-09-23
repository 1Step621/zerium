use super::*;

impl TimelineEditor {
    pub(crate) fn set_selected_property_animation_enabled(
        &mut self,
        effect_id: Option<EffectInstanceId>,
        property_id: String,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
        enabled: bool,
    ) -> bool {
        let Some(item_id) = self.selection.primary else {
            return false;
        };
        if enabled
            && self.active_scene_has_binding(item_id, effect_id, &property_id, |binding| {
                binding.conflicts_with_animation(&property_id, element_id, scalar_index)
            })
        {
            return false;
        }
        let before = self.history_snapshot();
        let Some(schema) = self.animation_schema(item_id, effect_id, &property_id) else {
            return false;
        };
        if !schema.is_editable(scalar_index) {
            return false;
        }
        if effect_id.is_none()
            && scalar_index == Some(1)
            && self.active_document().item(item_id).is_some_and(|item| {
                item.preserves_aspect_ratio()
                    && item
                        .schema()
                        .is_some_and(|schema| schema.is_size_property(&property_id))
            })
        {
            return false;
        }
        let address = ScalarAnimationAddress::new(property_id.clone(), element_id, scalar_index);
        let changed = if !enabled {
            self.animation_store_mut(item_id, effect_id)
                .is_some_and(|animations| animations.remove(&address))
        } else {
            if !schema.is_animatable(scalar_index) {
                return false;
            }
            let Some(item) = self.active_document().item(item_id) else {
                return false;
            };
            let values = match effect_id {
                Some(effect_id) => item
                    .effects
                    .iter()
                    .find(|effect| effect.id == effect_id)
                    .map(|effect| effect.properties.clone()),
                None if item.scene_id().is_some() => {
                    materialize_scene_instance_properties(item, &self.project().scenes)
                }
                None => Some(item.properties.clone()),
            };
            let Some(values) = values else {
                return false;
            };
            let Some(value) = values
                .property(&property_id)
                .and_then(|value| value.element(element_id))
                .and_then(|value| value.scalar_at(scalar_index))
            else {
                return false;
            };
            let Some(value_type) = (match (element_id, schema.ty()) {
                (Some(_), PropertyType::Array { element_type, .. })
                | (None, PropertyType::Value(element_type)) => element_type.scalar_at(scalar_index),
                _ => None,
            }) else {
                return false;
            };
            let Some(track) = ScalarTrack::from_value(value.clone(), value_type) else {
                return false;
            };
            self.animation_store_mut(item_id, effect_id)
                .is_some_and(|animations| animations.insert(address, track))
        };
        self.finish_project_edit_if_changed(changed, Some(before), None)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn set_selected_property_animation_stop(
        &mut self,
        effect_id: Option<EffectInstanceId>,
        property_id: String,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
        index: usize,
        value: PropertyValue,
        focused_segment: Option<usize>,
    ) -> bool {
        let Some(item_id) = self.selection.primary else {
            return false;
        };
        let Some(item) = self.active_document().item(item_id) else {
            return false;
        };
        let address = ScalarAnimationAddress::new(property_id.clone(), element_id, scalar_index);
        let Some(position) = self
            .animation_track(item_id, effect_id, &address)
            .and_then(|track| track.stops().get(index))
            .map(|stop| stop.position())
        else {
            return false;
        };
        let stop_frame = Frame::new(item.animation_timeline_frame(position).round().max(0.) as u64);
        let key = HistoryKey::AnimationStopValue(
            item_id,
            effect_id,
            property_id.clone(),
            element_id,
            scalar_index,
            stop_frame,
        );
        let before = self.history_snapshot_for_edit(Some(&key));
        let Some(schema) = self.animation_schema(item_id, effect_id, &property_id) else {
            return false;
        };
        let changed = schema.is_editable(scalar_index)
            && schema
                .configuration_constraints(scalar_index)
                .allows(&value)
            && self
                .animation_track_mut(item_id, effect_id, &address)
                .is_some_and(|animation| animation.set_stop(index, value, focused_segment));
        self.finish_project_edit_if_changed(changed, before, Some(key))
    }

    pub(crate) fn set_selected_property_animation_pair_stop_at(
        &mut self,
        property_id: &str,
        element_id: Option<PropertyElementId>,
        position: f32,
        value: [f32; 2],
    ) -> bool {
        let Some(item_id) = self.selection.primary else {
            return false;
        };
        let Some(item) = self.active_document().item(item_id) else {
            return false;
        };
        let stop_frame = Frame::new(item.animation_timeline_frame(position).round().max(0.) as u64);
        let key = HistoryKey::AnimationPairStopValue(
            item_id,
            property_id.to_owned(),
            element_id,
            stop_frame,
        );
        let before = self.history_snapshot_for_edit(Some(&key));
        let Some(schema) = self.animation_schema(item_id, None, property_id) else {
            return false;
        };
        let editable = [0_usize, 1].map(|scalar_index| {
            schema.is_editable(Some(scalar_index))
                && schema
                    .configuration_constraints(Some(scalar_index))
                    .allows(&PropertyValue::F32(value[scalar_index]))
        });
        let mut changed = false;
        for scalar_index in 0..2 {
            if !editable[scalar_index] {
                continue;
            }
            let address = ScalarAnimationAddress::new(property_id, element_id, Some(scalar_index));
            let Some(index) = self
                .animation_track(item_id, None, &address)
                .and_then(|track| track.stop_index_at(position))
            else {
                continue;
            };
            changed |= self
                .animation_track_mut(item_id, None, &address)
                .is_some_and(|track| {
                    track.set_stop_exact(index, PropertyValue::F32(value[scalar_index]))
                });
        }
        self.finish_project_edit_if_changed(changed, before, Some(key))
    }

    pub(crate) fn insert_selected_property_animation_stop(
        &mut self,
        effect_id: Option<EffectInstanceId>,
        property_id: String,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
        position: f32,
        value: PropertyValue,
    ) -> Option<usize> {
        let item_id = self.selection.primary?;
        let stop_frame = self
            .active_document()
            .item(item_id)
            .map(|item| Frame::new(item.animation_timeline_frame(position).round().max(0.) as u64))
            .unwrap_or(self.playhead);
        let key = HistoryKey::AnimationStopValue(
            item_id,
            effect_id,
            property_id.clone(),
            element_id,
            scalar_index,
            stop_frame,
        );
        let before = self.history_snapshot_for_edit(Some(&key));
        let inserted = self
            .animation_schema(item_id, effect_id, &property_id)
            .filter(|schema| {
                schema.is_editable(scalar_index)
                    && schema
                        .configuration_constraints(scalar_index)
                        .allows(&value)
            })
            .and_then(|_| {
                self.animation_track_mut(
                    item_id,
                    effect_id,
                    &ScalarAnimationAddress::new(property_id, element_id, scalar_index),
                )
            })
            .and_then(|animation| animation.insert_stop(position, value));
        if inserted.is_some() {
            self.finish_project_edit(before, Some(key));
        }
        inserted
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn set_selected_animation_handle(
        &mut self,
        effect_id: Option<EffectInstanceId>,
        property_id: String,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
        segment: usize,
        handle: BezierHandle,
        position: [f32; 2],
    ) -> bool {
        let Some(item_id) = self.selection.primary else {
            return false;
        };
        let key = HistoryKey::AnimationHandle(
            item_id,
            effect_id,
            property_id.clone(),
            element_id,
            scalar_index,
            segment,
            handle,
        );
        let before = self.history_snapshot_for_edit(Some(&key));
        self.edit_selected_animation(
            item_id,
            effect_id,
            ScalarAnimationAddress::new(property_id.as_str(), element_id, scalar_index),
            before,
            Some(key),
            |animation| animation.set_segment_handle(segment, handle, position),
        )
    }

    pub(crate) fn set_selected_animation_interpolation(
        &mut self,
        effect_id: Option<EffectInstanceId>,
        property_id: String,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
        segment: usize,
        interpolation: SegmentInterpolation,
    ) -> bool {
        let Some(item_id) = self.selection.primary else {
            return false;
        };
        let before = self.history_snapshot();
        self.edit_selected_animation(
            item_id,
            effect_id,
            ScalarAnimationAddress::new(property_id.as_str(), element_id, scalar_index),
            Some(before),
            None,
            |animation| animation.set_segment_interpolation(segment, interpolation),
        )
    }

    pub(crate) fn remove_selected_animation_stop(
        &mut self,
        effect_id: Option<EffectInstanceId>,
        property_id: String,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
        stop: usize,
    ) -> bool {
        let Some(item_id) = self.selection.primary else {
            return false;
        };
        let before = self.history_snapshot();
        self.edit_selected_animation(
            item_id,
            effect_id,
            ScalarAnimationAddress::new(property_id.as_str(), element_id, scalar_index),
            Some(before),
            None,
            |animation| animation.remove_stop(stop),
        )
    }

    pub(crate) fn move_selected_animation_stop(
        &mut self,
        effect_id: Option<EffectInstanceId>,
        property_id: String,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
        stop: usize,
        position: f32,
    ) -> bool {
        let Some(item_id) = self.selection.primary else {
            return false;
        };
        let key = HistoryKey::AnimationStopPosition(
            item_id,
            effect_id,
            property_id.clone(),
            element_id,
            scalar_index,
            stop,
        );
        let before = self.history_snapshot_for_edit(Some(&key));
        self.edit_selected_animation(
            item_id,
            effect_id,
            ScalarAnimationAddress::new(property_id.as_str(), element_id, scalar_index),
            before,
            Some(key),
            |animation| animation.move_stop(stop, position),
        )
    }
}
