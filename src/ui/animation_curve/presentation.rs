use crate::domain::{
    property::{PropertySchema, PropertyType, ScalarPropertyType},
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
        let property = animation_property(editor, item, address)?;
        let scalar_index = address.scalar_index;
        let value_type = match property.ty() {
            PropertyType::Value(value_type)
            | PropertyType::Array {
                element_type: value_type,
                ..
            } => value_type,
        };
        let scalar_type = value_type.scalar_at(scalar_index)?.clone();
        let element_index = address.element_id.and_then(|id| {
            item.property_values(address.effect_id)?
                .property(&address.property_id)?
                .element_index(id)
        });
        let label = animation_label(&property, address, element_index);
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
        let (suffix, step) = numeric_animation_display(&property, scalar_index)?;
        item.animation_track(
            address.effect_id,
            &address.property_id,
            address.element_id,
            address.scalar_index,
        )?;
        Some(Self {
            label,
            suffix,
            step,
        })
    }
}

fn numeric_animation_display(
    property: &PropertySchema,
    scalar_index: Option<usize>,
) -> Option<(String, f64)> {
    if !property.is_visible() || !property.configuration_ui(scalar_index).is_visible() {
        return None;
    }
    let value_type = match property.ty() {
        PropertyType::Value(value_type)
        | PropertyType::Array {
            element_type: value_type,
            ..
        } => value_type,
    };
    let ui = property.configuration_ui(scalar_index);
    let step = match value_type.scalar_at(scalar_index)? {
        ScalarPropertyType::F32 => f64::from(ui.step()),
        ScalarPropertyType::I32 | ScalarPropertyType::U32 => f64::from(ui.step()).max(1.),
        _ => return None,
    };
    Some((ui.unit().to_owned(), step))
}

fn animation_property(
    editor: &TimelineEditor,
    item: &TimelineItem,
    address: &PropertyAddress,
) -> Option<PropertySchema> {
    if let Some(scene_id) = item.scene_id()
        && let Some(property) = editor
            .scene(scene_id)?
            .arguments
            .iter()
            .find(|argument| argument.schema.id() == address.property_id)
            .map(|argument| argument.schema.clone())
    {
        return Some(property);
    }
    if let Some(effect_id) = address.effect_id {
        return item
            .effects
            .iter()
            .find(|effect| effect.id == effect_id)
            .and_then(|effect| effect.schema().property(&address.property_id))
            .cloned();
    }
    item.schema()?.property(&address.property_id).cloned()
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
