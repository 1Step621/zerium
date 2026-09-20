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
        let schema = selected.schema()?;
        let position_property = schema.position_property();
        let size_property = schema.size_property();

        let mut positions = Vec::new();
        let mut motion_path = Vec::new();
        if let Some(property) = position_property.filter(|property| property.is_editable(None)) {
            let progresses = Self::animation_progresses(selected, property.id(), None);
            if progresses.is_empty() {
                let value = Self::item_position(&current, Some(property.id()));
                positions.push(PreviewPairProperty {
                    property_id: property.id().to_owned(),
                    value,
                    target: PreviewEditTarget::Property,
                });
            } else {
                positions.extend(progresses.iter().map(|progress| {
                    let item = Self::evaluated_at_progress(selected, *progress);
                    PreviewPairProperty {
                        property_id: property.id().to_owned(),
                        value: Self::item_position(&item, Some(property.id())),
                        target: PreviewEditTarget::Keyframe(*progress),
                    }
                }));
                let samples =
                    ((selected.animation_span_frames().ceil() as usize) * 2).clamp(32, 256);
                motion_path.extend((0..=samples).map(|index| {
                    let progress = index as f32 / samples as f32;
                    Self::position_at_progress(selected, property.id(), progress)
                        .unwrap_or([0., 0.])
                }));
            }
        }

        let mut sizes = Vec::new();
        if let Some(property) = size_property.filter(|property| property.is_editable(None)) {
            let progresses = Self::animation_progresses(selected, property.id(), None);
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
                let Some(size) = Self::item_size(&item, property.id()) else {
                    continue;
                };
                sizes.push(PreviewSizeOverlay {
                    item_id: selected.id,
                    property_id: property.id().to_owned(),
                    center: Self::item_position(&item, position_property.map(|value| value.id())),
                    size,
                    aspect_ratio: item
                        .aspect_ratio_locked
                        .then_some(size[0] / size[1])
                        .filter(|ratio| ratio.is_finite() && *ratio > 0.),
                    target: PreviewEditTarget::from_progress(progress),
                });
            }
        }

        let mut points = Vec::new();
        if let (Some(property), Some(position_property), Some(size_property)) = (
            schema
                .points_property()
                .filter(|property| property.is_editable(None)),
            position_property,
            size_property,
        ) && let Some(PropertyValue::Array(elements)) =
            current.properties.property(property.id())
        {
            for (index, element) in elements.iter().enumerate() {
                let element_id = element.element_id();
                let progresses =
                    Self::animation_progresses(selected, property.id(), Some(element_id));
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
                    let Some(size) = Self::item_size(&item, size_property.id()) else {
                        continue;
                    };
                    let Some(value) = item.properties.property(property.id()).cloned() else {
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
                        center: Self::item_position(&item, Some(position_property.id())),
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

        (!positions.is_empty() || !sizes.is_empty() || !points.is_empty()).then_some(
            PreviewEditorOverlay {
                item_id: selected.id,
                positions,
                sizes,
                points,
                motion_path,
            },
        )
    }
}
