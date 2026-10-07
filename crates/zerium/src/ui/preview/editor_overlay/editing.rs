use super::*;
use zerium_core::timeline::{EditScope, PropertyAddress};

impl Preview {
    fn update_pair_property(
        editor: &mut TimelineEditor,
        item_id: zerium_core::timeline::ItemId,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
        target: PreviewEditTarget,
        value: [f32; 2],
    ) -> bool {
        let Some(item) = editor
            .single_selected_item()
            .filter(|item| item.id == item_id)
        else {
            return false;
        };
        let schema = match effect_id {
            Some(effect_id) => item
                .effects
                .iter()
                .find(|effect| effect.id == effect_id)
                .and_then(|effect| effect.schema().property(property_id)),
            None => item
                .schema()
                .and_then(|schema| schema.property(property_id)),
        };
        let Some(value) = schema
            .and_then(|property| property.constrained_value(&PropertyValue::f32_tuple(value)))
        else {
            return false;
        };
        let address = PropertyAddress {
            item_id,
            effect_id,
            property_id: property_id.to_owned(),
            element_id: None,
            scalar_index: None,
        };
        match target {
            PreviewEditTarget::Property => {
                editor.update_property(EditScope::Item(item_id), &address, value)
            }
            PreviewEditTarget::Keyframe(progress) => Self::f32_pair(&value).is_some_and(|value| {
                editor.set_property_animation_pair_stop_at(&address, progress, value)
            }),
        }
    }

    pub(in crate::ui::preview) fn move_editor_control_from_pointer(
        &mut self,
        drag: &PreviewEditorDrag,
        pointer: [f32; 2],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if drag.preview_id != cx.entity_id() {
            return;
        }
        match drag.kind {
            PreviewDragKind::Resize(handle) => {
                self.resize_from_pointer(drag, handle, pointer, window, cx);
            }
            PreviewDragKind::Position => {
                self.move_position_from_pointer(drag, pointer, window, cx);
            }
            PreviewDragKind::Point(element_id) => {
                self.move_point_from_pointer(drag, element_id, pointer, window, cx);
            }
        }
    }

    fn begin_resize(
        &mut self,
        overlay: &PreviewSizeOverlay,
        effect_id: Option<EffectInstanceId>,
        handle: PreviewResizeHandle,
        composition_units_per_pixel: f32,
        event: &MouseDownEvent,
    ) {
        if event.button != MouseButton::Left || composition_units_per_pixel <= 0. {
            return;
        }
        let Some(resize_scale) = handle.resize_scale(overlay.origin) else {
            return;
        };
        self.editor_drag = PreviewEditorDragState::Resize(PreviewResizeOrigin {
            item_id: overlay.item_id,
            effect_id,
            property_id: overlay.property_id.clone(),
            handle,
            pointer: [f32::from(event.position.x), f32::from(event.position.y)],
            size: overlay.size,
            resize_scale,
            target: overlay.target,
            composition_units_per_pixel,
        });
    }

    fn resize_from_pointer(
        &mut self,
        drag: &PreviewEditorDrag,
        handle: PreviewResizeHandle,
        pointer: [f32; 2],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let origin = match &self.editor_drag {
            PreviewEditorDragState::Resize(origin)
                if origin.item_id == drag.item_id
                    && origin.effect_id == drag.effect_id
                    && origin.handle == handle =>
            {
                origin.clone()
            }
            _ => return,
        };
        cx.set_active_drag_cursor_style(origin.handle.cursor(), window);
        let axis = usize::from(!origin.handle.changes_width());
        let mut size = origin.size;
        size[axis] = (size[axis]
            + (pointer[axis] - origin.pointer[axis])
                * origin.composition_units_per_pixel
                * origin.resize_scale)
            .max(1.);
        self.editor.update(cx, |editor, cx| {
            let changed = match origin.target {
                PreviewEditTarget::Property => {
                    editor
                        .single_selected_item()
                        .is_some_and(|item| item.id == origin.item_id)
                        && editor.update_property(
                            EditScope::Item(origin.item_id),
                            &PropertyAddress {
                                item_id: origin.item_id,
                                effect_id: origin.effect_id,
                                property_id: origin.property_id.clone(),
                                element_id: None,
                                scalar_index: Some(axis),
                            },
                            PropertyValue::F32(size[axis]),
                        )
                }
                PreviewEditTarget::Keyframe(_) => Self::update_pair_property(
                    editor,
                    origin.item_id,
                    origin.effect_id,
                    &origin.property_id,
                    origin.target,
                    size,
                ),
            };
            if changed {
                cx.notify();
            }
        });
    }

    fn begin_position_drag(
        &mut self,
        position: &PreviewPositionOverlay,
        composition_units_per_pixel: f32,
        event: &MouseDownEvent,
    ) {
        if event.button != MouseButton::Left || composition_units_per_pixel <= 0. {
            return;
        }
        self.editor_drag = PreviewEditorDragState::Position(PreviewPositionOrigin {
            address: position.address.clone(),
            pointer: [f32::from(event.position.x), f32::from(event.position.y)],
            position: position.value,
            target: position.target,
            composition_units_per_pixel,
        });
    }

    fn move_position_from_pointer(
        &mut self,
        drag: &PreviewEditorDrag,
        pointer: [f32; 2],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let origin = match &self.editor_drag {
            PreviewEditorDragState::Position(origin)
                if origin.address.item_id == drag.item_id
                    && origin.address.effect_id == drag.effect_id =>
            {
                origin.clone()
            }
            _ => return,
        };
        cx.set_active_drag_cursor_style(CursorStyle::ClosedHand, window);
        let position = [
            origin.position[0]
                + (pointer[0] - origin.pointer[0]) * origin.composition_units_per_pixel,
            origin.position[1]
                + (pointer[1] - origin.pointer[1]) * origin.composition_units_per_pixel,
        ];
        self.editor.update(cx, |editor, cx| {
            if Self::update_pair_property(
                editor,
                origin.address.item_id,
                origin.address.effect_id,
                &origin.address.property_id,
                origin.target,
                position,
            ) {
                cx.notify();
            }
        });
    }

    pub(super) fn position_handle(
        &self,
        position: PreviewPositionOverlay,
        resolution: zerium_core::timeline::ProjectResolution,
        composition_units_per_pixel: f32,
        color: Hsla,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        const HANDLE_SIZE: f32 = 14.;
        let item_id = position.address.item_id;
        let effect_id = position.address.effect_id;
        let drag = PreviewEditorDrag {
            preview_id: cx.entity_id(),
            item_id,
            effect_id,
            kind: PreviewDragKind::Position,
        };
        let begin_position = position.clone();
        let control_id = SharedString::from(format!(
            "preview-position-handle-{}-{}",
            item_id.get(),
            position.target.key()
        ));
        div()
            .id(control_id)
            .absolute()
            .left(relative(
                position.value[0] / resolution.width() as f32 + 0.5,
            ))
            .top(relative(
                position.value[1] / resolution.height() as f32 + 0.5,
            ))
            .ml(px(-HANDLE_SIZE / 2.))
            .mt(px(-HANDLE_SIZE / 2.))
            .size(px(HANDLE_SIZE))
            .rounded_full()
            .border_2()
            .border_color(gpui::white())
            .bg(color)
            .cursor_grab()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event, _, _| {
                    this.begin_position_drag(&begin_position, composition_units_per_pixel, event);
                }),
            )
            .on_drag(drag, move |drag, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| drag.clone())
            })
    }

    fn begin_point_drag(
        &mut self,
        overlay: &PreviewPointOverlay,
        effect_id: Option<EffectInstanceId>,
        composition_units_per_pixel: f32,
        event: &MouseDownEvent,
    ) {
        if event.button != MouseButton::Left || composition_units_per_pixel <= 0. {
            return;
        }
        self.editor_drag = PreviewEditorDragState::Point(PreviewPointOrigin {
            item_id: overlay.item_id,
            effect_id,
            property_id: overlay.points.property_id.clone(),
            element_id: overlay.point.element_id,
            pointer: [f32::from(event.position.x), f32::from(event.position.y)],
            point: overlay.point.value,
            size: overlay.size,
            value: overlay.points.value.clone(),
            target: overlay.point.target,
            composition_units_per_pixel,
        });
    }

    fn move_point_from_pointer(
        &mut self,
        drag: &PreviewEditorDrag,
        element_id: PropertyElementId,
        pointer: [f32; 2],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let origin = match &self.editor_drag {
            PreviewEditorDragState::Point(origin)
                if origin.item_id == drag.item_id
                    && origin.effect_id == drag.effect_id
                    && origin.element_id == element_id =>
            {
                origin.clone()
            }
            _ => return,
        };
        cx.set_active_drag_cursor_style(CursorStyle::Crosshair, window);
        let point = [
            origin.point[0]
                + (pointer[0] - origin.pointer[0]) * origin.composition_units_per_pixel
                    / origin.size[0]
                    * 100.,
            origin.point[1]
                + (pointer[1] - origin.pointer[1]) * origin.composition_units_per_pixel
                    / origin.size[1]
                    * 100.,
        ];
        let mut value = origin.value.clone();
        let PropertyValue::Array(elements) = &mut value else {
            return;
        };
        let Some(element) = elements
            .iter_mut()
            .find(|element| element.element_id() == origin.element_id)
        else {
            return;
        };
        *element.value_mut() = PropertyValue::f32_tuple(point);
        self.editor.update(cx, |editor, cx| {
            if editor
                .single_selected_item()
                .is_none_or(|item| item.id != origin.item_id)
            {
                return;
            }
            let value = editor.single_selected_item().and_then(|item| {
                let schema = match origin.effect_id {
                    Some(effect_id) => item
                        .effects
                        .iter()
                        .find(|effect| effect.id == effect_id)?
                        .schema()
                        .property(&origin.property_id),
                    None => item.schema()?.property(&origin.property_id),
                }?;
                schema.constrained_value(&value)
            });
            let changed = value.is_some_and(|value| match origin.target {
                PreviewEditTarget::Keyframe(progress) => value
                    .element(Some(origin.element_id))
                    .and_then(Self::f32_pair)
                    .is_some_and(|value| {
                        editor.set_property_animation_pair_stop_at(
                            &PropertyAddress {
                                item_id: origin.item_id,
                                effect_id: origin.effect_id,
                                property_id: origin.property_id.clone(),
                                element_id: Some(origin.element_id),
                                scalar_index: None,
                            },
                            progress,
                            value,
                        )
                    }),
                PreviewEditTarget::Property => editor.update_property(
                    EditScope::Item(origin.item_id),
                    &PropertyAddress {
                        item_id: origin.item_id,
                        effect_id: origin.effect_id,
                        property_id: origin.property_id.clone(),
                        element_id: None,
                        scalar_index: None,
                    },
                    value,
                ),
            });
            if changed {
                cx.notify();
            }
        });
    }

    pub(super) fn point_handle(
        &self,
        overlay: PreviewPointOverlay,
        effect_id: Option<EffectInstanceId>,
        resolution: zerium_core::timeline::ProjectResolution,
        composition_units_per_pixel: f32,
        color: Hsla,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        const HANDLE_SIZE: f32 = 9.;
        let item_id = overlay.item_id;
        let size = overlay.size;
        let point = &overlay.point;
        let position = [
            overlay.center[0] + (point.value[0] / 100. - 0.5) * size[0],
            overlay.center[1] + (point.value[1] / 100. - 0.5) * size[1],
        ];
        let drag = PreviewEditorDrag {
            preview_id: cx.entity_id(),
            item_id,
            effect_id,
            kind: PreviewDragKind::Point(point.element_id),
        };
        div()
            .id(SharedString::from(format!(
                "preview-point-handle-{}-{}-{}",
                item_id.get(),
                overlay.index,
                point.target.key()
            )))
            .absolute()
            .left(relative(position[0] / resolution.width() as f32 + 0.5))
            .top(relative(position[1] / resolution.height() as f32 + 0.5))
            .ml(px(-HANDLE_SIZE / 2.))
            .mt(px(-HANDLE_SIZE / 2.))
            .size(px(HANDLE_SIZE))
            .rounded_full()
            .border_1()
            .border_color(color)
            .bg(gpui::white())
            .cursor_crosshair()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event, _, _| {
                    this.begin_point_drag(&overlay, effect_id, composition_units_per_pixel, event);
                }),
            )
            .on_drag(drag, move |drag, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| drag.clone())
            })
    }

    pub(super) fn resize_handle(
        &self,
        overlay: &PreviewSizeOverlay,
        effect_id: Option<EffectInstanceId>,
        handle: PreviewResizeHandle,
        composition_units_per_pixel: f32,
        color: Hsla,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        const HANDLE_SIZE: f32 = 10.;
        let drag = PreviewEditorDrag {
            preview_id: cx.entity_id(),
            item_id: overlay.item_id,
            effect_id,
            kind: PreviewDragKind::Resize(handle),
        };
        let begin_overlay = overlay.clone();
        let direction = handle.direction();
        div()
            .id(SharedString::from(format!(
                "preview-resize-handle-{}-{}",
                overlay.item_id.get(),
                handle as u8
            )))
            .absolute()
            .when(direction[0] < 0., |this| this.left(px(-HANDLE_SIZE / 2.)))
            .when(direction[0] > 0., |this| this.right(px(-HANDLE_SIZE / 2.)))
            .when(direction[1] < 0., |this| this.top(px(-HANDLE_SIZE / 2.)))
            .when(direction[1] > 0., |this| this.bottom(px(-HANDLE_SIZE / 2.)))
            .when(direction[0] == 0., |this| {
                this.left(relative(0.5)).ml(px(-HANDLE_SIZE / 2.))
            })
            .when(direction[1] == 0., |this| {
                this.top(relative(0.5)).mt(px(-HANDLE_SIZE / 2.))
            })
            .size(px(HANDLE_SIZE))
            .rounded_sm()
            .border_1()
            .border_color(color)
            .bg(gpui::white())
            .when(handle.changes_width(), |this| this.cursor_col_resize())
            .when(!handle.changes_width(), |this| this.cursor_row_resize())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event, _, _| {
                    this.begin_resize(
                        &begin_overlay,
                        effect_id,
                        handle,
                        composition_units_per_pixel,
                        event,
                    );
                }),
            )
            .on_drag(drag, move |drag, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| drag.clone())
            })
    }
}
