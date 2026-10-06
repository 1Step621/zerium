//! Stable scalar coordinates shared by editing, animation, and scene bindings.
use serde::{Deserialize, Serialize};

use super::{
    PropertyConfiguration, PropertyElementId, PropertySchema, PropertyType, PropertyValue,
    PropertyValues, ScalarPropertyType,
};

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Hash, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyPath {
    #[serde(rename = "property")]
    property_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    element_id: Option<PropertyElementId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scalar_index: Option<usize>,
}

pub struct ResolvedPropertyScalar<'a> {
    pub value: &'a PropertyValue,
    pub ty: &'a ScalarPropertyType,
    pub configuration: &'a PropertyConfiguration,
}

impl PropertyPath {
    pub fn new(
        property_id: impl Into<String>,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
    ) -> Self {
        Self {
            property_id: property_id.into(),
            element_id,
            scalar_index,
        }
    }

    pub fn property_id(&self) -> &str {
        &self.property_id
    }

    pub const fn element_id(&self) -> Option<PropertyElementId> {
        self.element_id
    }

    pub const fn scalar_index(&self) -> Option<usize> {
        self.scalar_index
    }

    pub fn value<'a>(&self, values: &'a PropertyValues) -> Option<&'a PropertyValue> {
        values
            .property(self.property_id())?
            .scalar(self.element_id, self.scalar_index)
    }

    pub fn value_mut<'a>(&self, values: &'a mut PropertyValues) -> Option<&'a mut PropertyValue> {
        values
            .property_mut(self.property_id())?
            .scalar_mut(self.element_id, self.scalar_index)
    }
}

impl PropertySchema {
    pub fn scalar_type(
        &self,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
    ) -> Option<&ScalarPropertyType> {
        let value_type = match (element_id, self.ty()) {
            (Some(_), PropertyType::Array { element_type, .. })
            | (None, PropertyType::Value(element_type)) => element_type,
            _ => return None,
        };
        value_type.scalar_at(scalar_index)
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
            configuration: self.configurations.get(scalar_index.unwrap_or(0))?,
        })
    }
}

impl PropertyValue {
    pub fn scalar(
        &self,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
    ) -> Option<&Self> {
        self.element(element_id)?.scalar_at(scalar_index)
    }

    pub fn scalar_mut(
        &mut self,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
    ) -> Option<&mut Self> {
        self.element_mut(element_id)?.scalar_at_mut(scalar_index)
    }

    /// Replace a whole value, array element, or tuple scalar without changing siblings.
    pub fn replaced_at(
        &self,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
        value: Self,
    ) -> Option<Self> {
        let mut next = self.clone();
        let element = next.element_mut(element_id)?;
        let target = match scalar_index {
            Some(index) => element.scalar_at_mut(Some(index))?,
            None => element,
        };
        *target = value;
        Some(next)
    }
}
