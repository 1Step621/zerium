use super::*;
use crate::ui::operation_hints::{Hint, OperationHintExt as _};
use gpui::fill;

pub(super) const STRETCH_COLOR: Hsla = Hsla {
    h: 200. / 360.,
    s: 0.9,
    l: 0.65,
    a: 1.,
};

#[derive(Clone)]
pub(super) struct StretchPointDrag {
    timeline_id: EntityId,
    scene_id: SceneId,
    gesture: Rc<RefCell<Option<EditGesture>>>,
}

impl Render for StretchPointDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl Timeline {
    pub(super) fn stretch_indicator(background: Hsla) -> impl IntoElement {
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                window.paint_quad(fill(
                    Bounds::new(
                        point(bounds.origin.x + px(4.), bounds.origin.y),
                        size(px(6.), bounds.size.height),
                    ),
                    background,
                ));
                let mut lines = PathBuilder::stroke(px(1.5));
                for offset in [4., 10.] {
                    lines.move_to(point(bounds.origin.x + px(offset), bounds.origin.y));
                    lines.line_to(point(
                        bounds.origin.x + px(offset),
                        bounds.origin.y + bounds.size.height,
                    ));
                }
                if let Ok(path) = lines.build() {
                    window.paint_path(path, STRETCH_COLOR.opacity(0.4));
                }
            },
        )
        .absolute()
        .size_full()
    }

    pub(super) fn stretch_marker(
        &self,
        colors: ThemeColor,
        viewport_width: f32,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let editor = self.editor.read(cx);
        let scene_id = editor.active_scene_id()?;
        let scene = editor.scene(scene_id)?;
        let point = scene.stretch_point();
        let x = self
            .viewport
            .x_at_seconds(editor.frame_rate().frame_to_seconds(point));
        if x < -7. || x > viewport_width + 7. {
            return None;
        }
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
                .hover(|style| style.bg(STRETCH_COLOR.opacity(0.12)))
                .child(Self::stretch_indicator(colors.background))
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

    pub(super) fn move_stretch_point(
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
