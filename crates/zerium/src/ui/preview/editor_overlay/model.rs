use super::*;

impl Preview {
    pub(in crate::ui::preview) fn selected_editor_overlay(
        &self,
        frame: Frame,
        cx: &Context<Self>,
    ) -> Option<PreviewEditorOverlay> {
        if self.transport.read(cx).is_playing() {
            return None;
        }
        let editor = self.editor.read(cx);
        let selected = editor.selected_items();
        let [selected] = selected.as_slice() else {
            return None;
        };
        let current = editor
            .active_items_at(frame)
            .into_iter()
            .find_map(|(_, item)| (item.id == selected.id).then_some(item))?;
        let effect_id = editor.active_edit_effect();
        let (position_property, size_property, points_property, spline_properties) = match effect_id
        {
            Some(effect_id) => {
                let effect = selected
                    .effects
                    .iter()
                    .find(|effect| effect.id == effect_id)?;
                if !effect.schema().has_editor() {
                    return None;
                }
                (
                    effect.schema().position_property(),
                    effect.schema().size_property(),
                    effect.schema().points_property(),
                    effect.schema().spline_properties(),
                )
            }
            None => {
                let schema = selected.schema()?;
                (
                    schema.position_property(),
                    schema.size_property(),
                    schema.points_property(),
                    schema.spline_properties(),
                )
            }
        };
        let current_properties = Self::overlay_properties(&current, effect_id)?;

        let spline_path = (|| {
            let (points_property, position_property, size_property) =
                (points_property?, position_property?, size_property?);
            let (tension_property, closed_property) = spline_properties?;
            let size = Self::item_size(&current, effect_id, size_property.id())?;
            let center = Self::item_position(&current, effect_id, Some(position_property.id()));
            let PropertyValue::Array(elements) =
                current_properties.property(points_property.id())?
            else {
                return None;
            };
            let points = elements
                .iter()
                .map(|element| {
                    let point = Self::f32_pair(element.value())?;
                    point.iter().all(|value| value.is_finite()).then_some([
                        center[0] + (point[0] / 100. - 0.5) * size[0],
                        center[1] + (point[1] / 100. - 0.5) * size[1],
                    ])
                })
                .collect::<Option<Vec<_>>>()?;
            let PropertyValue::F32(tension) = current_properties.property(tension_property.id())?
            else {
                return None;
            };
            let PropertyValue::Bool(closed) = current_properties.property(closed_property.id())?
            else {
                return None;
            };
            tension
                .is_finite()
                .then(|| spline::sample(&points, *tension, *closed))
        })()
        .unwrap_or_default();

        let mut positions = Vec::new();
        let mut motion_path = Vec::new();
        if let Some(property) = position_property.filter(|property| property.is_editable(None)) {
            let progresses = Self::animation_progresses(selected, effect_id, property.id(), None);
            if progresses.is_empty() {
                let value = Self::item_position(&current, effect_id, Some(property.id()));
                positions.push(PreviewPositionOverlay {
                    address: PropertyAddress {
                        item_id: selected.id,
                        effect_id,
                        property_id: property.id().to_owned(),
                        element_id: None,
                        scalar_index: None,
                    },
                    value,
                    target: PreviewEditTarget::Property,
                });
            } else {
                positions.extend(progresses.iter().map(|progress| {
                    let item = Self::evaluated_at_progress(selected, *progress);
                    PreviewPositionOverlay {
                        address: PropertyAddress {
                            item_id: selected.id,
                            effect_id,
                            property_id: property.id().to_owned(),
                            element_id: None,
                            scalar_index: None,
                        },
                        value: Self::item_position(&item, effect_id, Some(property.id())),
                        target: PreviewEditTarget::Keyframe(*progress),
                    }
                }));
                let samples =
                    ((selected.animation_span_frames().ceil() as usize) * 2).clamp(32, 256);
                motion_path.extend((0..=samples).map(|index| {
                    let progress = index as f32 / samples as f32;
                    Self::position_at_progress(selected, effect_id, property.id(), progress)
                        .unwrap_or([0., 0.])
                }));
            }
        }

        let mut sizes = Vec::new();
        if let Some(property) = size_property.filter(|property| property.is_editable(None)) {
            let progresses = Self::animation_progresses(selected, effect_id, property.id(), None);
            let progresses = if progresses.is_empty() {
                vec![None]
            } else {
                progresses.into_iter().map(Some).collect()
            };
            for progress in progresses {
                let item = progress.map_or_else(
                    || current.clone(),
                    |progress| Self::evaluated_at_progress(selected, progress),
                );
                let Some(size) = Self::item_size(&item, effect_id, property.id()) else {
                    continue;
                };
                sizes.push(PreviewSizeOverlay {
                    item_id: selected.id,
                    property_id: property.id().to_owned(),
                    center: Self::item_position(
                        &item,
                        effect_id,
                        position_property.map(|value| value.id()),
                    ),
                    size,
                    target: PreviewEditTarget::from_progress(progress),
                });
            }
        }

        let mut points = Vec::new();
        if let (Some(property), Some(position_property), Some(size_property)) = (
            points_property.filter(|property| property.is_editable(None)),
            position_property,
            size_property,
        ) && let Some(PropertyValue::Array(elements)) =
            current_properties.property(property.id())
        {
            for (index, element) in elements.iter().enumerate() {
                let element_id = element.element_id();
                let progresses = Self::animation_progresses(
                    selected,
                    effect_id,
                    property.id(),
                    Some(element_id),
                );
                let progresses = if progresses.is_empty() {
                    vec![None]
                } else {
                    progresses.into_iter().map(Some).collect()
                };
                for progress in progresses {
                    let item = progress.map_or_else(
                        || current.clone(),
                        |progress| Self::evaluated_at_progress(selected, progress),
                    );
                    let Some(size) = Self::item_size(&item, effect_id, size_property.id()) else {
                        continue;
                    };
                    let Some(value) = Self::overlay_properties(&item, effect_id)
                        .and_then(|properties| properties.property(property.id()))
                        .cloned()
                    else {
                        continue;
                    };
                    let Some(element) = value.element(Some(element_id)) else {
                        continue;
                    };
                    let Some(point) = Self::f32_pair(element)
                        .filter(|value| value.iter().all(|value| value.is_finite()))
                    else {
                        continue;
                    };
                    points.push(PreviewPointOverlay {
                        item_id: selected.id,
                        center: Self::item_position(&item, effect_id, Some(position_property.id())),
                        size,
                        points: PreviewPointsProperty {
                            property_id: property.id().to_owned(),
                            value,
                        },
                        point: PreviewPoint {
                            element_id,
                            value: point,
                            target: PreviewEditTarget::from_progress(progress),
                        },
                        index,
                    });
                }
            }
        }

        (!positions.is_empty()
            || !sizes.is_empty()
            || !points.is_empty()
            || !spline_path.is_empty())
        .then_some(PreviewEditorOverlay {
            effect_id,
            positions,
            sizes,
            points,
            motion_path,
            spline_path,
        })
    }
}
