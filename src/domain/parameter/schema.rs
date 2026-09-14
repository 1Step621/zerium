//! Parameter schemas, animation permissions, and semantic validation.

use super::{ParameterError, constraints::ParameterConstraints, ui::ParameterUi};
use crate::domain::parameter::{ParameterType, ParameterValue, ParameterValueType};

const MAX_ARRAY_ITEMS: u32 = 1_000_000;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ParameterAnimatable {
    Scalar(bool),
    Tuple(Vec<bool>),
}

impl Default for ParameterAnimatable {
    fn default() -> Self {
        Self::Scalar(false)
    }
}

impl ParameterAnimatable {
    pub(crate) fn is_enabled(&self, element: Option<usize>) -> bool {
        match (self, element) {
            (Self::Scalar(enabled), None) => *enabled,
            (Self::Tuple(elements), Some(index)) => elements.get(index).copied().unwrap_or(false),
            _ => false,
        }
    }

    pub(crate) fn to_scalar(&self, element: usize) -> Self {
        Self::Scalar(self.is_enabled(Some(element)))
    }

    fn valid_for(&self, ty: &ParameterValueType) -> bool {
        match (self, ty) {
            (Self::Scalar(enabled), ParameterValueType::Scalar(ty)) => {
                !enabled || ty.is_interpolatable()
            }
            (Self::Tuple(elements), ParameterValueType::Tuple(tuple)) => {
                elements.len() == tuple.element_count()
                    && elements
                        .iter()
                        .zip(tuple.elements())
                        .all(|(enabled, ty)| !enabled || ty.is_interpolatable())
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ParameterEditable {
    Scalar(bool),
    Tuple(Vec<bool>),
}

impl Default for ParameterEditable {
    fn default() -> Self {
        Self::Scalar(true)
    }
}

impl ParameterEditable {
    pub(crate) fn is_enabled(&self, element: Option<usize>) -> bool {
        match (self, element) {
            (Self::Scalar(enabled), None) => *enabled,
            (Self::Tuple(elements), Some(index)) => elements.get(index).copied().unwrap_or(false),
            (Self::Tuple(elements), None) => elements.iter().all(|enabled| *enabled),
            _ => false,
        }
    }

    pub(crate) fn to_scalar(&self, element: usize) -> Self {
        Self::Scalar(self.is_enabled(Some(element)))
    }

    fn valid_for(&self, ty: &ParameterValueType) -> bool {
        match (self, ty) {
            (Self::Scalar(_), ParameterValueType::Scalar(_)) => true,
            (Self::Tuple(elements), ParameterValueType::Tuple(tuple)) => {
                elements.len() == tuple.element_count()
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ParameterSchema {
    pub(in crate::domain) id: String,
    pub(in crate::domain) label: String,
    pub(in crate::domain) ty: ParameterType,
    pub(in crate::domain) default: ParameterValue,
    pub(in crate::domain) editable: ParameterEditable,
    pub(in crate::domain) animatable: ParameterAnimatable,
    pub(in crate::domain) scene_bindable: bool,
    pub(in crate::domain) constraints: ParameterConstraints,
    pub(in crate::domain) ui: ParameterUi,
}

impl ParameterSchema {
    pub(crate) fn scalar_ui(&self, element: Option<usize>) -> &ParameterUi {
        element.map_or(&self.ui, |index| self.ui.for_element(index))
    }

    pub(crate) fn scalar_constraints(&self, element: Option<usize>) -> &ParameterConstraints {
        element.map_or(&self.constraints, |index| {
            self.constraints.for_element(index)
        })
    }

    pub(crate) fn scalar_label(&self, element: Option<usize>) -> Option<String> {
        element.map(|index| {
            self.ui
                .for_element(index)
                .label()
                .map(str::to_owned)
                .unwrap_or_else(|| (index + 1).to_string())
        })
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    pub(crate) fn ty(&self) -> &ParameterType {
        &self.ty
    }

    pub(crate) fn default_value(&self) -> &ParameterValue {
        &self.default
    }

    pub(crate) fn is_editable(&self, element: Option<usize>) -> bool {
        self.editable.is_enabled(element)
    }

    pub(crate) fn is_animatable(&self, element: Option<usize>) -> bool {
        self.is_editable(element) && self.animatable.is_enabled(element)
    }

    pub(crate) const fn is_scene_bindable(&self) -> bool {
        self.scene_bindable
    }

    pub(crate) const fn constraints(&self) -> &ParameterConstraints {
        &self.constraints
    }

    pub(crate) const fn ui(&self) -> &ParameterUi {
        &self.ui
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.ui.is_visible()
    }

    pub(crate) fn accepts_value(&self, value: &ParameterValue) -> bool {
        self.ty.allows(value) && self.constraints.allows(value)
    }

    pub(crate) fn constrained_value(&self, value: &ParameterValue) -> Option<ParameterValue> {
        if !self.ty.allows(value) {
            return None;
        }
        let constrained = self.constraints.clamp_value(value)?;
        self.accepts_value(&constrained).then_some(constrained)
    }

    pub(in crate::domain) fn validate(
        &self,
        owner_kind: &str,
        owner_id: &str,
    ) -> Result<(), ParameterError> {
        if self.id.trim().is_empty() {
            return Err(self.validation_error(owner_kind, owner_id, "id must not be empty"));
        }

        if let ParameterType::Array {
            min_items,
            max_items,
            ..
        } = &self.ty
        {
            if !(1..=MAX_ARRAY_ITEMS).contains(max_items) {
                return Err(self.validation_error(
                    owner_kind,
                    owner_id,
                    &format!("max_items must be between 1 and {MAX_ARRAY_ITEMS}"),
                ));
            }
            if min_items > max_items {
                return Err(self.validation_error(
                    owner_kind,
                    owner_id,
                    "min_items must not exceed max_items",
                ));
            }
        }

        let component_type = self.ty.element_type();
        if !self.editable.valid_for(component_type) {
            return Err(self.validation_error(
                owner_kind,
                owner_id,
                "has editable elements that do not match its type",
            ));
        }
        if !self.animatable.valid_for(component_type) {
            return Err(self.validation_error(
                owner_kind,
                owner_id,
                "has animatable elements that do not match its type",
            ));
        }
        if !self.ty.allows(&self.default) {
            return Err(self.validation_error(
                owner_kind,
                owner_id,
                "default does not match its type",
            ));
        }
        if self.label.trim().is_empty() {
            return Err(self.validation_error(owner_kind, owner_id, "label must not be empty"));
        }

        self.constraints
            .validate(owner_kind, owner_id, &self.id, &self.ty, &self.default)?;
        self.ui
            .validate(owner_kind, owner_id, &self.id, &self.ty, component_type)
    }

    fn validation_error(&self, owner_kind: &str, owner_id: &str, message: &str) -> ParameterError {
        ParameterError::invalid_definition(format!(
            "{owner_kind} '{owner_id}' parameter '{}': {message}",
            self.id
        ))
    }
}
