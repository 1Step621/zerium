//! Apply enabled tracks to property values, enforcing their contracts.
use super::{ElementAnimations, PropertyAnimations};
use crate::domain::property::{PropertySchema, PropertyValues, materialized_property_values};

impl PropertyAnimations {
    pub(crate) fn evaluated_values(
        &self,
        base: &PropertyValues,
        schema: &[PropertySchema],
        progress: f32,
    ) -> PropertyValues {
        let mut values = materialized_property_values(base, schema);
        for (property_id, animations) in self.properties() {
            let Some(property) = schema.iter().find(|property| property.id == property_id) else {
                continue;
            };
            let Some(value) = values.property_mut(property_id) else {
                continue;
            };
            let mut apply = |element_id, element_animations: &ElementAnimations| {
                let Some(element) = value.element_mut(element_id) else {
                    return;
                };
                for (scalar_index, track) in element_animations.tracks() {
                    let Some(animated) = track.evaluate(progress) else {
                        continue;
                    };
                    if let Some(scalar) = element.scalar_at_mut(scalar_index) {
                        *scalar = animated;
                    }
                }
            };
            if let Some(element) = animations.element(None) {
                apply(None, element);
            }
            for (element_id, element) in animations.elements() {
                apply(Some(element_id), element);
            }
            let Some(constrained) = property.constrained_value(value) else {
                continue;
            };
            values
                .set(property, constrained)
                .expect("constrained animation values preserve the property contract");
        }
        values
    }
}
