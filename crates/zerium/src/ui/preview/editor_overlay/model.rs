use super::*;
use zerium_core::plugin::EditorCapability;

impl Preview {
    fn scalar_values(
        item: &TimelineItem,
        current: &TimelineItem,
        effect_id: Option<EffectInstanceId>,
        property: &PropertySchema,
        element_id: Option<PropertyElementId>,
    ) -> Vec<PreviewScalarValue> {
        let mut values = Vec::new();
        for axis in 0..2 {
            if !property.is_editable(Some(axis)) {
                continue;
            }
            let address = PropertyAddress {
                item_id: item.id,
                effect_id,
                property_id: property.id().to_owned(),
                element_id,
                scalar_index: Some(axis),
            };
            if let Some(track) =
                item.animation_track(effect_id, property.id(), element_id, Some(axis))
            {
                values.extend(
                    track
                        .stops()
                        .iter()
                        .enumerate()
                        .filter_map(|(index, stop)| {
                            let value = stop.value().numeric_scalar()? as f32;
                            value.is_finite().then_some(PreviewScalarValue {
                                address: address.clone(),
                                value,
                                stop: Some(index),
                            })
                        }),
                );
            } else if let Some(value) = current
                .property_values(effect_id)
                .and_then(|values| address.path().value(values))
                .and_then(|value| value.numeric_scalar())
                .map(|value| value as f32)
                .filter(|value| value.is_finite())
            {
                values.push(PreviewScalarValue {
                    address,
                    value,
                    stop: None,
                });
            }
        }
        values
    }

    fn size_control(
        scalar: PreviewScalarValue,
        position: [f32; 2],
        center: [f32; 2],
        origin: [f32; 2],
        side: f32,
    ) -> Option<PreviewScalarControl> {
        let axis = scalar.axis();
        let units_per_value = side - origin[axis];
        if units_per_value == 0. {
            return None;
        }
        let mut anchor = center;
        anchor[axis] = position[axis] + units_per_value * scalar.value;
        Some(PreviewScalarControl {
            scalar,
            position: anchor,
            units_per_value,
        })
    }

    fn point_control(
        scalar: PreviewScalarValue,
        anchor: [f32; 2],
        center: [f32; 2],
        size: [f32; 2],
    ) -> PreviewScalarControl {
        let axis = scalar.axis();
        let mut position = anchor;
        position[axis] = center[axis] + (scalar.value / 100. - 0.5) * size[axis];
        PreviewScalarControl {
            scalar,
            position,
            units_per_value: size[axis] / 100.,
        }
    }

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
        let (editors, properties) = match effect_id {
            Some(effect_id) => {
                let effect = selected
                    .effects
                    .iter()
                    .find(|effect| effect.id == effect_id)?;
                (effect.schema().editor(), effect.schema().properties())
            }
            None => {
                let schema = selected.schema()?;
                (schema.editor(), schema.properties())
            }
        };
        let property = |id: &str| properties.iter().find(|property| property.id() == id);
        let position_property = editors.iter().find_map(|editor| match editor {
            EditorCapability::Position { property: id } => property(id),
            _ => None,
        });
        let size_editor = editors.iter().find_map(|editor| match editor {
            EditorCapability::Size {
                property: id,
                position,
                origin,
            } => Some((property(id)?, position.as_str(), origin.as_deref())),
            _ => None,
        });
        let points_editor = editors.iter().find_map(|editor| match editor {
            EditorCapability::Points {
                property: id,
                position,
                size,
                origin,
            } => Some((
                property(id)?,
                position.as_str(),
                size.as_str(),
                origin.as_deref(),
            )),
            _ => None,
        });
        let spline_editor = editors.iter().find_map(|editor| match editor {
            EditorCapability::Spline {
                points,
                position,
                size,
                tension,
                closed,
                origin,
            } => Some((points, position, size, tension, closed, origin.as_deref())),
            _ => None,
        });
        let current_properties = current.property_values(effect_id)?;

        let spline_path = (|| {
            let (points, position, size, tension, closed, origin) = spline_editor?;
            let size = Self::item_size(&current, effect_id, size)?;
            let center = Self::item_center(&current, effect_id, position, size, origin);
            let PropertyValue::Array(elements) = current_properties.property(points)? else {
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
            let tension = current_properties.property(tension)?.as_f32()?;
            let closed = current_properties.property(closed)?.as_bool()?;
            tension
                .is_finite()
                .then(|| spline::sample(&points, tension, closed))
        })()
        .unwrap_or_default();

        let mut controls = Vec::new();
        let mut motion_paths = Vec::new();
        if let Some(property) = position_property {
            let position = Self::item_position(&current, effect_id, property.id());
            controls.extend(
                Self::scalar_values(selected, &current, effect_id, property, None)
                    .into_iter()
                    .map(|scalar| {
                        let mut anchor = position;
                        anchor[scalar.axis()] = scalar.value;
                        PreviewScalarControl {
                            scalar,
                            position: anchor,
                            units_per_value: 1.,
                        }
                    }),
            );
            if (0..2).any(|axis| {
                selected
                    .animation_track(effect_id, property.id(), None, Some(axis))
                    .is_some()
            }) {
                motion_paths = Self::motion_paths(selected, effect_id, property);
            }
        }

        let mut bounds = None;
        if let Some((property, position, origin)) = size_editor
            && let Some(size) = Self::item_size(&current, effect_id, property.id())
        {
            let position = Self::item_position(&current, effect_id, position);
            let origin = Self::item_origin(&current, effect_id, origin);
            let center = [0, 1].map(|axis| position[axis] + (0.5 - origin[axis]) * size[axis]);
            bounds = Some(PreviewBounds { center, size });
            for scalar in Self::scalar_values(selected, &current, effect_id, property, None) {
                for side in [0., 1.] {
                    if let Some(control) =
                        Self::size_control(scalar.clone(), position, center, origin, side)
                    {
                        controls.push(control);
                    }
                }
            }
        }

        let mut points = Vec::new();
        if let Some((property, position, size, origin)) = points_editor
            && let Some(size) = Self::item_size(&current, effect_id, size)
            && let Some(PropertyValue::Array(elements)) = current_properties.property(property.id())
        {
            let center = Self::item_center(&current, effect_id, position, size, origin);
            for element in elements {
                let Some(point) = Self::f32_pair(element.value())
                    .filter(|value| value.iter().all(|value| value.is_finite()))
                else {
                    continue;
                };
                let anchor =
                    [0, 1].map(|axis| center[axis] + (point[axis] / 100. - 0.5) * size[axis]);
                points.push(anchor);
                controls.extend(
                    Self::scalar_values(
                        selected,
                        &current,
                        effect_id,
                        property,
                        Some(element.element_id()),
                    )
                    .into_iter()
                    .map(|scalar| Self::point_control(scalar, anchor, center, size)),
                );
            }
        }

        (!controls.is_empty() || bounds.is_some() || !points.is_empty() || !spline_path.is_empty())
            .then_some(PreviewEditorOverlay {
                controls,
                bounds,
                points,
                motion_paths,
                spline_path,
            })
    }
}
