use gpui::{
    DispatchPhase, HitboxBehavior, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PathBuilder, canvas, fill,
};

use super::*;

impl Preview {
    fn scalar_controls(
        &self,
        controls: Vec<PreviewScalarControl>,
        resolution: ProjectResolution,
        color: Hsla,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let preview = cx.entity();
        let active = self.editor_drag.as_ref().map(|drag| drag.controls.clone());
        canvas(
            move |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |bounds, hitbox, window, _| {
                let hovered = if hitbox.is_hovered(window) {
                    PreviewScalarControl::at_pointer(
                        &controls,
                        window.mouse_position(),
                        bounds,
                        resolution,
                    )
                } else {
                    Vec::new()
                };
                let highlighted = active.as_ref().unwrap_or(&hovered);
                let cursor = PreviewScalarControl::cursor(highlighted);
                if active.is_some() {
                    window.set_window_cursor_style(cursor);
                } else {
                    window.set_cursor_style(cursor, &hitbox);
                }
                for control in &controls {
                    let position = control.screen_position(bounds, resolution);
                    let axis = control.scalar.axis();
                    let extent = if axis == 0 {
                        size(px(2.), px(12.))
                    } else {
                        size(px(12.), px(2.))
                    };
                    let handle_bounds = Bounds::new(
                        position - point(extent.width / 2., extent.height / 2.),
                        extent,
                    );
                    let highlighted = highlighted.iter().any(|target| control.same_target(target));
                    if highlighted {
                        let guide_bounds = if axis == 0 {
                            Bounds::new(
                                point(position.x, bounds.origin.y),
                                size(px(1.), bounds.size.height),
                            )
                        } else {
                            Bounds::new(
                                point(bounds.origin.x, position.y),
                                size(bounds.size.width, px(1.)),
                            )
                        };
                        window.paint_quad(fill(guide_bounds, color.opacity(0.6)));
                    }
                    window.paint_quad(fill(handle_bounds, color));
                }
                window.on_mouse_event({
                    let preview = preview.clone();
                    let controls = controls.clone();
                    let hitbox = hitbox.clone();
                    move |event: &MouseDownEvent, phase, window, cx| {
                        if phase != DispatchPhase::Bubble
                            || event.button != MouseButton::Left
                            || !hitbox.is_hovered(window)
                        {
                            return;
                        }
                        let targets = PreviewScalarControl::at_pointer(
                            &controls,
                            event.position,
                            bounds,
                            resolution,
                        );
                        if targets.is_empty() {
                            return;
                        }
                        preview.update(cx, |preview, cx| {
                            preview.begin_editor_drag(
                                targets,
                                event.position,
                                resolution.width() as f32 / f32::from(bounds.size.width),
                                cx,
                            );
                        });
                        cx.stop_propagation();
                    }
                });
                window.on_mouse_event({
                    let preview = preview.clone();
                    move |event: &MouseUpEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                            preview.update(cx, |preview, cx| preview.end_editor_drag(cx));
                        }
                    }
                });
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    if preview.read(cx).editor_drag.is_some() {
                        preview.update(cx, |preview, cx| {
                            if event.pressed_button == Some(MouseButton::Left) {
                                preview.move_editor_controls_from_pointer(event.position, cx);
                            } else {
                                preview.end_editor_drag(cx);
                            }
                        });
                        cx.stop_propagation();
                        return;
                    }
                    let targets = if hitbox.is_hovered(window) {
                        PreviewScalarControl::at_pointer(
                            &controls,
                            event.position,
                            bounds,
                            resolution,
                        )
                    } else {
                        Vec::new()
                    };
                    if targets.len() != hovered.len()
                        || !targets.iter().zip(&hovered).all(|(a, b)| a.same_target(b))
                    {
                        cx.notify(preview.entity_id());
                    }
                });
            },
        )
        .absolute()
        .inset_0()
        .size_full()
    }

    fn size_overlay(
        bounds: PreviewBounds,
        resolution: zerium_core::timeline::ProjectResolution,
        color: Hsla,
    ) -> impl IntoElement {
        let resolution = [resolution.width() as f32, resolution.height() as f32];
        div()
            .absolute()
            .left(relative(
                (bounds.center[0] - bounds.size[0] * 0.5) / resolution[0] + 0.5,
            ))
            .top(relative(
                (bounds.center[1] - bounds.size[1] * 0.5) / resolution[1] + 0.5,
            ))
            .w(relative(bounds.size[0] / resolution[0]))
            .h(relative(bounds.size[1] / resolution[1]))
            .border_1()
            .border_color(color.opacity(0.6))
    }

    fn path_overlay(
        positions: Vec<[f32; 2]>,
        resolution: zerium_core::timeline::ProjectResolution,
        color: Hsla,
    ) -> impl IntoElement {
        canvas(
            move |_, _, _| positions.clone(),
            move |bounds, positions, window, _| {
                let to_point = |position: [f32; 2]| {
                    point(
                        bounds.origin.x
                            + bounds.size.width * (position[0] / resolution.width() as f32 + 0.5),
                        bounds.origin.y
                            + bounds.size.height * (position[1] / resolution.height() as f32 + 0.5),
                    )
                };
                let mut builder = PathBuilder::stroke(px(1.5));
                let mut positions = positions.iter().copied();
                if let Some(first) = positions.next() {
                    builder.move_to(to_point(first));
                    for position in positions {
                        builder.line_to(to_point(position));
                    }
                    if let Ok(path) = builder.build() {
                        window.paint_path(path, color.opacity(0.75));
                    }
                }
            },
        )
        .absolute()
        .inset_0()
        .size_full()
    }

    pub(in crate::ui::preview) fn editor_overlay(
        &self,
        overlay: PreviewEditorOverlay,
        resolution: zerium_core::timeline::ProjectResolution,
        color: Hsla,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        div()
            .absolute()
            .inset_0()
            .children(
                overlay
                    .motion_paths
                    .into_iter()
                    .map(|path| Self::path_overlay(path, resolution, color)),
            )
            .when(!overlay.spline_path.is_empty(), |this| {
                this.child(Self::path_overlay(overlay.spline_path, resolution, color))
            })
            .when_some(overlay.bounds, |this, bounds| {
                this.child(Self::size_overlay(bounds, resolution, color))
            })
            .children(overlay.points.into_iter().map(|point| {
                div()
                    .absolute()
                    .left(relative(point[0] / resolution.width() as f32 + 0.5))
                    .top(relative(point[1] / resolution.height() as f32 + 0.5))
                    .ml(px(-2.5))
                    .mt(px(-2.5))
                    .size(px(5.))
                    .rounded_full()
                    .bg(color)
            }))
            .child(self.scalar_controls(overlay.controls, resolution, color, cx))
    }
}
