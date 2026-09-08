//! Parameter compatibility, value acceptance, and constraint projection.

use super::schema::ParameterSchema;
use crate::domain::parameter::ParameterValue;

impl ParameterSchema {
    pub(crate) fn constraints_allow(&self, value: &ParameterValue) -> bool {
        self.constraints.allows(value)
    }

    pub(crate) fn constraints_allow_numeric_scalar(
        &self,
        element: Option<usize>,
        value: f64,
    ) -> bool {
        use super::ScalarParameterType;
        let base = match self.ty.element_type().scalar_at(element) {
            Some(ScalarParameterType::F32) => ParameterValue::F32(0.),
            Some(ScalarParameterType::I32) => ParameterValue::I32(0),
            Some(ScalarParameterType::U32) => ParameterValue::U32(0),
            _ => return false,
        };
        base.with_numeric_scalar(value)
            .is_some_and(|value| self.scalar_constraints(element).allows(&value))
    }

    pub(crate) fn accepts_value(&self, value: &ParameterValue) -> bool {
        value.matches_type(&self.ty) && self.constraints_allow(value)
    }

    pub(crate) fn accepts_values_from(&self, source: &Self) -> bool {
        self.ty == source.ty && self.constraints.contains(&source.constraints)
    }

    pub(crate) fn narrowed_for_target(&self, target: &Self) -> Option<Self> {
        if target.ty != self.ty {
            return None;
        }
        let mut narrowed = self.clone();
        narrowed.constraints = self.constraints.intersection(&target.constraints)?;
        narrowed.default = narrowed.constrained_value(&self.default)?;
        Some(narrowed)
    }

    pub(crate) fn constrained_value(&self, value: &ParameterValue) -> Option<ParameterValue> {
        if !value.matches_type(&self.ty) {
            return None;
        }
        let constrained = self.constraints.clamp_value(value)?;
        self.accepts_value(&constrained).then_some(constrained)
    }
}
