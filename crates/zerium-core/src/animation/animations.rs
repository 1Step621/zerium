//! Tracks indexed by property coordinates, with collection edits and evaluation.
use super::{RepeatMode, ScalarTrack};
use crate::property::{
    PropertyPath, PropertySchema, PropertyValue, PropertyValues, materialized_property_values,
};
use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScalarAnimations {
    tracks: BTreeMap<PropertyPath, ScalarTrack>,
}

impl ScalarAnimations {
    pub fn track(&self, address: &PropertyPath) -> Option<&ScalarTrack> {
        self.tracks.get(address)
    }

    pub fn track_mut(&mut self, address: &PropertyPath) -> Option<&mut ScalarTrack> {
        self.tracks.get_mut(address)
    }

    /// Add a track only when the property has no animation yet.
    pub fn insert(&mut self, address: PropertyPath, track: ScalarTrack) -> bool {
        let Entry::Vacant(entry) = self.tracks.entry(address) else {
            return false;
        };
        entry.insert(track);
        true
    }

    pub fn remove(&mut self, address: &PropertyPath) -> bool {
        self.tracks.remove(address).is_some()
    }

    pub fn tracks(&self) -> impl Iterator<Item = (&PropertyPath, &ScalarTrack)> {
        self.tracks.iter()
    }

    /// Remove invalid tracks only for the property whose structure changed.
    pub fn retain_valid_for_property(
        &mut self,
        property_id: &str,
        value: Option<&PropertyValue>,
    ) -> bool {
        let previous = self.tracks.len();
        self.tracks.retain(|address, _| {
            address.property_id() != property_id
                || value
                    .and_then(|value| value.scalar(address.element_id(), address.scalar_index()))
                    .is_some()
        });
        previous != self.tracks.len()
    }

    pub fn trim(&mut self, offset: f64, old_span: f64, new_span: f64) {
        if !offset.is_finite()
            || !old_span.is_finite()
            || old_span <= 0.
            || !new_span.is_finite()
            || new_span <= 0.
        {
            return;
        }
        for track in self.tracks.values_mut() {
            let repeat = track.repeat();
            if repeat.mode() != RepeatMode::None {
                track.set_repeat(repeat.shifted(offset));
            } else {
                track.remap_time_range(offset / old_span, (offset + new_span) / old_span);
            }
        }
    }

    pub fn stretch(&mut self, factor: f32) {
        if !factor.is_finite() || factor <= 0. {
            return;
        }
        for track in self.tracks.values_mut() {
            track.set_repeat(track.repeat().scaled(factor));
        }
    }

    pub fn evaluated_values<'a>(
        &self,
        base: &PropertyValues,
        schema: impl IntoIterator<Item = &'a PropertySchema>,
        progress: impl Fn(&ScalarTrack) -> f32,
    ) -> PropertyValues {
        let schema = schema.into_iter().collect::<Vec<_>>();
        let mut values = materialized_property_values(base, schema.iter().copied());
        let mut animated_properties = BTreeSet::new();
        for (address, track) in self.tracks() {
            let Some(scalar) = address.value_mut(&mut values) else {
                continue;
            };
            let Some(animated) = track.evaluate(progress(track)) else {
                continue;
            };
            *scalar = animated;
            animated_properties.insert(address.property_id());
        }
        for property in schema
            .into_iter()
            .filter(|property| animated_properties.contains(property.id()))
        {
            let Some(constrained) = values
                .property(property.id())
                .and_then(|value| property.constrained_value(value))
            else {
                continue;
            };
            values
                .set(property, constrained)
                .expect("constrained animation values preserve the property contract");
        }
        values
    }
}
