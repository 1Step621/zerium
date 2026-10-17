//! Tracks indexed by property coordinates, with collection edits and evaluation.
use super::{RepeatMode, ScalarTrack};
use crate::property::{PropertyPath, PropertySchema, PropertyValue};
use crate::timeline::{Frame, FrameDuration};
use std::collections::{BTreeMap, btree_map::Entry};

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

    pub(crate) fn trim(&mut self, offset: i128, duration: FrameDuration) {
        for track in self.tracks.values_mut() {
            let repeat = track.repeat();
            if repeat.mode() != RepeatMode::None {
                track.set_repeat(repeat.shifted(offset), duration);
            } else {
                track.trim(offset, duration.animation_span());
            }
        }
    }

    pub(crate) fn stretch(&mut self, old_duration: FrameDuration, duration: FrameDuration) {
        for track in self.tracks.values_mut() {
            track.set_repeat(
                track.repeat().scaled(duration.get(), old_duration.get()),
                duration,
            );
        }
    }

    /// Rehome copied effects onto an item with a different duration.
    pub(crate) fn fit_duration(&mut self, duration: FrameDuration) {
        for track in self.tracks.values_mut() {
            track.set_repeat(track.repeat(), duration);
        }
    }

    pub(crate) fn extend_at(&mut self, local_frame: Frame, duration: FrameDuration) {
        for track in self.tracks.values_mut() {
            if track.repeat().mode() == RepeatMode::None {
                track.extend_at(local_frame, duration.animation_span());
            }
        }
    }

    pub(crate) fn evaluate_property(
        &self,
        mut value: PropertyValue,
        property: &PropertySchema,
        frame: impl Fn(&ScalarTrack) -> f64,
    ) -> PropertyValue {
        let mut changed = false;
        let first = PropertyPath::new(property.id(), None, None);
        for (address, track) in self
            .tracks
            .range(first..)
            .take_while(|(address, _)| address.property_id() == property.id())
        {
            let Some(scalar) = value.scalar_mut(address.element_id(), address.scalar_index())
            else {
                continue;
            };
            let Some(animated) = track.evaluate(frame(track)) else {
                continue;
            };
            *scalar = animated;
            changed = true;
        }
        if changed && let Some(constrained) = property.constrained_value(&value) {
            value = constrained;
        }
        value
    }
}
