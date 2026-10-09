//! Property schemas, scalar metadata, and semantic validation.

use super::{PropertyError, constraints::PropertyConstraints, metadata::PropertyUi};
use crate::localized_text::LocalizedText;
use crate::property::{PropertyElement, PropertyElementId, PropertyValue, ScalarPropertyType};
use serde::{Deserialize, Serialize};

const MAX_ARRAY_ITEMS: u32 = 1_000_000;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct PropertyConfiguration {
    #[serde(skip_serializing_if = "is_true")]
    pub(crate) scene_bindable: bool,
    #[serde(skip_serializing_if = "is_true")]
    pub(crate) editable: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub(crate) animatable: bool,
    #[serde(skip_serializing_if = "PropertyConstraints::is_default")]
    pub(crate) constraints: PropertyConstraints,
    #[serde(skip_serializing_if = "PropertyUi::is_default")]
    pub(crate) ui: PropertyUi,
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
    fn validate(
        &self,
        owner_kind: &str,
        owner_id: &str,
        property_id: &str,
        scalar_type: &ScalarPropertyType,
        is_array: bool,
        default: Option<&PropertyValue>,
    ) -> Result<(), PropertyError> {
        self.constraints
            .validate(owner_kind, owner_id, property_id, scalar_type, default)?;
        self.ui
            .validate(owner_kind, owner_id, property_id, scalar_type, is_array)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PropertySchema {
    pub(crate) id: String,
    pub(crate) label: LocalizedText,
    pub(super) definition: PropertyDefinition,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PropertyDefinition {
    Value(ValueSchema),
    Array {
        element: ValueSchema,
        min_items: u32,
        max_items: u32,
        default: Vec<PropertyElement>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum ValueSchema {
    Scalar(ScalarSchema),
    Tuple(Vec<ScalarSchema>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScalarSchema {
    pub ty: ScalarPropertyType,
    pub default: PropertyValue,
    pub configuration: PropertyConfiguration,
}

impl ValueSchema {
    pub fn scalars(&self) -> &[ScalarSchema] {
        match self {
            Self::Scalar(scalar) => std::slice::from_ref(scalar),
            Self::Tuple(elements) => elements,
        }
    }

    pub fn scalar(&self, index: Option<usize>) -> Option<&ScalarSchema> {
        match (self, index) {
            (Self::Scalar(scalar), None) => Some(scalar),
            (Self::Tuple(elements), Some(index)) => elements.get(index),
            _ => None,
        }
    }

    fn scalars_mut(&mut self) -> &mut [ScalarSchema] {
        match self {
            Self::Scalar(scalar) => std::slice::from_mut(scalar),
            Self::Tuple(elements) => elements,
        }
    }

    fn same_type(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Scalar(left), Self::Scalar(right)) => left.ty.same_type(&right.ty),
            (Self::Tuple(left), Self::Tuple(right)) => {
                left.len() == right.len()
                    && left
                        .iter()
                        .zip(right)
                        .all(|(left, right)| left.ty.same_type(&right.ty))
            }
            _ => false,
        }
    }

    fn allows(&self, value: &PropertyValue) -> bool {
        match (self, value) {
            (Self::Scalar(scalar), value) => scalar.ty.allows(value),
            (Self::Tuple(scalars), PropertyValue::Tuple(values)) => {
                scalars.len() == values.len()
                    && scalars
                        .iter()
                        .zip(values)
                        .all(|(scalar, value)| scalar.ty.allows(value))
            }
            _ => false,
        }
    }

    pub(super) fn default_value(&self) -> PropertyValue {
        match self {
            Self::Scalar(scalar) => scalar.default.clone(),
            Self::Tuple(elements) => PropertyValue::Tuple(
                elements
                    .iter()
                    .map(|scalar| scalar.default.clone())
                    .collect(),
            ),
        }
    }
}

pub struct ResolvedPropertyScalar<'a> {
    pub value: &'a PropertyValue,
    pub ty: &'a ScalarPropertyType,
    pub configuration: &'a PropertyConfiguration,
}

impl PropertySchema {
    pub(crate) fn new_scalar(
        id: String,
        label: LocalizedText,
        ty: ScalarPropertyType,
        default: PropertyValue,
        configuration: PropertyConfiguration,
    ) -> Self {
        Self {
            id,
            label,
            definition: PropertyDefinition::Value(ValueSchema::Scalar(ScalarSchema {
                ty,
                default,
                configuration,
            })),
        }
    }

    pub fn value_schema(&self) -> &ValueSchema {
        match &self.definition {
            PropertyDefinition::Value(value) | PropertyDefinition::Array { element: value, .. } => {
                value
            }
        }
    }

    fn value_schema_mut(&mut self) -> &mut ValueSchema {
        match &mut self.definition {
            PropertyDefinition::Value(value) | PropertyDefinition::Array { element: value, .. } => {
                value
            }
        }
    }

    pub(crate) fn resolve_value(&self, value: Option<&PropertyValue>) -> PropertyValue {
        value
            .and_then(|value| self.constrained_value(value))
            .unwrap_or_else(|| self.default_value())
    }

    pub fn scalar_type(
        &self,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
    ) -> Option<&ScalarPropertyType> {
        if element_id.is_some() != matches!(self.definition, PropertyDefinition::Array { .. }) {
            return None;
        }
        Some(&self.value_schema().scalar(scalar_index)?.ty)
    }

    pub fn resolve_scalar<'a>(
        &'a self,
        value: &'a PropertyValue,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
    ) -> Option<ResolvedPropertyScalar<'a>> {
        let ty = self.scalar_type(element_id, scalar_index)?;
        let value = value.scalar(element_id, scalar_index)?;
        ty.allows(value).then_some(ResolvedPropertyScalar {
            value,
            ty,
            configuration: self.configuration(scalar_index),
        })
    }

    pub(crate) fn scalar_projection(
        &self,
        value: &PropertyValue,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
    ) -> Option<Self> {
        let resolved = self.resolve_scalar(value, element_id, scalar_index)?;
        let mut scalar = self.value_schema().scalar(scalar_index)?.clone();
        scalar.default = scalar
            .configuration
            .constraints
            .clamp_value(resolved.value)?;
        Some(Self {
            id: self.id.clone(),
            label: self.label.clone(),
            definition: PropertyDefinition::Value(ValueSchema::Scalar(scalar)),
        })
    }

    pub fn configuration(&self, scalar_index: Option<usize>) -> &PropertyConfiguration {
        // Whole-tuple permission checks visit all scalars; scalar controls supply their index.
        &self.value_schema().scalars()[scalar_index.unwrap_or(0)].configuration
    }

    pub fn configuration_mut(&mut self, scalar_index: Option<usize>) -> &mut PropertyConfiguration {
        &mut self.value_schema_mut().scalars_mut()[scalar_index.unwrap_or(0)].configuration
    }

    pub fn configuration_ui(&self, scalar_index: Option<usize>) -> &PropertyUi {
        &self.configuration(scalar_index).ui
    }

    pub fn configuration_constraints(&self, scalar_index: Option<usize>) -> &PropertyConstraints {
        &self.configuration(scalar_index).constraints
    }

    pub fn configuration_label(&self, scalar_index: Option<usize>) -> Option<String> {
        scalar_index.map(|index| {
            self.configuration(Some(index))
                .ui
                .label()
                .map(str::to_owned)
                .unwrap_or_else(|| (index + 1).to_string())
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn label(&self) -> &str {
        self.label.resolve()
    }

    pub fn definition(&self) -> &PropertyDefinition {
        &self.definition
    }

    pub fn same_type(&self, other: &Self) -> bool {
        match (&self.definition, &other.definition) {
            (PropertyDefinition::Value(left), PropertyDefinition::Value(right)) => {
                left.same_type(right)
            }
            (
                PropertyDefinition::Array {
                    element: left,
                    min_items: left_min,
                    max_items: left_max,
                    ..
                },
                PropertyDefinition::Array {
                    element: right,
                    min_items: right_min,
                    max_items: right_max,
                    ..
                },
            ) => left_min == right_min && left_max == right_max && left.same_type(right),
            _ => false,
        }
    }

    pub(crate) fn allows_type(&self, value: &PropertyValue) -> bool {
        match (&self.definition, value) {
            (PropertyDefinition::Value(schema), value) => schema.allows(value),
            (
                PropertyDefinition::Array {
                    element,
                    min_items,
                    max_items,
                    ..
                },
                PropertyValue::Array(values),
            ) => {
                let mut ids = std::collections::HashSet::with_capacity(values.len());
                values.len() >= *min_items as usize
                    && values.len() <= *max_items as usize
                    && values.iter().all(|value| {
                        value.element_id().is_valid()
                            && ids.insert(value.element_id())
                            && element.allows(value.value())
                    })
            }
            _ => false,
        }
    }

    pub fn is_file(&self) -> bool {
        matches!(
            &self.definition,
            PropertyDefinition::Value(ValueSchema::Scalar(ScalarSchema {
                ty: ScalarPropertyType::File,
                ..
            }))
        )
    }

    pub fn default_value(&self) -> PropertyValue {
        match &self.definition {
            PropertyDefinition::Value(value) => value.default_value(),
            PropertyDefinition::Array { default, .. } => PropertyValue::Array(default.clone()),
        }
    }

    pub fn element_default_value(&self) -> Option<PropertyValue> {
        match &self.definition {
            PropertyDefinition::Array { element, .. } => Some(element.default_value()),
            _ => None,
        }
    }

    pub(crate) fn set_default(&mut self, value: PropertyValue) {
        match (&mut self.definition, value) {
            (PropertyDefinition::Value(ValueSchema::Scalar(scalar)), value) => {
                scalar.default = value
            }
            (
                PropertyDefinition::Value(ValueSchema::Tuple(elements)),
                PropertyValue::Tuple(values),
            ) => {
                for (scalar, value) in elements.iter_mut().zip(values) {
                    scalar.default = value;
                }
            }
            (PropertyDefinition::Array { default, .. }, PropertyValue::Array(values)) => {
                *default = values
            }
            _ => unreachable!("default value was validated against the schema"),
        }
    }

    pub fn is_editable(&self, scalar_index: Option<usize>) -> bool {
        scalar_index.map_or_else(
            || {
                self.value_schema()
                    .scalars()
                    .iter()
                    .all(|scalar| scalar.configuration.editable)
            },
            |index| self.configuration(Some(index)).editable,
        )
    }

    pub fn is_animatable(&self, scalar_index: Option<usize>) -> bool {
        self.value_schema()
            .scalar(scalar_index)
            .is_some_and(|scalar| scalar.configuration.editable && scalar.configuration.animatable)
    }

    pub fn is_scene_bindable(&self, scalar_index: Option<usize>) -> bool {
        self.configuration(scalar_index).scene_bindable
    }

    pub fn is_visible(&self) -> bool {
        self.value_schema()
            .scalars()
            .iter()
            .any(|scalar| scalar.configuration.ui.is_visible())
    }

    pub fn accepts_value(&self, value: &PropertyValue) -> bool {
        self.allows_type(value) && self.accepts_constraints(value)
    }

    pub fn constrained_value(&self, value: &PropertyValue) -> Option<PropertyValue> {
        if !self.allows_type(value) {
            return None;
        }
        self.constrain_scalars(value)
    }

    fn accepts_constraints(&self, value: &PropertyValue) -> bool {
        value
            .scalars()
            .all(|(_, index, scalar)| self.configuration_constraints(index).allows(scalar))
    }

    fn constrain_scalars(&self, value: &PropertyValue) -> Option<PropertyValue> {
        match value {
            PropertyValue::Tuple(values) => values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    self.configuration_constraints(Some(index))
                        .clamp_value(value)
                })
                .collect::<Option<Vec<_>>>()
                .map(PropertyValue::Tuple),
            PropertyValue::Array(values) => values
                .iter()
                .map(|element| {
                    Some(PropertyElement {
                        id: element.element_id(),
                        value: self.constrain_scalars(element.value())?,
                    })
                })
                .collect::<Option<Vec<_>>>()
                .map(PropertyValue::Array),
            value => self.configuration_constraints(None).clamp_value(value),
        }
    }

    pub(crate) fn validate(&self, owner_kind: &str, owner_id: &str) -> Result<(), PropertyError> {
        let invalid = |message| self.validation_error(owner_kind, owner_id, message);
        if self.id.trim().is_empty() {
            return Err(invalid("id must not be empty"));
        }
        if self.label.is_empty() {
            return Err(invalid("label must not be empty"));
        }
        if let PropertyDefinition::Array {
            min_items,
            max_items,
            ..
        } = &self.definition
            && (!(1..=MAX_ARRAY_ITEMS).contains(max_items) || min_items > max_items)
        {
            return Err(invalid(
                "array length bounds must satisfy 0 <= min_items <= max_items <= 1000000, with max_items > 0",
            ));
        }
        for scalar in self.value_schema().scalars() {
            if (scalar.configuration.animatable && !scalar.ty.is_interpolatable())
                || !scalar.ty.allows(&scalar.default)
            {
                return Err(invalid(
                    "scalar default or animation settings do not match its type",
                ));
            }
            let is_array = matches!(
                &self.definition,
                PropertyDefinition::Array {
                    element: ValueSchema::Scalar(_),
                    ..
                }
            );
            scalar.configuration.validate(
                owner_kind,
                owner_id,
                &self.id,
                &scalar.ty,
                is_array,
                Some(&scalar.default),
            )?;
        }
        if !self.accepts_value(&self.default_value()) {
            return Err(invalid("default violates the type or constraints"));
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
