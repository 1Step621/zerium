use super::*;
use zerium_core::plugin::EditorCapability;

impl Preview {
    pub(in crate::ui::panels::preview) fn selected_editor_overlay(
        &self,
        frame: Frame,
        cx: &Context<Self>,
    ) -> Option<PreviewEditorOverlay> {
        if self.transport.read(cx).is_playing() {
            return None;
        }
        let editor = self.editor.read(cx);
        let selected = editor.active_selected_items();
        let [selected] = selected.as_slice() else {
            return None;
        };
        let current = editor
            .active_items_at(frame)
            .into_iter()
            .find_map(|(_, item)| (item.source_id == selected.id).then_some(item))?;
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
            let size = item_size(&current, effect_id, size)?;
            let center = item_center(&current, effect_id, position, size, origin);
            let PropertyValue::Array(elements) = current_properties.property(points)? else {
                return None;
            };
            let points = elements
                .iter()
                .map(|element| {
                    let point = f32_pair(element.value())?;
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
                .then(|| sample(&points, tension, closed))
        })()
        .unwrap_or_default();

        let mut controls = Vec::new();
        let mut motion_paths = Vec::new();
        if let Some(property) = position_property {
            let position = item_position(&current, effect_id, property.id());
            controls.extend(
                scalar_values(
                    selected,
                    &current,
                    effect_id,
                    property,
                    None,
                    TimelineTime::from_frame(frame),
                )
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
                motion_paths = build_motion_paths(editor, selected, effect_id, property);
            }
        }

        let mut bounds = None;
        if let Some((property, position, origin)) = size_editor
            && let Some(size) = item_size(&current, effect_id, property.id())
        {
            let position = item_position(&current, effect_id, position);
            let origin = item_origin(&current, effect_id, origin);
            let center = [0, 1].map(|axis| position[axis] + (0.5 - origin[axis]) * size[axis]);
            bounds = Some(PreviewBounds { center, size });
            for scalar in scalar_values(
                selected,
                &current,
                effect_id,
                property,
                None,
                TimelineTime::from_frame(frame),
            ) {
                for side in [0., 1.] {
                    if let Some(control) =
                        size_control(scalar.clone(), position, center, origin, side)
                    {
                        controls.push(control);
                    }
                }
            }
        }

        let mut points = Vec::new();
        if let Some((property, position, size, origin)) = points_editor
            && let Some(size) = item_size(&current, effect_id, size)
            && let Some(PropertyValue::Array(elements)) = current_properties.property(property.id())
        {
            let center = item_center(&current, effect_id, position, size, origin);
            for element in elements {
                let Some(point) = f32_pair(element.value())
                    .filter(|value| value.iter().all(|value| value.is_finite()))
                else {
                    continue;
                };
                let anchor =
                    [0, 1].map(|axis| center[axis] + (point[axis] / 100. - 0.5) * size[axis]);
                points.push(anchor);
                controls.extend(
                    scalar_values(
                        selected,
                        &current,
                        effect_id,
                        property,
                        Some(element.element_id()),
                        TimelineTime::from_frame(frame),
                    )
                    .into_iter()
                    .map(|scalar| point_control(scalar, anchor, center, size)),
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

fn scalar_values(
    item: &TimelineItem,
    current: &EvaluatedItem,
    effect_id: Option<EffectInstanceId>,
    property: &PropertySchema,
    element_id: Option<PropertyElementId>,
    time: TimelineTime,
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
        if let Some(track) = item.animation_track(effect_id, property.id(), element_id, Some(axis))
        {
            let frame = item.animation_clock(track).local_frame_at(time);
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
                            time_distance: (stop.frame().get() as f64 - frame).abs(),
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
                time_distance: 0.,
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

const MAX_PATH_WINDOWS: usize = 64;
const SAMPLES_PER_SEGMENT: usize = 32;

fn build_motion_paths(
    editor: &zerium_core::timeline::TimelineEditor,
    item: &TimelineItem,
    effect_id: Option<EffectInstanceId>,
    property: &PropertySchema,
) -> Vec<Vec<[f32; 2]>> {
    let tracks = (0..2)
        .filter_map(|axis| item.animation_track(effect_id, property.id(), None, Some(axis)))
        .map(|track| (item.animation_clock(track), track))
        .collect::<Vec<_>>();
    let span = item.duration(editor.frame_rate()).get().saturating_sub(1) as f64;
    let window_span = tracks.iter().map(|(clock, _)| clock.span_frames()).fold(
        item.duration(editor.frame_rate()).animation_span().get() as f64,
        f64::min,
    );
    let windows = (span / window_span).ceil() as usize;
    let stride = windows.div_ceil(MAX_PATH_WINDOWS).max(1);
    let mut indices = (0..windows).step_by(stride).collect::<Vec<_>>();
    if indices.last().copied() != windows.checked_sub(1)
        && let Some(last) = windows.checked_sub(1)
    {
        indices.push(last);
    }
    let mut paths = Vec::new();
    for index in indices {
        let start =
            TimelineTime::from_frames(item.start().get() as f64 + index as f64 * window_span);
        let end = TimelineTime::from_frames(
            item.start().get() as f64 + ((index + 1) as f64 * window_span).min(span),
        );
        let mut boundaries = vec![start, end];
        for (clock, track) in &tracks {
            for stop in track.stops() {
                boundaries.extend(clock.occurrences(stop.frame(), start, end));
            }
        }
        boundaries.sort_by(|left, right| left.frames().total_cmp(&right.frames()));
        boundaries.dedup();
        // Sample each interval without its right endpoint, then append the
        // final endpoint once. Shared boundaries never need deduplication.
        let path = boundaries
            .windows(2)
            .flat_map(|interval| {
                let [start, end] = [interval[0], interval[1]];
                (0..SAMPLES_PER_SEGMENT).map(move |sample| {
                    start.offset(
                        (end.frames() - start.frames()) * sample as f64
                            / SAMPLES_PER_SEGMENT as f64,
                    )
                })
            })
            .chain(boundaries.last().copied())
            .filter_map(|time| position_at_time(editor, item, effect_id, property, time))
            .collect::<Vec<_>>();
        if !path.is_empty() {
            paths.push(path);
        }
    }
    paths
}

// Match the spline segment and chord sampling used by the built-in shaders.
fn sample(points: &[[f32; 2]], tension: f32, closed: bool) -> Vec<[f32; 2]> {
    if points.len() < 2 {
        return Vec::new();
    }
    let closed = closed && points.len() >= 3;
    let segment_count = if closed {
        points.len()
    } else {
        points.len() - 1
    };
    let handle_scale = (1. - (tension / 100.).clamp(-1., 1.)) / 6.;
    let mut sampled = Vec::new();
    for segment in 0..segment_count {
        let count = points.len();
        let (p0, p1, p2, p3) = if closed {
            (
                points[(segment + count - 1) % count],
                points[segment],
                points[(segment + 1) % count],
                points[(segment + 2) % count],
            )
        } else {
            (
                points[segment.saturating_sub(1)],
                points[segment],
                points[segment + 1],
                points[(segment + 2).min(count - 1)],
            )
        };
        let b0 = p1;
        let b1 = add(p1, scale(sub(p2, p0), handle_scale));
        let b2 = sub(p2, scale(sub(p3, p1), handle_scale));
        let b3 = p2;
        let second_at_start = scale(add(sub(b2, scale(b1, 2.)), b0), 6.);
        let second_at_end = scale(add(sub(b3, scale(b2, 2.)), b1), 6.);
        let bend = length(second_at_start).max(length(second_at_end));
        let steps = bend.sqrt().ceil().clamp(8., 256.) as usize;
        if segment == 0 {
            sampled.push(b0);
        }
        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            let u = 1. - t;
            sampled.push(add(
                add(scale(b0, u * u * u), scale(b1, 3. * u * u * t)),
                add(scale(b2, 3. * u * t * t), scale(b3, t * t * t)),
            ));
        }
    }
    sampled
}

fn add(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn scale(a: [f32; 2], amount: f32) -> [f32; 2] {
    [a[0] * amount, a[1] * amount]
}

fn length(a: [f32; 2]) -> f32 {
    a[0].hypot(a[1])
}

fn f32_pair(value: &PropertyValue) -> Option<[f32; 2]> {
    Some([
        value.scalar_at(Some(0))?.numeric_scalar()? as f32,
        value.scalar_at(Some(1))?.numeric_scalar()? as f32,
    ])
}

fn item_position(
    item: &EvaluatedItem,
    effect_id: Option<EffectInstanceId>,
    property_id: &str,
) -> [f32; 2] {
    item.property_values(effect_id)
        .and_then(|properties| properties.property(property_id))
        .and_then(f32_pair)
        .filter(|value| value.iter().all(|value| value.is_finite()))
        .unwrap_or([0., 0.])
}

fn item_origin(
    item: &EvaluatedItem,
    effect_id: Option<EffectInstanceId>,
    property_id: Option<&str>,
) -> [f32; 2] {
    property_id
        .and_then(|id| item.property_values(effect_id)?.property(id))
        .and_then(|value| {
            let PropertyValue::Tuple(values) = value else {
                return None;
            };
            let [PropertyValue::Enum(x), PropertyValue::Enum(y)] = values.as_slice() else {
                return None;
            };
            Some([*x as f32 * 0.5, *y as f32 * 0.5])
        })
        .unwrap_or([0.5, 0.5])
}

fn item_center(
    item: &EvaluatedItem,
    effect_id: Option<EffectInstanceId>,
    position: &str,
    size: [f32; 2],
    origin: Option<&str>,
) -> [f32; 2] {
    let position = item_position(item, effect_id, position);
    let origin = item_origin(item, effect_id, origin);
    [0, 1].map(|axis| position[axis] + (0.5 - origin[axis]) * size[axis])
}

fn position_at_time(
    editor: &zerium_core::timeline::TimelineEditor,
    item: &TimelineItem,
    effect_id: Option<EffectInstanceId>,
    property: &PropertySchema,
    time: TimelineTime,
) -> Option<[f32; 2]> {
    f32_pair(&editor.evaluated_property_value(
        &PropertyAddress {
            item_id: item.id,
            effect_id,
            property_id: property.id().to_owned(),
            element_id: None,
            scalar_index: None,
        },
        time,
    )?)
    .filter(|value| value.iter().all(|value| value.is_finite()))
}

fn item_size(
    item: &EvaluatedItem,
    effect_id: Option<EffectInstanceId>,
    property_id: &str,
) -> Option<[f32; 2]> {
    f32_pair(item.property_values(effect_id)?.property(property_id)?)
        .filter(|value| value.iter().all(|value| value.is_finite() && *value > 0.))
}
