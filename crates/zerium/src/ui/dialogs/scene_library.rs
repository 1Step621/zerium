//! Registration settings for scene-file references shared across projects.
use crate::{
    app::{
        plugins,
        project::validate_scene_file,
        storage::config::{ConfigStore, SceneLibrary},
    },
    ui::components::{
        form::{form_dialog, form_fields},
        pane::settings_section,
    },
};
use ::ui::{
    ActiveTheme as _, ContextModal as _, Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    modal::{Modal, ModalButtonProps},
    notification::Notification,
};
use gpui::{App, AppContext as _, Context, Render, Task, Window, div, prelude::*, px};
use rust_i18n::t;
use zerium_core::persistence::PROJECT_EXTENSION;

struct SceneLibrarySettings {
    draft: SceneLibrary,
    registration_task: Option<Task<()>>,
}

pub(in crate::ui) fn open(window: &mut Window, cx: &mut App) {
    if window.has_active_modal(cx) {
        return;
    }
    let draft = match ConfigStore::read(cx) {
        Ok(config) => config.scene_library.clone(),
        Err(error) => {
            log::warn!("Could not open scene library settings: {error}");
            window.push_notification(
                Notification::error(
                    t!("settings.load_failed", error = error.to_string()).to_string(),
                ),
                cx,
            );
            return;
        }
    };
    let settings = cx.new(|_| SceneLibrarySettings {
        draft,
        registration_task: None,
    });
    window.open_modal(cx, move |modal: Modal, window, _| {
        let confirm_settings = settings.clone();
        form_dialog(modal, t!("scene_library.title").to_string(), window)
            .button_props(
                ModalButtonProps::default()
                    .ok_text(t!("menu.save").to_string())
                    .cancel_text(t!("common.close").to_string()),
            )
            .on_ok(move |_, window, cx| {
                let settings = confirm_settings.read(cx);
                if settings.registration_task.is_some() {
                    return false;
                }
                let draft = settings.draft.clone();
                match ConfigStore::update(cx, |config| config.scene_library = draft) {
                    Ok(()) => window.push_notification(
                        Notification::success(t!("settings.saved").to_string()),
                        cx,
                    ),
                    Err(error) => {
                        log::warn!("Could not save scene library settings: {error}");
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

impl SceneLibrarySettings {
    fn register_scenes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.registration_task.is_some() {
            return;
        }
        let selection = rfd::AsyncFileDialog::new()
            .set_parent(window)
            .set_title(t!("scene_library.add").to_string())
            .add_filter("Zerium", &[PROJECT_EXTENSION])
            .pick_files();
        let plugins = plugins::plugins();
        self.registration_task = Some(cx.spawn_in(window, async move |settings, cx| {
            let result = if let Some(files) = selection.await {
                cx.background_spawn(async move {
                    files
                        .into_iter()
                        .map(|file| {
                            let path = file.path();
                            validate_scene_file(path, &plugins).map_err(|error| {
                                t!(
                                    "scene_library.invalid_file",
                                    path = path.display(),
                                    error = error
                                )
                                .to_string()
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .await
                .map(Some)
            } else {
                Ok(None)
            };
            let _ = settings.update_in(cx, |this, window, cx| {
                this.registration_task = None;
                match result {
                    Ok(Some(paths)) => this.draft.add(paths),
                    Ok(None) => {}
                    Err(error) => window.push_notification(Notification::error(error), cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

impl Render for SceneLibrarySettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let library = &self.draft;
        form_fields()
            .child(
                settings_section(colors)
                    .overflow_hidden()
                    .bg(colors.list_even)
                    .when(library.is_empty(), |this| {
                        this.child(
                            div()
                                .p_3()
                                .text_color(colors.muted_foreground)
                                .child(t!("scene_library.empty").to_string()),
                        )
                    })
                    .when(!library.is_empty(), |this| {
                        this.child(
                            div()
                                .id("settings-scene-library")
                                .max_h(px(320.))
                                .overflow_y_scroll()
                                .children(library.files().iter().enumerate().map(
                                    |(index, path)| {
                                        let name = path
                                            .file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                            .into_owned();
                                        let display_path = path.display().to_string();
                                        let path = path.clone();
                                        div()
                                            .w_full()
                                            .min_w_0()
                                            .flex_none()
                                            .flex()
                                            .items_center()
                                            .gap_3()
                                            .px_3()
                                            .py_2()
                                            .when(index > 0, |this| {
                                                this.border_t_1().border_color(colors.border)
                                            })
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .flex()
                                                    .flex_col()
                                                    .child(
                                                        div()
                                                            .overflow_hidden()
                                                            .text_ellipsis()
                                                            .whitespace_nowrap()
                                                            .child(name),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(colors.muted_foreground)
                                                            .overflow_hidden()
                                                            .text_ellipsis()
                                                            .whitespace_nowrap()
                                                            .child(display_path),
                                                    ),
                                            )
                                            .child(
                                                Button::new(("settings-remove-scene", index))
                                                    .small()
                                                    .compact()
                                                    .ghost()
                                                    .icon(IconName::Xmark)
                                                    .tooltip(t!("scene_library.remove").to_string())
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            this.draft.remove(&path);
                                                            cx.notify();
                                                        },
                                                    )),
                                            )
                                    },
                                )),
                        )
                    }),
            )
            .child(
                div().child(
                    Button::new("settings-register-scenes")
                        .small()
                        .icon(IconName::Plus)
                        .label(t!("scene_library.add").to_string())
                        .disabled(self.registration_task.is_some())
                        .on_click(
                            cx.listener(|this, _, window, cx| this.register_scenes(window, cx)),
                        ),
                ),
            )
    }
}
