use rust_i18n::t;

use ::ui::{
    ContextModal as _, Sizable as _, StyledExt as _,
    input::{InputState, NumberInput},
    modal::{Modal, ModalButtonProps},
};
use gpui::{App, Entity, Window, div, prelude::*};
use zerium_core::timeline::{FrameRate, ProjectResolution, TimelineEditor};

use super::{session::UiNotifications, transport::TransportController};

pub(crate) fn open_settings(
    editor: &Entity<TimelineEditor>,
    transport: &Entity<TransportController>,
    notifications: &Entity<UiNotifications>,
    window: &mut Window,
    cx: &mut App,
) {
    let resolution = editor.read(cx).resolution();
    let frame_rate = editor.read(cx).frame_rate();
    let inputs = [
        resolution.width(),
        resolution.height(),
        frame_rate.numerator(),
        frame_rate.denominator(),
    ]
    .map(|value| cx.new(|cx| InputState::new(window, cx).default_value(value.to_string())));
    let editor = editor.clone();
    let transport = transport.clone();
    let notifications = notifications.clone();
    window.open_modal(cx, move |modal: Modal, _, _| {
        let [width, height, numerator, denominator] = &inputs;
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
                let result = values(&confirm_inputs, cx).and_then(|(resolution, frame_rate)| {
                    editor.update(cx, |editor, cx| {
                        let result = editor.update_project_settings(resolution, frame_rate);
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
                    .child(row(
                        t!("project.width").to_string(),
                        NumberInput::new(width).small().w_full(),
                    ))
                    .child(row(
                        t!("project.height").to_string(),
                        NumberInput::new(height).small().w_full(),
                    ))
                    .child(row(
                        t!("project.frame_rate").to_string(),
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .child(NumberInput::new(numerator).small().w_full()),
                            )
                            .child(div().flex_none().child("/"))
                            .child(
                                div()
                                    .flex_1()
                                    .child(NumberInput::new(denominator).small().w_full()),
                            ),
                    )),
            )
    });
}

fn values(
    inputs: &[Entity<InputState>; 4],
    cx: &App,
) -> Result<(ProjectResolution, FrameRate), String> {
    let [width, height, numerator, denominator] = inputs;
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
    Ok((resolution, frame_rate))
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
