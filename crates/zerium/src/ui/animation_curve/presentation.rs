use crate::ui::numeric_property::numeric_input_spec;
use zerium_core::{
    property::{PropertySchema, ScalarPropertyType},
    timeline::{PropertyAddress, TimelineEditor, TimelineItem},
};

#[derive(Clone)]
pub(super) struct AnimationPresentation {
    pub(super) label: String,
    pub(super) suffix: String,
    pub(super) step: f64,
}

impl AnimationPresentation {
    pub(super) fn for_address(
        editor: &TimelineEditor,
        item: &TimelineItem,
        address: &PropertyAddress,
    ) -> Option<Self> {
        let property = address.schema(editor)?;
        let scalar_index = address.scalar_index;
        let scalar_type = property.scalar_type(address.element_id, scalar_index)?;
        let element_index = address.element_id.and_then(|id| {
            item.property_values(address.effect_id)?
                .property(&address.property_id)?
                .element_index(id)
        });
        let label = animation_label(property, address, element_index);
        if matches!(scalar_type, ScalarPropertyType::Color) {
            item.animation_track(
                address.effect_id,
                &address.property_id,
                address.element_id,
                address.scalar_index,
            )?;
            return Some(Self {
                label,
                suffix: String::new(),
                step: 0.01,
            });
        }
        let spec = numeric_input_spec(property, scalar_index)?;
        item.animation_track(
            address.effect_id,
            &address.property_id,
            address.element_id,
            address.scalar_index,
        )?;
        Some(Self {
            label,
            suffix: spec.suffix,
            step: spec.step,
        })
    }
}

fn animation_label(
    property: &PropertySchema,
    address: &PropertyAddress,
    element_index: Option<usize>,
) -> String {
    let label = element_index.map_or_else(
        || property.label().to_owned(),
        |index| format!("{} {}", property.label(), index + 1),
    );
    match address.scalar_index {
        Some(scalar_index) => property
            .configuration_label(Some(scalar_index))
            .map_or(label.clone(), |scalar_label| {
                format!("{label} {scalar_label}")
            }),
        None => label,
    }
}
