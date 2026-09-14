//! Apply enabled tracks to parameter values, enforcing their contracts.
use super::ParameterAnimations;
use crate::domain::parameter::{ParameterSchema, ParameterValues, materialized_parameter_values};

impl ParameterAnimations {
    pub(crate) fn evaluated_values(
        &self,
        base: &ParameterValues,
        schema: &[ParameterSchema],
        progress: f32,
    ) -> ParameterValues {
        let mut values = materialized_parameter_values(base, schema);
        for (address, track) in self.iter() {
            let Some(animated) = track.evaluate(progress) else {
                continue;
            };
            let Some(scalar) = values.get_scalar_at_mut(address) else {
                continue;
            };
            *scalar = animated;

            let Some(parameter) = schema
                .iter()
                .find(|parameter| parameter.id == address.parameter_id)
            else {
                continue;
            };
            let Some(value) = values
                .get(&address.parameter_id)
                .and_then(|value| parameter.constrained_value(value))
            else {
                continue;
            };
            values
                .set(parameter, value)
                .expect("constrained animation values preserve the parameter contract");
        }
        values
    }
}
