//! Public parameter schema and read-only queries.

use super::{constraints::ParameterConstraints, ui::ParameterUi};
use crate::domain::parameter::{ParameterType, ParameterValue};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ParameterSchema {
    pub(in crate::domain) id: String,
    pub(in crate::domain) label: String,
    pub(in crate::domain) ty: ParameterType,
    pub(in crate::domain) default: ParameterValue,
    pub(in crate::domain) animatable: bool,
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

    pub(crate) const fn is_animatable(&self) -> bool {
        self.animatable
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
}
