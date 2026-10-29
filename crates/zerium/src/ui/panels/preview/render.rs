//! Preview layout, playback controls and editor interaction surfaces.
use super::Preview;
use crate::{
    app::{actions::HidePreviewOverlay, transport::ScrubSource},
    ui::{
        components::time_grid,
        workspace::operation_hints::{Hint, OperationHintExt as _},
    },
};
use ::ui::{
    ActiveTheme as _, StyledExt as _, menu::context_menu::ContextMenuExt as _, slider::Slider,
};
use gpui::{
    Context, Hsla, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Render, SharedString,
    Window, div, prelude::*, px, relative, wgpu_surface,
};

impl Preview {
    const METER_MIN_DB: f32 = -60.;
    const BAR_THICKNESS: f32 = 6.;

    fn render_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let (current, end, frame_rate) = {
            let editor = self.editor.read(cx);
            (
                editor.playhead(),
                editor.end_frame_exclusive(),
                editor.frame_rate(),
            )
        };
        let ratio = self.seekbar_progress(cx);
        let current_label = format!(
            "{}  {}f",
            time_grid::format_timestamp(frame_rate.frame_to_seconds(current)),
            current.get()
        );
        let total_label = format!(
            "{}  {}f",
            time_grid::format_timestamp(frame_rate.frame_to_seconds(end)),
            end.get()
        );
        let begin_scrub = cx.listener(|this, event: &MouseDownEvent, _, cx| {
            if event.button == MouseButton::Left {
                this.transport.update(cx, |transport, cx| {
                    transport.begin_scrub(ScrubSource::Preview, cx)
                });
            }
        });
        let end_scrub = cx.listener(|this, event: &MouseUpEvent, _, cx| {
            if event.button == MouseButton::Left {
                this.transport.update(cx, |transport, cx| {
                    transport.end_scrub(ScrubSource::Preview, cx)
                });
            }
        });
        div()
            .w_full()
            .h(px(50.))
            .flex_none()
            .flex()
            .items_center()
            .bg(colors.background)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .h_full()
                    .justify_center()
                    .gap_0()
                    .border_t_1()
                    .border_color(colors.border.opacity(0.55))
                    .px_3()
                    .py_1()
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .justify_between()
                            .text_sm()
                            .text_color(colors.muted_foreground)
                            .child(current_label)
                            .child(total_label),
                    )
                    .child(
                        div()
                            .id("preview-seekbar")
                            .w_full()
                            .capture_any_mouse_down(begin_scrub)
                            .capture_any_mouse_up(end_scrub)
                            .child(
                                div()
                                    .relative()
                                    .w_full()
                                    .h(px(20.))
                                    .child(
                                        div()
                                            .absolute()
                                            .left_0()
                                            .right_0()
                                            .top(px(7.))
                                            .h(px(Self::BAR_THICKNESS))
                                            .bg(colors.slider_bar.opacity(0.75)),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .left_0()
                                            .top(px(7.))
                                            .h(px(Self::BAR_THICKNESS))
                                            .w(relative(ratio))
                                            .bg(colors.primary),
                                    )
                                    .child(
                                        Slider::new(&self.seekbar)
                                            .horizontal()
                                            .bg(colors.background.opacity(0.)),
                                    ),
                            ),
                    ),
            )
    }
}

impl Render for Preview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.transport.read(cx).is_fast_forwarding()
            && !Self::playback_shortcuts_enabled(window, cx)
        {
            self.set_fast_forward(false, window, cx);
        }
        if self.transport.read(cx).is_playing() {
            let transport = self.transport.clone();
            window.on_next_frame(move |_, cx| {
                transport.update(cx, |transport, cx| transport.advance(cx));
            });
            window.request_animation_frame();
        }
        self.recover_gpu(window, cx);
        // Resizing replaces the surface textures after layout. Paint their new
        // extent even when playback is stopped and no editor change follows.
        if window.is_window_resizing()
            || self
                .surface
                .as_ref()
                .is_some_and(|surface| !surface.is_device_lost() && surface.is_resize_pending())
        {
            let preview = cx.entity();
            window.on_next_frame(move |_, cx| preview.update(cx, |_, cx| cx.notify()));
            window.request_animation_frame();
        }
        self.validate_color_pick(window, cx);
        self.render_latest_frame(cx);

        let colors = cx.theme().colors;
        let surface = self
            .surface
            .clone()
            .filter(|surface| !surface.is_device_lost());
        let preview = cx.entity();
        let (frame, resolution) = {
            let editor = self.editor.read(cx);
            (editor.playhead(), editor.resolution())
        };
        let aspect_ratio = resolution.aspect_ratio();
        let levels = if self.transport.read(cx).is_playing() {
            self.transport.read(cx).audio_levels(cx)
        } else {
            let editor = self.editor.read(cx);
            self.audio_level_sampler.levels_at(editor)
        };
        let error = self
            .render_runtime
            .read(cx)
            .error()
            .map(SharedString::from)
            .or_else(|| self.error.clone())
            .or_else(|| self.frame_error.clone());
        let overlay = if self.editor_overlay_hidden || self.color_pick.is_some() {
            None
        } else {
            self.selected_editor_overlay(frame, cx)
        };
        if overlay.is_none()
            || self
                .editor_drag
                .as_ref()
                .is_some_and(|drag| !drag.edit.is_current(self.editor.read(cx)))
        {
            self.end_editor_drag(cx);
        }

        div()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(colors.background)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .overflow_hidden()
                    .child(render_audio_meter(
                        levels[0],
                        colors.slider_bar,
                        colors.primary,
                    ))
                    .child(
                        div()
                            .id("preview-image")
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .overflow_hidden()
                            .context_menu(move |menu, _, cx| {
                                preview.update(cx, |preview, cx| preview.image_menu(menu, cx))
                            })
                            .when_some(surface, |this, surface| {
                                this.child(
                                    div()
                                        .relative()
                                        .w_full()
                                        .max_h_full()
                                        .aspect_ratio(aspect_ratio)
                                        .child(
                                            wgpu_surface(surface)
                                                .absolute()
                                                .inset_0()
                                                .size_full()
                                                .defer_resize_until_mouse_up(true),
                                        )
                                        .when_some(overlay, |this, overlay| {
                                            this.child(self.editor_overlay(
                                                overlay,
                                                resolution,
                                                colors.primary,
                                                cx,
                                            ))
                                        })
                                        .when(self.color_pick.is_some(), |this| {
                                            this.child(self.color_pick_overlay(cx))
                                        }),
                                )
                            })
                            .when_some(error, |this, error| {
                                this.child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_sm()
                                        .text_color(colors.danger)
                                        .child(error),
                                )
                            })
                            .when(self.transport.read(cx).is_fast_forwarding(), |this| {
                                this.child(
                                    div()
                                        .absolute()
                                        .right_2()
                                        .bottom_2()
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .bg(gpui::black().opacity(0.65))
                                        .text_color(gpui::white())
                                        .text_sm()
                                        .font_semibold()
                                        .child("x2"),
                                )
                            }),
                    )
                    .child(render_audio_meter(
                        levels[1],
                        colors.slider_bar,
                        colors.primary,
                    )),
            )
            .child(self.render_controls(cx))
            .operation_hints([
                Hint::action(HidePreviewOverlay, "hints.hide_preview_overlay"),
                Hint::gesture("hints.key_l", "hints.fast_forward", Modifiers::default()),
            ])
    }
}

fn render_audio_meter(level: f32, background: Hsla, primary: Hsla) -> impl IntoElement {
    let level = if level > 0. {
        ((20. * level.log10() - Preview::METER_MIN_DB) / -Preview::METER_MIN_DB).clamp(0., 1.)
    } else {
        0.
    };
    div()
        .w(px(Preview::BAR_THICKNESS))
        .h_full()
        .relative()
        .bg(background.opacity(0.75))
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(relative(level))
                .bg(primary),
        )
}
