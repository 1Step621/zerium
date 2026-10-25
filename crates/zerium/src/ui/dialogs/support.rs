use std::time::{SystemTime, UNIX_EPOCH};

use ::ui::{
    ActiveTheme as _, ContextModal as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    modal::{Modal, ModalButtonProps},
    notification::Notification,
};
use gpui::{App, Window, div, prelude::*, px};
use rust_i18n::t;

use crate::app::storage::{config::ConfigStore, state::StateStore};

pub(in crate::ui) const KO_FI_URL: &str = "https://ko-fi.com/onestep621";
const FIRST_REMINDER_LAUNCHES: u64 = 5;
const REPEAT_REMINDER_LAUNCHES: u64 = 10;
const REMINDER_INTERVAL_SECS: u64 = 14 * 24 * 60 * 60;

pub(crate) fn show_startup_reminder(window: &mut Window, cx: &mut App) {
    match ConfigStore::read(cx) {
        Ok(config) if config.show_support_reminder == Some(false) => return,
        Ok(_) => {}
        Err(error) => {
            log::warn!("Could not read support reminder preference: {error}");
            return;
        }
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let can_show = !window.has_active_modal(cx);
    let should_show = StateStore::update(cx, |state| {
        let reminder = &mut state.support_reminder;
        reminder.launches_since_reminder = reminder.launches_since_reminder.saturating_add(1);
        let due = match reminder.last_shown_at {
            None => reminder.launches_since_reminder >= FIRST_REMINDER_LAUNCHES,
            Some(last_shown) => {
                reminder.launches_since_reminder >= REPEAT_REMINDER_LAUNCHES
                    && now.saturating_sub(last_shown) >= REMINDER_INTERVAL_SECS
            }
        };
        if !can_show || !due {
            return false;
        }
        reminder.launches_since_reminder = 0;
        reminder.last_shown_at = Some(now);
        true
    });
    match should_show {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            log::warn!("Could not update support reminder: {error}");
            return;
        }
    }
    window.open_modal(cx, |modal: Modal, _, cx| {
        modal
            .title(
                div()
                    .font_family(crate::ui::theme::FONT_FAMILY)
                    .font_normal()
                    .child(t!("support.title").to_string()),
            )
            .width(px(520.))
            .confirm()
            .button_props(
                ModalButtonProps::default()
                    .ok_text(t!("support.open_ko_fi").to_string())
                    .cancel_text(t!("support.later").to_string()),
            )
            .on_ok(|_, _, cx| {
                cx.open_url(KO_FI_URL);
                true
            })
            .footer(|ok, cancel, window, cx| {
                vec![
                    div()
                        .w_full()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .child(
                            Button::new("support-never")
                                .text()
                                .h_8()
                                .label(t!("support.never").to_string())
                                .on_click(|_, window, cx| {
                                    match ConfigStore::update(cx, |config| {
                                        config.show_support_reminder = Some(false);
                                    }) {
                                        Ok(()) => window.close_modal(cx),
                                        Err(error) => {
                                            log::warn!(
                                                "Could not disable support reminder: {error}"
                                            );
                                            window.push_notification(
                                                Notification::error(
                                                    t!("support.save_failed").to_string(),
                                                ),
                                                cx,
                                            );
                                        }
                                    }
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(cancel(window, cx))
                                .child(ok(window, cx)),
                        ),
                ]
            })
            .child(
                div()
                    .font_family(crate::ui::theme::FONT_FAMILY)
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(t!("support.message").to_string())
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("support.optional").to_string()),
                    ),
            )
    });
}
