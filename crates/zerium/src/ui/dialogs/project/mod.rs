mod export;

use self::export::ExportSettingsForm;

use rust_i18n::t;

use crate::{
    app::transport::TransportController,
    ui::{
        components::{
            form::{form_body, form_dialog, form_row, form_section},
            input::number::{
                settings::SettingsNumberInput,
                value::NumericInput::{F32, U32},
            },
        },
        workspace::notifications::UiNotifications,
    },
};
use ::ui::{
    ContextModal as _, StyledExt as _,
    modal::{Modal, ModalButtonProps},
};
use gpui::{App, Entity, Window, div, prelude::*};
use zerium_core::timeline::{BeatGuide, FrameRate, ProjectResolution, TimelineEditor};

pub(crate) fn open_settings(
    editor: &Entity<TimelineEditor>,
    transport: &Entity<TransportController>,
    notifications: &Entity<UiNotifications>,
    window: &mut Window,
    cx: &mut App,
) {
    let resolution = editor.read(cx).resolution();
    let frame_rate = editor.read(cx).frame_rate();
    let controls = [
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
    .map(|(value, number, step, suffix)| {
        cx.new(|cx| SettingsNumberInput::new(value, number, step, suffix, window, cx))
    });
    let export_settings = editor.read(cx).export_settings().clone();
    let export_form = cx.new(|cx| ExportSettingsForm::new(export_settings, window, cx));
    let editor = editor.clone();
    let transport = transport.clone();
    let notifications = notifications.clone();
    window.open_modal(cx, move |modal: Modal, window, _| {
        let [width, height, numerator, denominator, bpm, beat_offset] = controls.clone();
        let confirm_controls = controls.clone();
        let confirm_export = export_form.clone();
        let editor = editor.clone();
        let transport = transport.clone();
        let notifications = notifications.clone();
        form_dialog(modal, t!("project.settings").to_string(), window)
            .width(gpui::px(560.))
            .button_props(
                ModalButtonProps::default()
                    .ok_text(t!("project.apply").to_string())
                    .cancel_text(t!("common.cancel").to_string()),
            )
            .on_ok(move |_, _, cx| {
                let result = values(&confirm_controls, cx).and_then(
                    |(resolution, frame_rate, beat_guide)| {
                        let export_settings = confirm_export.read(cx).settings(cx)?;
                        editor.update(cx, |editor, cx| {
                            let result = editor.update_project_settings(
                                resolution,
                                frame_rate,
                                beat_guide,
                                export_settings,
                            );
                            if let Ok(true) = result {
                                cx.notify();
                            }
                            result.map_err(|error| error.to_string())
                        })
                    },
                );
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
                form_body()
                    .child(
                        form_section(t!("project.video").to_string())
                            .child(form_row(t!("project.width").to_string(), width))
                            .child(form_row(t!("project.height").to_string(), height))
                            .child(form_row(
                                t!("project.frame_rate").to_string(),
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(div().flex_1().child(numerator))
                                    .child(div().flex_none().child("/"))
                                    .child(div().flex_1().child(denominator)),
                            )),
                    )
                    .child(
                        form_section(t!("project.timeline").to_string())
                            .child(form_row(t!("project.bpm").to_string(), bpm))
                            .child(form_row(t!("project.beat_offset").to_string(), beat_offset)),
                    )
                    .child(form_section(t!("export.title").to_string()).child(export_form.clone())),
            )
    });
}

fn values(
    controls: &[Entity<SettingsNumberInput>; 6],
    cx: &App,
) -> Result<(ProjectResolution, FrameRate, BeatGuide), String> {
    let [width, height, numerator, denominator, bpm, beat_offset] = controls;
    let resolution = width
        .read(cx)
        .value(cx)
        .parse::<u32>()
        .ok()
        .zip(height.read(cx).value(cx).parse::<u32>().ok())
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
        .value(cx)
        .trim()
        .parse::<u32>()
        .map_err(|_| t!("project.invalid_integer_frame_rate").to_string())?;
    let denominator = denominator
        .read(cx)
        .value(cx)
        .trim()
        .parse::<u32>()
        .map_err(|_| t!("project.invalid_integer_frame_rate").to_string())?;
    let frame_rate = FrameRate::new(numerator, denominator)
        .ok_or_else(|| t!("project.invalid_frame_rate").to_string())?;
    let offset_seconds = beat_offset
        .read(cx)
        .value(cx)
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| t!("project.invalid_beat_offset").to_string())?;
    let beat_guide = bpm
        .read(cx)
        .value(cx)
        .trim()
        .parse::<f32>()
        .ok()
        .and_then(|bpm| BeatGuide::new(bpm, offset_seconds))
        .ok_or_else(|| t!("project.invalid_bpm").to_string())?;
    Ok((resolution, frame_rate, beat_guide))
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
                    .font_family(crate::ui::theme::FONT_FAMILY)
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
