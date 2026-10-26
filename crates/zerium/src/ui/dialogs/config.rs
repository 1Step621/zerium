use ::ui::{
    ContextModal as _, Selectable as _, Sizable as _, ThemeMode,
    button::Button,
    menu::{PopupMenuItem, popup_menu::PopupMenuExt as _},
    modal::{Modal, ModalButtonProps},
    notification::Notification,
    switch::Switch,
};
use gpui::{App, AppContext as _, Context, Render, Window, div, prelude::*};
use rust_i18n::t;

use crate::{
    app::storage::config::{AccentColor, AppConfig, ConfigStore},
    ui::components::form::{form_dialog, form_fields, form_row},
};

struct ConfigSettings {
    draft: AppConfig,
}

pub(in crate::ui) fn open(window: &mut Window, cx: &mut App) {
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
    window.open_modal(cx, move |modal: Modal, window, _| {
        let confirm_settings = settings.clone();
        form_dialog(modal, t!("settings.title").to_string(), window)
            .button_props(
                ModalButtonProps::default()
                    .ok_text(t!("menu.save").to_string())
                    .cancel_text(t!("common.close").to_string()),
            )
            .on_ok(move |_, window, cx| {
                let draft = confirm_settings.read(cx).draft.clone();
                match ConfigStore::update(cx, |config| {
                    let language_changed = config.language != draft.language;
                    *config = draft;
                    language_changed
                }) {
                    Ok(language_changed) => {
                        crate::ui::theme::apply(cx);
                        let message = if language_changed {
                            t!("settings.saved_restart")
                        } else {
                            t!("settings.saved")
                        };
                        window.push_notification(Notification::success(message.to_string()), cx);
                    }
                    Err(error) => {
                        log::warn!("Could not save settings: {error}");
                        window.push_notification(
                            Notification::error(
                                t!("settings.save_failed", error = error.to_string()).to_string(),
                            ),
                            cx,
                        );
                    }
                }
                false
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
        let accent_settings = cx.entity();
        let theme_mode = self.draft.theme_mode.unwrap_or(ThemeMode::Dark);
        let accent = self.draft.accent_color.unwrap_or_default();
        let accents = [
            (
                AccentColor::Orange,
                t!("settings.accent_orange").to_string(),
            ),
            (AccentColor::Teal, t!("settings.accent_teal").to_string()),
            (AccentColor::Red, t!("settings.accent_red").to_string()),
            (AccentColor::Pink, t!("settings.accent_pink").to_string()),
            (
                AccentColor::Purple,
                t!("settings.accent_purple").to_string(),
            ),
            (
                AccentColor::Yellow,
                t!("settings.accent_yellow").to_string(),
            ),
        ];
        let selected_accent_index = accents
            .iter()
            .position(|(choice, _)| *choice == accent)
            .unwrap_or(0);

        form_fields()
            .child(form_row(
                t!("settings.theme_mode").to_string(),
                div().flex().gap_1().children(
                    [
                        (ThemeMode::Dark, t!("settings.dark")),
                        (ThemeMode::Light, t!("settings.light")),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(index, (mode, label))| {
                        Button::new(("settings-theme", index))
                            .small()
                            .outline()
                            .selected(mode == theme_mode)
                            .label(label.to_string())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.draft.theme_mode = Some(mode);
                                cx.notify();
                            }))
                    }),
                ),
            ))
            .child(form_row(
                t!("settings.accent_color").to_string(),
                Button::new("settings-accent")
                    .small()
                    .outline()
                    .label(accents[selected_accent_index].1.clone())
                    .dropdown_caret(true)
                    .popup_menu(move |menu, _, _| {
                        accents.iter().fold(menu, |menu, (choice, label)| {
                            let settings = accent_settings.clone();
                            let choice = *choice;
                            menu.item(
                                PopupMenuItem::new(label.clone())
                                    .checked(choice == accent)
                                    .on_click(move |_, _, cx| {
                                        settings.update(cx, |this, cx| {
                                            this.draft.accent_color = Some(choice);
                                            cx.notify();
                                        });
                                    }),
                            )
                        })
                    }),
            ))
            .child(form_row(
                t!("settings.language").to_string(),
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
            ))
            .child(form_row(
                t!("settings.show_support_reminder").to_string(),
                Switch::new("settings-support-reminder")
                    .small()
                    .checked(self.draft.show_support_reminder.unwrap_or(true))
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.draft.show_support_reminder = Some(*checked);
                        cx.notify();
                    })),
            ))
    }
}
