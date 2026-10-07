//! Apply enabled tracks to property values, enforcing their contracts.
use super::ScalarAnimations;
use crate::property::{PropertySchema, PropertyValues, materialized_property_values};
use std::collections::BTreeSet;

impl ScalarAnimations {
    pub fn evaluated_values<'a>(
        &self,
        base: &PropertyValues,
        schema: impl IntoIterator<Item = &'a PropertySchema>,
        progress: f32,
    ) -> PropertyValues {
        let schema = schema.into_iter().collect::<Vec<_>>();
        let mut values = materialized_property_values(base, schema.iter().copied());
        let mut animated_properties = BTreeSet::new();
        for (address, track) in self.tracks() {
            let property_id = address.property_id();
            if !schema.iter().any(|property| property.id == property_id) {
                continue;
            }
            let Some(animated) = track.evaluate(progress) else {
                continue;
            };
            if let Some(scalar) = address.value_mut(&mut values) {
                *scalar = animated;
                animated_properties.insert(property_id.to_owned());
            }
        }
        for property_id in animated_properties {
            let Some(property) = schema.iter().find(|property| property.id == property_id) else {
                continue;
            };
            let Some(value) = values.property(&property_id).cloned() else {
                continue;
            };
            let Some(constrained) = property.constrained_value(&value) else {
                continue;
            };
            values
                .set(property, constrained)
                .expect("constrained animation values preserve the property contract");
        }
        values
    }
}
