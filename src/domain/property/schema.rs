//! Property schemas, animation permissions, and semantic validation.

use super::{PropertyError, constraints::PropertyConstraints, ui::PropertyUi};
use crate::domain::property::{PropertyType, PropertyValue, PropertyValueType};

const MAX_ARRAY_ITEMS: u32 = 1_000_000;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PropertyAnimatable {
    Scalar(bool),
    Tuple(Vec<bool>),
}

impl Default for PropertyAnimatable {
    fn default() -> Self {
        Self::Scalar(false)
    }
}

impl PropertyAnimatable {
    pub(crate) fn is_enabled(&self, scalar_index: Option<usize>) -> bool {
        match (self, scalar_index) {
            (Self::Scalar(enabled), None) => *enabled,
            (Self::Tuple(scalars), Some(index)) => scalars.get(index).copied().unwrap_or(false),
            _ => false,
        }
    }

    pub(crate) fn to_scalar(&self, scalar_index: usize) -> Self {
        Self::Scalar(self.is_enabled(Some(scalar_index)))
    }

    fn valid_for(&self, ty: &PropertyValueType) -> bool {
        match (self, ty) {
            (Self::Scalar(enabled), PropertyValueType::Scalar(ty)) => {
                !enabled || ty.is_interpolatable()
            }
            (Self::Tuple(scalars), PropertyValueType::Tuple(tuple)) => {
                scalars.len() == tuple.scalar_count()
                    && scalars
                        .iter()
                        .zip(tuple.scalars())
                        .all(|(enabled, ty)| !enabled || ty.is_interpolatable())
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PropertyEditable {
    Scalar(bool),
    Tuple(Vec<bool>),
}

impl Default for PropertyEditable {
    fn default() -> Self {
        Self::Scalar(true)
    }
}

impl PropertyEditable {
    pub(crate) fn is_enabled(&self, scalar_index: Option<usize>) -> bool {
        match (self, scalar_index) {
            (Self::Scalar(enabled), None) => *enabled,
            (Self::Tuple(scalars), Some(index)) => scalars.get(index).copied().unwrap_or(false),
            (Self::Tuple(scalars), None) => scalars.iter().all(|enabled| *enabled),
            _ => false,
        }
    }

    pub(crate) fn to_scalar(&self, scalar_index: usize) -> Self {
        Self::Scalar(self.is_enabled(Some(scalar_index)))
    }

    fn valid_for(&self, ty: &PropertyValueType) -> bool {
        match (self, ty) {
            (Self::Scalar(_), PropertyValueType::Scalar(_)) => true,
            (Self::Tuple(scalars), PropertyValueType::Tuple(tuple)) => {
                scalars.len() == tuple.scalar_count()
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PropertySchema {
    pub(in crate::domain) id: String,
    pub(in crate::domain) label: String,
    pub(in crate::domain) ty: PropertyType,
    pub(in crate::domain) default: PropertyValue,
    pub(in crate::domain) editable: PropertyEditable,
    pub(in crate::domain) animatable: PropertyAnimatable,
    pub(in crate::domain) scene_bindable: bool,
    pub(in crate::domain) constraints: PropertyConstraints,
    pub(in crate::domain) ui: PropertyUi,
}

impl PropertySchema {
    pub(crate) fn scalar_ui(&self, scalar_index: Option<usize>) -> &PropertyUi {
        scalar_index.map_or(&self.ui, |index| self.ui.for_scalar(index))
    }

    pub(crate) fn scalar_constraints(&self, scalar_index: Option<usize>) -> &PropertyConstraints {
        scalar_index.map_or(&self.constraints, |index| {
            self.constraints.for_scalar(index)
        })
    }

    pub(crate) fn scalar_label(&self, scalar_index: Option<usize>) -> Option<String> {
        scalar_index.map(|index| {
            self.ui
                .for_scalar(index)
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

    pub(crate) fn ty(&self) -> &PropertyType {
        &self.ty
    }

    pub(crate) fn default_value(&self) -> &PropertyValue {
        &self.default
    }

    pub(crate) fn is_editable(&self, scalar_index: Option<usize>) -> bool {
        self.editable.is_enabled(scalar_index)
    }

    pub(crate) fn is_animatable(&self, scalar_index: Option<usize>) -> bool {
        self.is_editable(scalar_index) && self.animatable.is_enabled(scalar_index)
    }

    pub(crate) const fn is_scene_bindable(&self) -> bool {
        self.scene_bindable
    }

    pub(crate) const fn constraints(&self) -> &PropertyConstraints {
        &self.constraints
    }

    pub(crate) const fn ui(&self) -> &PropertyUi {
        &self.ui
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.ui.is_visible()
    }

    pub(crate) fn accepts_value(&self, value: &PropertyValue) -> bool {
        self.ty.allows(value) && self.constraints.allows(value)
    }

    pub(crate) fn constrained_value(&self, value: &PropertyValue) -> Option<PropertyValue> {
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
    ) -> Result<(), PropertyError> {
        if self.id.trim().is_empty() {
            return Err(self.validation_error(owner_kind, owner_id, "id must not be empty"));
        }

        if let PropertyType::Array {
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

        let component_type = match &self.ty {
            PropertyType::Value(value_type)
            | PropertyType::Array {
                element_type: value_type,
                ..
            } => value_type,
        };
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

    fn validation_error(&self, owner_kind: &str, owner_id: &str, message: &str) -> PropertyError {
        PropertyError::invalid_definition(format!(
            "{owner_kind} '{owner_id}' property '{}': {message}",
            self.id
        ))
    }
}
