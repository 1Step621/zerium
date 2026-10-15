use ::ui::{
    ContextModal as _, StyledExt as _,
    button::Button,
    form::{form_field, h_form},
    menu::{PopupMenuItem, popup_menu::PopupMenuExt as _},
    modal::{Modal, ModalButtonProps},
    notification::Notification,
    switch::Switch,
};
use gpui::{App, AppContext as _, Context, Render, Window, div, prelude::*, px};
use rust_i18n::t;
use ui::Sizable;

use crate::app::config::{AppConfig, ConfigStore};

struct ConfigSettings {
    draft: AppConfig,
}

pub(super) fn open(window: &mut Window, cx: &mut App) {
    if window.has_active_modal(cx) {
        return;
    }
    let draft = match ConfigStore::read(cx) {
        Ok(config) => config.clone(),
        Err(error) => {
            log::warn!("Could not open settings: {error}");
            window.push_notification(
                Notification::error(
                    t!("settings.load_failed", error = error.to_string()).to_string(),
                ),
                cx,
            );
            return;
        }
    };
    let settings = cx.new(|_| ConfigSettings { draft });
    window.open_modal(cx, move |modal: Modal, _, _| {
        let confirm_settings = settings.clone();
        modal
            .title(
                div()
                    .font_family(super::theme::FONT_FAMILY)
                    .font_normal()
                    .child(t!("settings.title").to_string()),
            )
            .width(px(520.))
            .confirm()
            .button_props(
                ModalButtonProps::default()
                    .ok_text(t!("menu.save").to_string())
                    .cancel_text(t!("common.cancel").to_string()),
            )
            .on_ok(move |_, window, cx| {
                let draft = confirm_settings.read(cx).draft.clone();
                match ConfigStore::update(cx, |config| {
                    let language_changed = config.language != draft.language;
                    *config = draft;
                    language_changed
                }) {
                    Ok(language_changed) => {
                        let message = if language_changed {
                            t!("settings.saved_restart")
                        } else {
                            t!("settings.saved")
                        };
                        window.push_notification(Notification::success(message.to_string()), cx);
                        true
                    }
                    Err(error) => {
                        log::warn!("Could not save settings: {error}");
                        window.push_notification(
                            Notification::error(
                                t!("settings.save_failed", error = error.to_string()).to_string(),
                            ),
                            cx,
                        );
                        false
                    }
                }
            })
            .child(settings.clone())
    });
}

impl Render for ConfigSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut languages = vec![
            (t!("settings.system_language").to_string(), None),
            (t!("app.japanese").to_string(), Some("ja-JP".to_owned())),
            (t!("english").to_string(), Some("en-US".to_owned())),
        ];
        let language = self
            .draft
            .language
            .as_deref()
            .map(str::trim)
            .filter(|language| !language.is_empty())
            .map(|language| language.replace('_', "-"));
        let selected_index = languages
            .iter()
            .position(|(_, value)| match (value.as_deref(), language.as_deref()) {
                (Some(value), Some(language)) => value.eq_ignore_ascii_case(language),
                (None, None) => true,
                _ => false,
            })
            .unwrap_or_else(|| {
                languages.push((
                    language.clone().unwrap_or_default(),
                    self.draft.language.clone(),
                ));
                languages.len() - 1
            });
        let settings = cx.entity();

        div().font_family(super::theme::FONT_FAMILY).child(
            h_form()
                .label_width(px(208.))
                .child(
                    form_field()
                        .label(t!("settings.language").to_string())
                        .items_center()
                        .child(
                            Button::new("settings-language")
                                .small()
                                .outline()
                                .label(languages[selected_index].0.clone())
                                .dropdown_caret(true)
                                .popup_menu(move |menu, _, _| {
                                    languages.iter().enumerate().fold(
                                        menu,
                                        |menu, (index, (label, language))| {
                                            let settings = settings.clone();
                                            let language = language.clone();
                                            menu.item(
                                                PopupMenuItem::new(label.clone())
                                                    .checked(index == selected_index)
                                                    .on_click(move |_, _, cx| {
                                                        settings.update(cx, |this, cx| {
                                                            this.draft.language = language.clone();
                                                            cx.notify();
                                                        });
                                                    }),
                                            )
                                        },
                                    )
                                }),
                        ),
                )
                .child(
                    form_field()
                        .label(t!("settings.show_support_reminder").to_string())
                        .items_center()
                        .child(
                            Switch::new("settings-support-reminder")
                                .small()
                                .checked(self.draft.show_support_reminder.unwrap_or(true))
                                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                    this.draft.show_support_reminder = Some(*checked);
                                    cx.notify();
                                })),
                        ),
                ),
        )
    }
}
