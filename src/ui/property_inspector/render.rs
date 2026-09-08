use super::*;

impl Render for PropertyInspector {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let selected = self.selected_view(cx);

        div()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<PropertyValueDrag>, window, cx| {
                    let drag = event.drag(cx).clone();
                    cx.set_active_drag_cursor_style(CursorStyle::ResizeLeftRight, window);
                    this.handle_value_drag(
                        &drag,
                        f32::from(event.event.position.x),
                        event.event.modifiers.shift,
                        window,
                        cx,
                    );
                },
            ))
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<SceneArgumentValueDrag>, window, cx| {
                    let drag = event.drag(cx).clone();
                    cx.set_active_drag_cursor_style(CursorStyle::ResizeLeftRight, window);
                    this.handle_scene_argument_value_drag(
                        &drag,
                        f32::from(event.event.position.x),
                        event.event.modifiers.shift,
                        window,
                        cx,
                    );
                },
            ))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.finish_value_drag(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.finish_value_drag(cx)),
            )
            .bg(colors.background)
            .text_color(colors.foreground)
            .when_some(selected, |this, selected| {
                this.child(self.selected_view_element(selected, cx))
            })
    }
}
