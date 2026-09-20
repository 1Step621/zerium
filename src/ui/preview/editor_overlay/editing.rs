use super::*;

impl Preview {
    fn update_pair_property(
        editor: &mut TimelineEditor,
        item_id: crate::domain::timeline::ItemId,
        property_id: &str,
        target: PreviewEditTarget,
        value: [f32; 2],
    ) -> bool {
        let Some(item) = editor.selected_item().filter(|item| item.id == item_id) else {
            return false;
        };
        let Some(value) = item
            .schema()
            .and_then(|schema| schema.property(property_id))
            .and_then(|property| property.constrained_value(&PropertyValue::f32_tuple(value)))
        else {
            return false;
        };
        match target {
            PreviewEditTarget::Property => editor.update_selected_property(property_id, value),
            PreviewEditTarget::Keyframe(progress) => Self::f32_pair(&value).is_some_and(|value| {
                editor.set_selected_property_animation_pair_stop_at(
                    property_id,
                    None,
                    progress,
                    value,
                )
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
        handle: PreviewResizeHandle,
        composition_units_per_pixel: f32,
        event: &MouseDownEvent,
    ) {
        if event.button != MouseButton::Left || composition_units_per_pixel <= 0. {
            return;
        }
        self.editor_drag.resize_origin = Some(PreviewResizeOrigin {
            item_id: overlay.item_id,
            property_id: overlay.property_id.clone(),
            handle,
            pointer: [f32::from(event.position.x), f32::from(event.position.y)],
            size: overlay.size,
            aspect_ratio: overlay.aspect_ratio,
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
        let Some(origin) = self
            .editor_drag
            .resize_origin
            .as_ref()
            .filter(|origin| origin.item_id == drag.item_id && origin.handle == handle)
            .cloned()
        else {
            return;
        };
        cx.set_active_drag_cursor_style(origin.handle.cursor(), window);
        let direction = origin.handle.direction();
        let mut size = [
            (origin.size[0]
                + (pointer[0] - origin.pointer[0])
                    * origin.composition_units_per_pixel
                    * direction[0]
                    * 2.)
                .max(1.),
            (origin.size[1]
                + (pointer[1] - origin.pointer[1])
                    * origin.composition_units_per_pixel
                    * direction[1]
                    * 2.)
                .max(1.),
        ];
        if let Some(aspect_ratio) = origin.aspect_ratio {
            if origin.handle.changes_width() {
                size[1] = size[0] / aspect_ratio;
            } else {
                size[0] = size[1] * aspect_ratio;
            }
        }
        self.editor.update(cx, |editor, cx| {
            if Self::update_pair_property(
                editor,
                origin.item_id,
                &origin.property_id,
                origin.target,
                size,
            ) {
                cx.notify();
            }
        });
    }

    fn begin_position_drag(
        &mut self,
        item_id: crate::domain::timeline::ItemId,
        position: &PreviewPairProperty,
        composition_units_per_pixel: f32,
        event: &MouseDownEvent,
    ) {
        if event.button != MouseButton::Left || composition_units_per_pixel <= 0. {
            return;
        }
        self.editor_drag.position_origin = Some(PreviewPositionOrigin {
            item_id,
            property_id: position.property_id.clone(),
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
        let Some(origin) = self
            .editor_drag
            .position_origin
            .as_ref()
            .filter(|origin| origin.item_id == drag.item_id)
            .cloned()
        else {
            return;
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
                origin.item_id,
                &origin.property_id,
                origin.target,
                position,
            ) {
                cx.notify();
            }
        });
    }

    pub(super) fn position_handle(
        &self,
        item_id: crate::domain::timeline::ItemId,
        position: PreviewPairProperty,
        resolution: crate::domain::timeline::ProjectResolution,
        composition_units_per_pixel: f32,
        color: Hsla,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        const HANDLE_SIZE: f32 = 14.;
        let drag = PreviewEditorDrag {
            preview_id: cx.entity_id(),
            item_id,
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
                    this.begin_position_drag(
                        item_id,
                        &begin_position,
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

    fn begin_point_drag(
        &mut self,
        item_id: crate::domain::timeline::ItemId,
        points: &PreviewPointsProperty,
        point: &PreviewPoint,
        size: [f32; 2],
        composition_units_per_pixel: f32,
        event: &MouseDownEvent,
    ) {
        if event.button != MouseButton::Left || composition_units_per_pixel <= 0. {
            return;
        }
        self.editor_drag.point_origin = Some(PreviewPointOrigin {
            item_id,
            property_id: points.property_id.clone(),
            element_id: point.element_id,
            pointer: [f32::from(event.position.x), f32::from(event.position.y)],
            point: point.value,
            size,
            value: points.value.clone(),
            target: point.target,
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
        let Some(origin) = self
            .editor_drag
            .point_origin
            .as_ref()
            .filter(|origin| origin.item_id == drag.item_id && origin.element_id == element_id)
            .cloned()
        else {
            return;
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
                .selected_item()
                .is_none_or(|item| item.id != origin.item_id)
            {
                return;
            }
            let value = editor.selected_item().and_then(|item| {
                item.schema()?
                    .property(&origin.property_id)?
                    .constrained_value(&value)
            });
            let changed = value.is_some_and(|value| match origin.target {
                PreviewEditTarget::Keyframe(progress) => value
                    .element(Some(origin.element_id))
                    .and_then(Self::f32_pair)
                    .is_some_and(|value| {
                        editor.set_selected_property_animation_pair_stop_at(
                            &origin.property_id,
                            Some(origin.element_id),
                            progress,
                            value,
                        )
                    }),
                PreviewEditTarget::Property => {
                    editor.update_selected_property(&origin.property_id, value)
                }
            });
            if changed {
                cx.notify();
            }
        });
    }

    pub(super) fn point_handle(
        &self,
        overlay: PreviewPointOverlay,
        resolution: crate::domain::timeline::ProjectResolution,
        composition_units_per_pixel: f32,
        color: Hsla,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        const HANDLE_SIZE: f32 = 9.;
        let item_id = overlay.item_id;
        let size = overlay.size;
        let point = overlay.point;
        let position = [
            overlay.center[0] + (point.value[0] / 100. - 0.5) * size[0],
            overlay.center[1] + (point.value[1] / 100. - 0.5) * size[1],
        ];
        let drag = PreviewEditorDrag {
            preview_id: cx.entity_id(),
            item_id,
            kind: PreviewDragKind::Point(point.element_id),
        };
        let begin_points = overlay.points;
        let begin_point = point.clone();
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
                    this.begin_point_drag(
                        item_id,
                        &begin_points,
                        &begin_point,
                        size,
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

    pub(super) fn resize_handle(
        &self,
        overlay: &PreviewSizeOverlay,
        handle: PreviewResizeHandle,
        composition_units_per_pixel: f32,
        color: Hsla,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        const HANDLE_SIZE: f32 = 10.;
        let drag = PreviewEditorDrag {
            preview_id: cx.entity_id(),
            item_id: overlay.item_id,
            kind: PreviewDragKind::Resize(handle),
        };
        let begin_overlay = PreviewSizeOverlay {
            item_id: overlay.item_id,
            property_id: overlay.property_id.clone(),
            center: overlay.center,
            size: overlay.size,
            aspect_ratio: overlay.aspect_ratio,
            target: overlay.target,
        };
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
                    this.begin_resize(&begin_overlay, handle, composition_units_per_pixel, event);
                }),
            )
            .on_drag(drag, move |drag, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| drag.clone())
            })
    }
}
