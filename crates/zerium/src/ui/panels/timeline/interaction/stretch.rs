use rust_i18n::t;
use std::{cell::RefCell, rc::Rc};

use ::ui::ThemeColor;
use gpui::{
    Bounds, ContentMask, Context, CursorStyle, Empty, EntityId, MouseButton, MouseMoveEvent,
    PathBuilder, Pixels, Render, Window, canvas, div, fill, point, prelude::*, px, size,
};

use zerium_core::timeline::{EditSession, SceneId};

use crate::ui::{
    components::pane::PANE_HEADER_HEIGHT,
    panels::timeline::{LAYER_HEADER_WIDTH, Timeline},
    workspace::operation_hints::{Hint, OperationHintExt as _},
};

#[derive(Clone)]
pub(in crate::ui::panels::timeline) struct StretchPointDrag {
    timeline_id: EntityId,
    scene_id: SceneId,
    gesture: Rc<RefCell<Option<EditSession>>>,
}

impl Render for StretchPointDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl Timeline {
    fn stretch_position(&self, viewport_width: f32, cx: &Context<Self>) -> Option<(SceneId, f32)> {
        let editor = self.editor.read(cx);
        let scene_id = editor.active_scene_id();
        if scene_id == editor.main_scene_id() {
            return None;
        }
        let scene = editor.active_scene();
        let point = scene.stretch_point();
        let x = self
            .viewport
            .x_at_seconds(editor.frame_rate().frame_to_seconds(point));
        if x < -7. || x > viewport_width + 7. {
            return None;
        }
        Some((scene_id, x))
    }

    pub(in crate::ui::panels::timeline) fn stretch_overlay(
        &self,
        colors: ThemeColor,
        viewport_width: f32,
        cx: &Context<Self>,
    ) -> Option<impl IntoElement> {
        let (_, x) = self.stretch_position(viewport_width, cx)?;
        Some(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    window.with_content_mask(Some(ContentMask { bounds }), |window| {
                        paint_stretch_indicator(
                            Bounds::new(
                                bounds.origin + point(px(x - 7.), px(0.)),
                                size(px(14.), bounds.size.height),
                            ),
                            colors,
                            window,
                        );
                    });
                },
            )
            .absolute()
            .top(px(PANE_HEADER_HEIGHT))
            .left(px(LAYER_HEADER_WIDTH))
            .right_0()
            .bottom_0(),
        )
    }

    pub(in crate::ui::panels::timeline) fn stretch_marker(
        &self,
        colors: ThemeColor,
        viewport_width: f32,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let (scene_id, x) = self.stretch_position(viewport_width, cx)?;
        let drag = StretchPointDrag {
            timeline_id: cx.entity_id(),
            scene_id,
            gesture: Rc::new(RefCell::new(None)),
        };
        let start_editor = self.editor.clone();
        Some(
            div()
                .id("scene-stretch-point")
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(x - 7.))
                .w(px(14.))
                .cursor(CursorStyle::ResizeLeftRight)
                .hover(move |style| style.bg(colors.primary.opacity(0.12)))
                .child(stretch_indicator(colors))
                .tooltip(|window, cx| {
                    ::ui::tooltip::Tooltip::new(t!("timeline.stretch_point_tooltip").to_string())
                        .build(window, cx)
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.stop_playback(cx);
                        this.editor
                            .update(cx, |editor, _| editor.finish_history_group());
                        cx.stop_propagation();
                    }),
                )
                .on_drag(drag, move |drag, _, _, cx| {
                    *drag.gesture.borrow_mut() = Some(start_editor.read(cx).begin_edit_gesture());
                    cx.stop_propagation();
                    cx.new(|_| drag.clone())
                })
                .operation_hints([
                    Hint::gesture(
                        "hints.drag",
                        "hints.move_stretch_point",
                        gpui::Modifiers::default(),
                    ),
                    Hint::gesture("hints.drag", "hints.no_snap", gpui::Modifiers::alt()),
                ]),
        )
    }

    pub(in crate::ui::panels::timeline) fn move_stretch_point(
        &mut self,
        drag: &StretchPointDrag,
        event: &MouseMoveEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if drag.timeline_id != cx.entity_id() {
            return;
        }
        let mut frame = self.pointer_frame(f32::from(event.position.x), window, cx);
        if !event.modifiers.alt {
            frame = self.snap_frame(frame, &[], cx);
        }
        let mut gesture = drag.gesture.borrow_mut();
        let Some(gesture) = gesture.as_mut() else {
            return;
        };
        self.editor.update(cx, |editor, cx| {
            if editor.edit_gesture(gesture, |editor| {
                editor.set_scene_stretch_point(drag.scene_id, frame);
            }) {
                cx.notify();
            }
        });
    }
}

fn stretch_indicator(colors: ThemeColor) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| paint_stretch_indicator(bounds, colors, window),
    )
    .absolute()
    .size_full()
}

fn paint_stretch_indicator(bounds: Bounds<Pixels>, colors: ThemeColor, window: &mut Window) {
    window.paint_quad(fill(
        Bounds::new(
            point(bounds.origin.x + px(4.), bounds.origin.y),
            size(px(6.), bounds.size.height),
        ),
        colors.background,
    ));
    let mut lines = PathBuilder::stroke(px(1.5));
    for offset in [4., 10.] {
        lines.move_to(point(bounds.origin.x + px(offset), bounds.origin.y));
        lines.line_to(point(bounds.origin.x + px(offset), bounds.bottom_right().y));
    }
    if let Ok(path) = lines.build() {
        window.paint_path(path, colors.primary.opacity(0.4));
    }
}
