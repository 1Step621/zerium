//! Apply enabled tracks to property values, enforcing their contracts.
use super::ScalarAnimations;
use crate::domain::property::{PropertySchema, PropertyValues, materialized_property_values};
use std::collections::BTreeSet;

impl ScalarAnimations {
    pub(crate) fn evaluated_values(
        &self,
        base: &PropertyValues,
        schema: &[PropertySchema],
        progress: f32,
    ) -> PropertyValues {
        let mut values = materialized_property_values(base, schema);
        let mut animated_properties = BTreeSet::new();
        for (address, track) in self.tracks() {
            let property_id = address.property_id();
            if !schema.iter().any(|property| property.id == property_id) {
                continue;
            }
            let Some(value) = values.property_mut(property_id) else {
                continue;
            };
            let Some(element) = value.element_mut(address.element_id()) else {
                continue;
            };
            let Some(animated) = track.evaluate(progress) else {
                continue;
            };
            if let Some(scalar) = element.scalar_at_mut(address.scalar_index()) {
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
