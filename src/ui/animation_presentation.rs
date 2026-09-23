use crate::{
    domain::{
        property::{PropertySchema, PropertyType, ScalarPropertyType},
        timeline::{PropertyAddress, TimelineEditor, TimelineItem},
    },
    ui::animation_curve::AnimationTarget,
};
#[derive(Clone, Debug)]
pub(crate) struct AnimationPresentation {
    pub label: String,
    pub suffix: String,
    pub step: f64,
    pub value_factor: f64,
}

#[derive(Clone)]
pub(crate) struct NumberSpec {
    pub suffix: String,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub scalar_type: ScalarPropertyType,
    pub is_size: bool,
}

#[derive(Clone)]
pub(crate) struct NumberAnimationSource {
    pub property_id: String,
    pub element_id: Option<crate::domain::property::PropertyElementId>,
    pub scalar_index: Option<usize>,
    pub value_factor: f64,
}

pub(crate) fn number_spec(
    property: &PropertySchema,
    scalar_index: Option<usize>,
    is_size: bool,
) -> Option<NumberSpec> {
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
    let scalar_type = value_type.scalar_at(scalar_index)?.clone();
    let (type_min, type_max) = match &scalar_type {
        ScalarPropertyType::F32 => (f64::from(f32::MIN), f64::from(f32::MAX)),
        ScalarPropertyType::I32 => (f64::from(i32::MIN), f64::from(i32::MAX)),
        ScalarPropertyType::U32 => (0., f64::from(u32::MAX)),
        _ => return None,
    };
    let constraints = property.configuration_constraints(scalar_index);
    let ui = property.configuration_ui(scalar_index);
    let min = constraints.min.unwrap_or(type_min).max(type_min);
    let max = constraints.max.unwrap_or(type_max).min(type_max);
    let step = f64::from(ui.step());
    let (min, max, step) = match &scalar_type {
        ScalarPropertyType::I32 | ScalarPropertyType::U32 => {
            (min.ceil(), max.floor(), step.max(1.))
        }
        _ => (min, max, step),
    };
    Some(NumberSpec {
        suffix: ui.unit().to_owned(),
        min,
        max,
        step,
        scalar_type,
        is_size,
    })
}

pub(crate) fn number_animation_source(
    item: &TimelineItem,
    address: &PropertyAddress,
    spec: &NumberSpec,
) -> Option<NumberAnimationSource> {
    if item
        .animation_track(
            address.effect_id,
            &address.property_id,
            address.element_id,
            address.scalar_index,
        )
        .is_some()
    {
        return Some(NumberAnimationSource {
            property_id: address.property_id.clone(),
            element_id: address.element_id,
            scalar_index: address.scalar_index,
            value_factor: 1.,
        });
    }
    if address.effect_id.is_some() || !spec.is_size || address.scalar_index != Some(1) {
        return None;
    }
    let schema = item.schema()?;
    if !item.aspect_ratio_locked {
        return None;
    }
    let size = schema.size_property()?;
    let value = item.properties.property(size.id())?;
    let width = value.scalar_at(Some(0))?.numeric_scalar()?;
    let height = value.scalar_at(Some(1))?.numeric_scalar()?;
    if !width.is_finite() || !height.is_finite() || width <= 0. || height <= 0. {
        return None;
    }
    item.animation_track(None, size.id(), None, Some(0))?;
    Some(NumberAnimationSource {
        property_id: size.id().to_owned(),
        element_id: None,
        scalar_index: Some(0),
        value_factor: height / width,
    })
}

pub(crate) fn calculate(
    editor: &TimelineEditor,
    item: &TimelineItem,
    target: &AnimationTarget,
) -> Option<AnimationPresentation> {
    let property = animation_property(editor, item, target)?;
    let scalar_index = target.scalar_index;
    let value_type = match property.ty() {
        PropertyType::Value(value_type)
        | PropertyType::Array {
            element_type: value_type,
            ..
        } => value_type,
    };
    let scalar_type = value_type.scalar_at(scalar_index)?.clone();
    let element_index = target.element_id.and_then(|id| {
        item.property_values(target.effect_id)?
            .property(&target.property_id)?
            .element_index(id)
    });
    let label = animation_label(&property, target, element_index);
    if matches!(scalar_type, ScalarPropertyType::Color) {
        item.animation_track(
            target.effect_id,
            &target.property_id,
            target.element_id,
            target.scalar_index,
        )?;
        return Some(AnimationPresentation {
            label,
            suffix: String::new(),
            step: 0.01,
            value_factor: 1.,
        });
    }
    let is_size = target.effect_id.is_none()
        && item.scene_id().is_none()
        && item
            .schema()
            .is_some_and(|schema| schema.is_size_property(&target.property_id));
    let spec = number_spec(&property, scalar_index, is_size)?;
    let display = number_animation_source(item, &target.address, &spec)?;
    if display.property_id != target.property_id
        || display.element_id != target.element_id
        || display.scalar_index != target.scalar_index
    {
        return None;
    }
    Some(AnimationPresentation {
        label,
        suffix: spec.suffix.clone(),
        step: spec.step,
        value_factor: display.value_factor,
    })
}

fn animation_property(
    editor: &TimelineEditor,
    item: &TimelineItem,
    target: &AnimationTarget,
) -> Option<PropertySchema> {
    if let Some(scene_id) = item.scene_id()
        && let Some(property) = editor
            .scene(scene_id)?
            .arguments
            .iter()
            .find(|argument| argument.schema.id() == target.property_id)
            .map(|argument| argument.schema.clone())
    {
        return Some(property);
    }
    if let Some(effect_id) = target.effect_id {
        return item
            .effects
            .iter()
            .find(|effect| effect.id == effect_id)
            .and_then(|effect| effect.schema().property(&target.property_id))
            .cloned();
    }
    item.schema()?.property(&target.property_id).cloned()
}

fn animation_label(
    property: &PropertySchema,
    target: &AnimationTarget,
    element_index: Option<usize>,
) -> String {
    let label = element_index.map_or_else(
        || property.label().to_owned(),
        |index| format!("{} {}", property.label(), index + 1),
    );
    match target.scalar_index {
        Some(scalar_index) => property
            .configuration_label(Some(scalar_index))
            .map_or(label.clone(), |scalar_label| {
                format!("{label} {scalar_label}")
            }),
        None => label,
    }
}
