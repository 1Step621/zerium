//! Preview layout, playback controls and editor interaction surfaces.
use super::{Preview, editor_overlay::PreviewEditorDrag};
use crate::ui::{time_grid, transport::ScrubSource};
use ::ui::{ActiveTheme as _, slider::Slider};
use gpui::{
    Context, DragMoveEvent, Hsla, MouseButton, MouseDownEvent, MouseUpEvent, Render, SharedString,
    Window, div, prelude::*, px, relative, wgpu_surface,
};

impl Preview {
    const METER_MIN_DB: f32 = -60.;
    const BAR_THICKNESS: f32 = 6.;

    fn render_controls(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let (current, end, frame_rate) = {
            let editor = self.editor.read(cx);
            let frame_rate = editor.frame_rate();
            let end = editor.end_frame_exclusive();
            let current = editor
                .playback_time_seconds()
                .map(|seconds| frame_rate.seconds_to_frame(seconds))
                .unwrap_or_else(|| editor.playhead());
            (current, end, frame_rate)
        };
        let ratio = if end.get() == 0 {
            0.
        } else {
            (current.get() as f32 / end.get() as f32).clamp(0., 1.)
        };
        if (self.seekbar.read(cx).value().start() - ratio).abs() > f32::EPSILON {
            self.seekbar
                .update(cx, |seekbar, cx| seekbar.set_value(ratio, window, cx));
        }
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

    fn render_audio_meter(level: f32, background: Hsla, primary: Hsla) -> impl IntoElement {
        let level = if level > 0. {
            ((20. * level.log10() - Self::METER_MIN_DB) / -Self::METER_MIN_DB).clamp(0., 1.)
        } else {
            0.
        };
        div()
            .w(px(Self::BAR_THICKNESS))
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
}

impl Render for Preview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.transport.read(cx).is_playing() {
            self.transport
                .update(cx, |transport, cx| transport.advance(cx));
            window.request_animation_frame();
        }
        self.render_latest_frame(cx);

        let colors = cx.theme().colors;
        let surface = self.surface.clone();
        let (frame, resolution) = {
            let editor = self.editor.read(cx);
            (editor.playhead(), editor.resolution())
        };
        let aspect_ratio = resolution.aspect_ratio();
        let levels = if self.transport.read(cx).is_playing() {
            self.transport.read(cx).audio_levels(cx)
        } else {
            let editor = self.editor.read(cx);
            self.audio_level_sampler.levels_at(
                editor.visible_items(),
                frame,
                editor.frame_rate(),
                editor.media_cache().clone(),
            )
        };
        let error = self
            .render_runtime
            .read(cx)
            .error()
            .map(SharedString::from)
            .or_else(|| self.error.clone())
            .or_else(|| self.playback_error.clone());
        let overlay = self.selected_editor_overlay(frame, cx);
        let composition_units_per_pixel = surface.as_ref().map_or(0., |surface| {
            let logical_width = surface.size().0 as f32 / window.scale_factor();
            resolution.width() as f32 / logical_width.max(1.)
        });

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
                    .child(Self::render_audio_meter(
                        levels[0],
                        colors.slider_bar,
                        colors.primary,
                    ))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .overflow_hidden()
                            .when_some(surface, |this, surface| {
                                this.child(
                                    div()
                                        .relative()
                                        .w_full()
                                        .max_h_full()
                                        .aspect_ratio(aspect_ratio)
                                        .on_drag_move(cx.listener(
                                            |this,
                                             event: &DragMoveEvent<PreviewEditorDrag>,
                                             window,
                                             cx| {
                                                let drag = event.drag(cx).clone();
                                                this.move_editor_control_from_pointer(
                                                    &drag,
                                                    [
                                                        f32::from(event.event.position.x),
                                                        f32::from(event.event.position.y),
                                                    ],
                                                    window,
                                                    cx,
                                                );
                                            },
                                        ))
                                        .capture_any_mouse_up(cx.listener(
                                            |this, event: &MouseUpEvent, _, cx| {
                                                if event.button == MouseButton::Left {
                                                    let changed = this.editor_drag.take().is_some();
                                                    if changed {
                                                        this.editor.update(cx, |editor, _| {
                                                            editor.finish_history_group()
                                                        });
                                                        cx.notify();
                                                    }
                                                }
                                            },
                                        ))
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
                                                composition_units_per_pixel,
                                                colors.primary,
                                                cx,
                                            ))
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
                            }),
                    )
                    .child(Self::render_audio_meter(
                        levels[1],
                        colors.slider_bar,
                        colors.primary,
                    )),
            )
            .child(self.render_controls(window, cx))
    }
}
