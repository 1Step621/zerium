//! Property schemas, scalar metadata, and semantic validation.

use super::{PropertyError, constraints::PropertyConstraints, ui::PropertyUi};
use crate::domain::property::{PropertyType, PropertyValue, PropertyValueType, ScalarPropertyType};
use serde::{Deserialize, Serialize};

const MAX_ARRAY_ITEMS: u32 = 1_000_000;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct PropertyConfiguration {
    #[serde(skip_serializing_if = "is_true")]
    pub(in crate::domain) scene_bindable: bool,
    #[serde(skip_serializing_if = "is_true")]
    pub(in crate::domain) editable: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub(in crate::domain) animatable: bool,
    #[serde(skip_serializing_if = "PropertyConstraints::is_default")]
    pub(in crate::domain) constraints: PropertyConstraints,
    #[serde(skip_serializing_if = "PropertyUi::is_default")]
    pub(in crate::domain) ui: PropertyUi,
}

fn is_true(value: &bool) -> bool {
    *value
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl Default for PropertyConfiguration {
    fn default() -> Self {
        Self {
            scene_bindable: true,
            editable: true,
            animatable: false,
            constraints: PropertyConstraints::default(),
            ui: PropertyUi::default(),
        }
    }
}

impl PropertyConfiguration {
    fn valid_for(&self, ty: &ScalarPropertyType) -> bool {
        !self.animatable || ty.is_interpolatable()
    }

    fn accepts(&self, value: &PropertyValue) -> bool {
        self.constraints.allows(value)
    }

    fn constrain(&self, value: &PropertyValue) -> Option<PropertyValue> {
        self.constraints.clamp_value(value)
    }

    fn validate(
        &self,
        owner_kind: &str,
        owner_id: &str,
        property_id: &str,
        scalar_type: &ScalarPropertyType,
        ui_type: &PropertyType,
        default: Option<&PropertyValue>,
    ) -> Result<(), PropertyError> {
        let value_type = PropertyType::Value(PropertyValueType::Scalar(scalar_type.clone()));
        self.constraints
            .validate(owner_kind, owner_id, property_id, &value_type, default)?;
        self.ui.validate(owner_kind, owner_id, property_id, ui_type)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PropertySchema {
    pub(in crate::domain) id: String,
    pub(in crate::domain) label: String,
    #[serde(rename = "type")]
    pub(in crate::domain) ty: PropertyType,
    pub(in crate::domain) default: PropertyValue,
    pub(in crate::domain) configurations: Vec<PropertyConfiguration>,
}

impl PropertySchema {
    pub(crate) fn configuration(&self, scalar_index: Option<usize>) -> &PropertyConfiguration {
        &self.configurations[scalar_index.unwrap_or(0)]
    }

    pub(crate) fn configuration_mut(
        &mut self,
        scalar_index: Option<usize>,
    ) -> &mut PropertyConfiguration {
        &mut self.configurations[scalar_index.unwrap_or(0)]
    }

    pub(crate) fn configuration_ui(&self, scalar_index: Option<usize>) -> &PropertyUi {
        &self.configuration(scalar_index).ui
    }

    pub(crate) fn configuration_constraints(
        &self,
        scalar_index: Option<usize>,
    ) -> &PropertyConstraints {
        &self.configuration(scalar_index).constraints
    }

    pub(crate) fn configuration_label(&self, scalar_index: Option<usize>) -> Option<String> {
        scalar_index.map(|index| {
            self.configuration(Some(index))
                .ui
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
        scalar_index.map_or_else(
            || {
                self.configurations
                    .iter()
                    .all(|configuration| configuration.editable)
            },
            |index| self.configuration(Some(index)).editable,
        )
    }

    pub(crate) fn is_animatable(&self, scalar_index: Option<usize>) -> bool {
        let configuration = scalar_index.map_or_else(
            || (self.configurations.len() == 1).then(|| self.configuration(None)),
            |index| Some(self.configuration(Some(index))),
        );
        configuration
            .is_some_and(|configuration| configuration.editable && configuration.animatable)
    }

    pub(crate) fn is_scene_bindable(&self, scalar_index: Option<usize>) -> bool {
        self.configuration(scalar_index).scene_bindable
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.configurations
            .iter()
            .any(|configuration| configuration.ui.is_visible())
    }

    pub(crate) fn accepts_value(&self, value: &PropertyValue) -> bool {
        self.ty.allows(value) && self.accepts_constraints(value)
    }

    pub(crate) fn constrained_value(&self, value: &PropertyValue) -> Option<PropertyValue> {
        if !self.ty.allows(value) {
            return None;
        }
        let mut map = |configuration: &PropertyConfiguration, value: &PropertyValue| {
            configuration.constrain(value)
        };
        let constrained = self.map_values(value, &mut map)?;
        self.accepts_value(&constrained).then_some(constrained)
    }

    fn accepts_constraints(&self, value: &PropertyValue) -> bool {
        let mut map = |configuration: &PropertyConfiguration, value: &PropertyValue| {
            configuration.accepts(value).then(|| value.clone())
        };
        self.map_values(value, &mut map).is_some()
    }

    fn map_values<F>(&self, value: &PropertyValue, map: &mut F) -> Option<PropertyValue>
    where
        F: FnMut(&PropertyConfiguration, &PropertyValue) -> Option<PropertyValue>,
    {
        match value {
            PropertyValue::Tuple(values) => values
                .iter()
                .enumerate()
                .map(|(index, value)| map(self.configuration(Some(index)), value))
                .collect::<Option<Vec<_>>>()
                .map(PropertyValue::Tuple),
            PropertyValue::Array(values) => values
                .iter()
                .map(|element| {
                    let mut constrained = element.clone();
                    *constrained.value_mut() = self.map_values(element.value(), map)?;
                    Some(constrained)
                })
                .collect::<Option<Vec<_>>>()
                .map(PropertyValue::Array),
            value => map(self.configuration(None), value),
        }
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
        let scalar_types = match component_type {
            PropertyValueType::Scalar(scalar) => std::slice::from_ref(scalar),
            PropertyValueType::Tuple(tuple) => tuple.scalars(),
        };
        if self.configurations.len() != scalar_types.len()
            || self
                .configurations
                .iter()
                .zip(scalar_types)
                .any(|(configuration, ty)| !configuration.valid_for(ty))
        {
            return Err(self.validation_error(
                owner_kind,
                owner_id,
                "has scalar metadata that does not match its type",
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

        for (index, (configuration, ty)) in self.configurations.iter().zip(scalar_types).enumerate()
        {
            let scalar_ui_type = PropertyType::Value(PropertyValueType::Scalar(ty.clone()));
            let ui_type = if matches!(component_type, PropertyValueType::Scalar(_)) {
                &self.ty
            } else {
                &scalar_ui_type
            };
            let default = matches!(&self.ty, PropertyType::Value(_))
                .then(|| self.default.scalar_at(Some(index)).unwrap_or(&self.default));
            configuration.validate(owner_kind, owner_id, &self.id, ty, ui_type, default)?;
        }
        if !self.accepts_value(&self.default) {
            return Err(PropertyError::invalid_definition(format!(
                "{owner_kind} '{owner_id}' property '{}' default violates its constraints",
                self.id
            )));
        }
        Ok(())
    }

    fn validation_error(&self, owner_kind: &str, owner_id: &str, message: &str) -> PropertyError {
        PropertyError::invalid_definition(format!(
            "{owner_kind} '{owner_id}' property '{}': {message}",
            self.id
        ))
    }
}
