//! Semantic validation for parameter schemas and parameter collections.

use super::ParameterError;
use super::schema::ParameterSchema;
use crate::domain::parameter::ParameterType;

const MAX_ARRAY_ITEMS: u32 = 1_000_000;

impl ParameterSchema {
    pub(in crate::domain) fn validate(
        &self,
        owner_kind: &str,
        owner_id: &str,
    ) -> Result<(), ParameterError> {
        if self.id.trim().is_empty() {
            return Err(parameter_error(
                self,
                owner_kind,
                owner_id,
                "id must not be empty",
            ));
        }

        validate_array_type(self, owner_kind, owner_id)?;

        let component_type = self.ty.element_type();
        if self.animatable && !crate::domain::animation::supports_value(component_type) {
            return Err(parameter_error(
                self,
                owner_kind,
                owner_id,
                "must contain at least one numeric or color scalar to be animatable",
            ));
        }
        if !self.default.matches_type(&self.ty) {
            return Err(parameter_error(
                self,
                owner_kind,
                owner_id,
                "default does not match its type",
            ));
        }
        if self.label.trim().is_empty() {
            return Err(parameter_error(
                self,
                owner_kind,
                owner_id,
                "label must not be empty",
            ));
        }

        self.constraints
            .validate(owner_kind, owner_id, &self.id, &self.ty, &self.default)?;
        self.ui
            .validate(owner_kind, owner_id, &self.id, &self.ty, component_type)
    }
}

fn validate_array_type(
    parameter: &ParameterSchema,
    owner_kind: &str,
    owner_id: &str,
) -> Result<(), ParameterError> {
    let ParameterType::Array {
        element: _,
        min_items,
        max_items,
    } = &parameter.ty
    else {
        return Ok(());
    };

    if !(1..=MAX_ARRAY_ITEMS).contains(max_items) {
        return Err(parameter_error(
            parameter,
            owner_kind,
            owner_id,
            &format!("max_items must be between 1 and {MAX_ARRAY_ITEMS}"),
        ));
    }
    if min_items > max_items {
        return Err(parameter_error(
            parameter,
            owner_kind,
            owner_id,
            "min_items must not exceed max_items",
        ));
    }
    Ok(())
}

fn parameter_error(
    parameter: &ParameterSchema,
    owner_kind: &str,
    owner_id: &str,
    message: &str,
) -> ParameterError {
    ParameterError::invalid_definition(format!(
        "{owner_kind} '{owner_id}' parameter '{}': {message}",
        parameter.id
    ))
}
