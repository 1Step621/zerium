use crate::ui::animation::animation_property_label;
use crate::ui::numeric_property::numeric_input_spec;
use zerium_core::{
    property::ScalarPropertyType,
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
        editor.animation_track(address)?;
        let property = editor.property_schema(address)?;
        let scalar_index = address.scalar_index;
        let scalar_type = property.scalar_type(address.element_id, scalar_index)?;
        let label = animation_property_label(editor, item, address)?;
        if let ScalarPropertyType::Color = scalar_type {
            return Some(Self {
                label,
                suffix: String::new(),
                step: 0.01,
            });
        }
        let spec = numeric_input_spec(property, scalar_index)?;
        Some(Self {
            label,
            suffix: spec.suffix,
            step: spec.step,
        })
    }
}
