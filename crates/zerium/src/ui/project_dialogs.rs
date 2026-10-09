use rust_i18n::t;

use ::ui::{
    ContextModal as _, Sizable as _, StyledExt as _,
    input::{InputState, NumberInput, NumberInputEvent, StepAction},
    modal::{Modal, ModalButtonProps},
};
use gpui::{
    App, Context, DragMoveEvent, Entity, MouseButton, Render, Subscription, Window, div, prelude::*,
};
use zerium_core::property::ScalarPropertyType::{F32, U32};
use zerium_core::timeline::{BeatGuide, FrameRate, ProjectResolution, TimelineEditor};

use super::{
    input::set_input_text,
    number_input::{NumberValueDrag, number_input_drag},
    numeric_property::{NumericDrag, NumericInput},
    session::UiNotifications,
    transport::TransportController,
};

/// Edits a local input value without applying it to a project or property.
/// Dialogs can keep their usual Apply/Cancel behavior while sharing numeric gestures.
struct SettingsNumberInput {
    input: Entity<InputState>,
    number: NumericInput,
    step: f64,
    suffix: String,
    drag: Option<NumericDrag>,
    _subscription: Subscription,
}

impl SettingsNumberInput {
    fn new(
        input: Entity<InputState>,
        number: NumericInput,
        step: f64,
        suffix: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, input, event: &NumberInputEvent, window, cx| {
                let Some(value) = this.number.parse_number(&input.read(cx).value()) else {
                    return;
                };
                let NumberInputEvent::Step { action, fine } = event;
                let step = this.number.step(this.step * if *fine { 0.1 } else { 1. });
                let delta = if *action == StepAction::Increment {
                    step
                } else {
                    -step
                };
                this.set_number(value + delta, window, cx);
            },
        );
        Self {
            input,
            number,
            step,
            suffix: suffix.to_owned(),
            drag: None,
            _subscription: subscription,
        }
    }

    fn set_number(&mut self, value: f64, window: &mut Window, cx: &mut Context<Self>) {
        let (min, max) = self.number.bounds();
        let Some(text) = self
            .number
            .value_from_number(value.clamp(min, max))
            .and_then(|value| value.numeric_text())
        else {
            return;
        };
        set_input_text(&self.input, text, window, cx);
    }

    fn move_number(
        &mut self,
        event: &DragMoveEvent<NumberValueDrag>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = event.drag(cx);
        if target.owner_id != cx.entity_id() {
            return;
        }
        let Some(drag) = &self.drag else {
            return;
        };
        let value = drag.value_at(
            f32::from(event.event.position.x),
            event.event.modifiers.shift,
        );
        cx.set_active_drag_cursor_style(gpui::CursorStyle::ResizeLeftRight, window);
        self.set_number(value, window, cx);
    }
}

impl Render for SettingsNumberInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("settings-number-input")
            .w_full()
            .min_w_0()
            .flex()
            .on_drag_move(cx.listener(Self::move_number))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.drag = None),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.drag = None),
            )
            .child(number_input_drag(
                &cx.entity(),
                &self.input,
                NumberInput::new(&self.input)
                    .small()
                    .w_full()
                    .when(!self.suffix.is_empty(), |input| {
                        input.suffix(div().child(self.suffix.clone()))
                    }),
                false,
                |this, event, cx| {
                    let step = this.number.step(this.step);
                    this.drag =
                        this.number
                            .parse_number(&this.input.read(cx).value())
                            .map(|value| NumericDrag {
                                start_x: f32::from(event.position.x),
                                start_value: value,
                                step,
                                sensitivity: step,
                            });
                },
            ))
    }
}

pub(crate) fn open_settings(
    editor: &Entity<TimelineEditor>,
    transport: &Entity<TransportController>,
    notifications: &Entity<UiNotifications>,
    window: &mut Window,
    cx: &mut App,
) {
    let resolution = editor.read(cx).resolution();
    let frame_rate = editor.read(cx).frame_rate();
    let fields = [
        (resolution.width().to_string(), U32, 1., ""),
        (resolution.height().to_string(), U32, 1., ""),
        (frame_rate.numerator().to_string(), U32, 1., ""),
        (frame_rate.denominator().to_string(), U32, 1., ""),
        (editor.read(cx).beat_guide().bpm().to_string(), F32, 1., ""),
        (
            editor.read(cx).beat_guide().offset_seconds().to_string(),
            F32,
            0.01,
            "s",
        ),
    ]
    .map(|(value, scalar_type, step, suffix)| {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(value));
        let number = NumericInput {
            scalar: scalar_type,
        };
        let control =
            cx.new(|cx| SettingsNumberInput::new(input.clone(), number, step, suffix, window, cx));
        (input, control)
    });
    let inputs = fields.each_ref().map(|(input, _)| input.clone());
    let controls = fields.each_ref().map(|(_, control)| control.clone());
    let editor = editor.clone();
    let transport = transport.clone();
    let notifications = notifications.clone();
    window.open_modal(cx, move |modal: Modal, _, _| {
        let [width, height, numerator, denominator, bpm, beat_offset] = controls.clone();
        let confirm_inputs = inputs.clone();
        let editor = editor.clone();
        let transport = transport.clone();
        let notifications = notifications.clone();
        modal
            .title(
                div()
                    .font_family(super::theme::FONT_FAMILY)
                    .font_normal()
                    .child(t!("project.settings").to_string()),
            )
            .width(gpui::px(440.))
            .confirm()
            .button_props(
                ModalButtonProps::default()
                    .ok_text(t!("project.apply").to_string())
                    .cancel_text(t!("common.cancel").to_string()),
            )
            .on_ok(move |_, _, cx| {
                let result =
                    values(&confirm_inputs, cx).and_then(|(resolution, frame_rate, beat_guide)| {
                        editor.update(cx, |editor, cx| {
                            let result =
                                editor.update_project_settings(resolution, frame_rate, beat_guide);
                            if matches!(result, Ok(true)) {
                                cx.notify();
                            }
                            result.map_err(|error| error.to_string())
                        })
                    });
                match result {
                    Ok(_) => {
                        transport.update(cx, |transport, cx| transport.stop(cx));
                        true
                    }
                    Err(error) => {
                        notifications.update(cx, |notifications, cx| notifications.push(error, cx));
                        false
                    }
                }
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(row(t!("project.width").to_string(), width))
                    .child(row(t!("project.height").to_string(), height))
                    .child(row(
                        t!("project.frame_rate").to_string(),
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().flex_1().child(numerator))
                            .child(div().flex_none().child("/"))
                            .child(div().flex_1().child(denominator)),
                    ))
                    .child(row(t!("project.bpm").to_string(), bpm))
                    .child(row(t!("project.beat_offset").to_string(), beat_offset)),
            )
    });
}

fn values(
    inputs: &[Entity<InputState>; 6],
    cx: &App,
) -> Result<(ProjectResolution, FrameRate, BeatGuide), String> {
    let [width, height, numerator, denominator, bpm, beat_offset] = inputs;
    let resolution = width
        .read(cx)
        .value()
        .parse::<u32>()
        .ok()
        .zip(height.read(cx).value().parse::<u32>().ok())
        .and_then(|(width, height)| ProjectResolution::new(width, height))
        .ok_or_else(|| {
            t!(
                "project.invalid_resolution",
                max = ProjectResolution::MAX_DIMENSION
            )
            .to_string()
        })?;
    let numerator = numerator
        .read(cx)
        .value()
        .trim()
        .parse::<u32>()
        .map_err(|_| t!("project.invalid_integer_frame_rate").to_string())?;
    let denominator = denominator
        .read(cx)
        .value()
        .trim()
        .parse::<u32>()
        .map_err(|_| t!("project.invalid_integer_frame_rate").to_string())?;
    let frame_rate = FrameRate::new(numerator, denominator)
        .ok_or_else(|| t!("project.invalid_frame_rate").to_string())?;
    let offset_seconds = beat_offset
        .read(cx)
        .value()
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| t!("project.invalid_beat_offset").to_string())?;
    let beat_guide = bpm
        .read(cx)
        .value()
        .trim()
        .parse::<f32>()
        .ok()
        .and_then(|bpm| BeatGuide::new(bpm, offset_seconds))
        .ok_or_else(|| t!("project.invalid_bpm").to_string())?;
    Ok((resolution, frame_rate, beat_guide))
}

fn row(label: String, input: impl IntoElement) -> gpui::Div {
    div()
        .w_full()
        .flex()
        .items_center()
        .gap_3()
        .child(div().w(gpui::px(120.)).flex_none().child(label))
        .child(div().min_w_0().flex_1().child(input))
}

pub(crate) fn confirm_discard(
    window: &mut Window,
    cx: &mut App,
    on_confirm: impl Fn(&mut Window, &mut App) + 'static,
) {
    discard_dialog(
        t!("project.discard").to_string(),
        t!("project.discard_prompt").to_string(),
        on_confirm,
        window,
        cx,
    );
}

pub(crate) fn confirm_exit(window: &mut Window, cx: &mut App) {
    discard_dialog(
        t!("project.discard_exit").to_string(),
        t!("project.discard_exit_prompt").to_string(),
        |window, cx| window.defer(cx, |window, _| window.remove_window()),
        window,
        cx,
    );
}

fn discard_dialog(
    button: String,
    prompt: String,
    on_confirm: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let on_confirm = std::rc::Rc::new(on_confirm);
    window.open_modal(cx, move |modal: Modal, _, _| {
        let on_confirm = on_confirm.clone();
        modal
            .title(
                div()
                    .font_family(super::theme::FONT_FAMILY)
                    .font_normal()
                    .child(t!("project.unsaved_changes").to_string()),
            )
            .confirm()
            .button_props(
                ModalButtonProps::default()
                    .ok_text(button.clone())
                    .cancel_text(t!("common.cancel").to_string()),
            )
            .on_ok(move |_, window, cx| {
                on_confirm(window, cx);
                true
            })
            .child(prompt.clone())
    });
}
