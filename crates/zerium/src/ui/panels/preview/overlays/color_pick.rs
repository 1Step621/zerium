//! Routes color inputs to a one-click sample of the preview's composited image.
use ::ui::{
    IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
};
use gpui::{
    AppContext as _, Context, CursorStyle, DispatchPhase, Entity, FocusHandle, Global,
    HitboxBehavior, KeyDownEvent, MouseButton, MouseDownEvent, Task, WeakEntity, Window, WindowId,
    canvas, div, prelude::*,
};
use rust_i18n::t;

use crate::ui::{components::input::color_to_hsla, panels::preview::Preview};

struct SamplingTarget {
    preview: WeakEntity<Preview>,
    window: WindowId,
}

impl Global for SamplingTarget {}

pub(in crate::ui::panels::preview) struct ColorPick {
    picker: WeakEntity<ColorPickerState>,
    revision: u64,
    focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    task: Option<Task<()>>,
}

pub(in crate::ui::panels::preview) fn register(window: &Window, cx: &mut Context<Preview>) {
    cx.set_global(SamplingTarget {
        preview: cx.entity().downgrade(),
        window: window.window_handle().window_id(),
    });
}

pub(in crate::ui) fn color_input(picker: &Entity<ColorPickerState>) -> gpui::Div {
    let target = picker.downgrade();
    div()
        .min_w_0()
        .w_full()
        .flex()
        .items_center()
        .gap_1()
        .child(
            div()
                .min_w_0()
                .flex_1()
                .child(ColorPicker::new(picker).small().w_full()),
        )
        .child(
            Button::new(("pick-preview-color", picker.entity_id()))
                .icon(IconName::ColorPicker)
                .small()
                .compact()
                .ghost()
                .tooltip(t!("color.pick_from_preview").to_string())
                .on_click(move |_, window, cx| {
                    let preview = cx
                        .try_global::<SamplingTarget>()
                        .filter(|target| target.window == window.window_handle().window_id())
                        .map(|target| target.preview.clone());
                    if let Some(preview) = preview {
                        let _ = preview.update(cx, |preview, cx| {
                            preview.begin_color_pick(target.clone(), window, cx);
                        });
                    }
                }),
        )
}

impl Preview {
    fn begin_color_pick(
        &mut self,
        picker: WeakEntity<ColorPickerState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_color_pick(window, cx);
        if self.rendered_frame.is_none() || self.error.is_some() {
            self.notifications.update(cx, |notifications, cx| {
                notifications.push(t!("color.preview_unavailable").to_string(), cx);
            });
            return;
        }
        self.transport.update(cx, |transport, cx| {
            transport.stop_in_place(cx);
        });
        self.end_editor_drag(cx);
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        let focus = cx.focus_handle();
        let previous_focus = window.focused(cx);
        focus.focus(window, cx);
        self.color_pick = Some(ColorPick {
            picker,
            revision: self.editor.read(cx).render_revision(),
            focus,
            previous_focus,
            task: None,
        });
        cx.notify();
    }

    pub(in crate::ui::panels::preview) fn cancel_color_pick(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(pick) = self.color_pick.take() {
            if pick.focus.is_focused(window)
                && let Some(focus) = pick.previous_focus
            {
                focus.focus(window, cx);
            }
            cx.notify();
        }
    }

    pub(in crate::ui::panels::preview) fn validate_color_pick(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.color_pick.as_ref().is_some_and(|pick| {
            pick.revision != self.editor.read(cx).render_revision()
                || pick.picker.upgrade().is_none()
                || self.transport.read(cx).is_playing()
        }) {
            self.cancel_color_pick(window, cx);
        }
    }

    fn sample_preview_color(
        &mut self,
        position: [f32; 2],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pick) = self.color_pick.as_ref().filter(|pick| pick.task.is_none()) else {
            return;
        };
        // Sample only a completed frame matching the editor state captured at start.
        if !self
            .rendered_frame
            .is_some_and(|frame| frame.editor_revision == pick.revision)
        {
            return;
        }
        let Some(renderer) = self.render_runtime.read(cx).preview_session() else {
            return;
        };
        let Some(surface) = self.surface.clone() else {
            return;
        };
        let Some(_submission_guard) = surface.try_submit_guard() else {
            return;
        };
        if surface.is_device_lost() {
            return;
        }
        let readback = match renderer.read_color(position) {
            Ok(readback) => readback,
            Err(error) => {
                self.cancel_color_pick(window, cx);
                self.notifications.update(cx, |notifications, cx| {
                    notifications.push(t!("color.sample_failed", error = error).to_string(), cx);
                });
                return;
            }
        };
        let task = cx.spawn_in(window, async move |preview, cx| {
            let result = cx.background_spawn(async move { readback.finish() }).await;
            let _ = preview.update_in(cx, |preview, window, cx| {
                preview.validate_color_pick(window, cx);
                let Some(pick) = preview.color_pick.as_ref() else {
                    return;
                };
                let picker = pick.picker.upgrade();
                preview.cancel_color_pick(window, cx);
                match (result, picker) {
                    (Ok(color), Some(picker)) => {
                        picker.update(cx, |picker, cx| {
                            let color = color_to_hsla(color);
                            picker.set_value(color, window, cx);
                            cx.emit(ColorPickerEvent::Change(Some(color)));
                        });
                        let editor = preview.editor.clone();
                        cx.defer(move |cx| {
                            editor.update(cx, |editor, _| editor.finish_history_group());
                        });
                    }
                    (Err(error), _) => {
                        preview.notifications.update(cx, |notifications, cx| {
                            notifications
                                .push(t!("color.sample_failed", error = error).to_string(), cx);
                        });
                    }
                    _ => {}
                }
            });
        });
        self.color_pick.as_mut().expect("active color pick").task = Some(task);
    }

    pub(in crate::ui::panels::preview) fn color_pick_overlay(
        &self,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let preview = cx.entity();
        let focus = self
            .color_pick
            .as_ref()
            .expect("active color pick")
            .focus
            .clone();
        div()
            .id("preview-color-pick")
            .absolute()
            .inset_0()
            .size_full()
            .track_focus(&focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.cancel_color_pick(window, cx);
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, window, cx| {
                    this.cancel_color_pick(window, cx);
                    cx.stop_propagation();
                }),
            )
            .on_mouse_down_out(
                cx.listener(|this, _, window, cx| this.cancel_color_pick(window, cx)),
            )
            .child(
                canvas(
                    |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                    move |bounds, hitbox, window, _| {
                        window.set_cursor_style(CursorStyle::Crosshair, &hitbox);
                        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                            if phase == DispatchPhase::Bubble
                                && event.button == MouseButton::Left
                                && bounds.contains(&event.position)
                            {
                                let x = f32::from(event.position.x - bounds.origin.x)
                                    / f32::from(bounds.size.width);
                                let y = f32::from(event.position.y - bounds.origin.y)
                                    / f32::from(bounds.size.height);
                                preview.update(cx, |preview, cx| {
                                    preview.sample_preview_color([x, y], window, cx)
                                });
                                cx.stop_propagation();
                            }
                        });
                    },
                )
                .size_full(),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .px_2()
                    .py_1()
                    .text_sm()
                    .text_color(gpui::white())
                    .bg(gpui::black().opacity(0.7))
                    .child(t!("color.pick_instruction").to_string()),
            )
    }
}
