use crate::domain::timeline::{PropertyAddress, TimelineItem};

pub(crate) struct NumberAnimationSource {
    pub address: PropertyAddress,
    pub value_factor: f64,
}

pub(crate) fn number_animation_source(
    item: &TimelineItem,
    address: &PropertyAddress,
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
            address: address.clone(),
            value_factor: 1.,
        });
    }
    // With a locked aspect ratio, the height control displays the width track.
    if address.effect_id.is_some()
        || address.element_id.is_some()
        || address.scalar_index != Some(1)
    {
        return None;
    }
    let schema = item.schema()?;
    if !schema.is_size_property(&address.property_id) || !item.aspect_ratio_locked {
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
        address: PropertyAddress {
            item_id: item.id,
            effect_id: None,
            property_id: size.id().to_owned(),
            element_id: None,
            scalar_index: Some(0),
        },
        value_factor: height / width,
    })
}
