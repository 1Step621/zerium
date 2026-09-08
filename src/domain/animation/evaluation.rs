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
        for (target, animation) in self.iter() {
            let Some(parameter) = schema
                .iter()
                .find(|parameter| parameter.id == target.parameter_id)
            else {
                continue;
            };
            let Some(value) =
                values
                    .get(&target.parameter_id)
                    .and_then(|value| match target.array_index {
                        Some(index) => value.with_animated_array_element(
                            index,
                            animation.evaluate(&value.animated_array_element(index)?, progress)?,
                        ),
                        None => animation.evaluate(value, progress),
                    })
            else {
                continue;
            };
            let Some(value) = parameter.constrained_value(&value) else {
                continue;
            };
            values
                .set(parameter, value)
                .expect("constrained animation values preserve the parameter contract");
        }
        values
    }
}
