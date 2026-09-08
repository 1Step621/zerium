use ::ui::{
    ActiveTheme as _, ContextModal as _, Root, Sizable as _,
    button::{Button, ButtonVariants as _},
    menu::{PopupMenuItem, popup_menu::PopupMenuExt as _},
    resizable::{h_resizable, resizable_panel, v_resizable},
};
use gpui::{
    App, Application, Bounds, Context, FocusHandle, KeyBinding, Render, Subscription, Window,
    WindowBounds, WindowOptions, div, prelude::*, px, size,
};

gpui::actions!(
    zerium,
    [
        DeleteSelectedItem,
        CopySelectedItems,
        CutSelectedItems,
        PasteItems,
        TogglePlayback,
        Undo,
        Redo,
        NewProject,
        OpenProject,
        SaveProject,
        SaveProjectAs,
        OpenExportDialog
    ]
);

const WORKSPACE_KEY_CONTEXT: &str = "ZeriumWorkspace";
const WORKSPACE_SHORTCUT_KEY_CONTEXT: &str = "ZeriumWorkspace && !Input";
const MENU_BAR_HEIGHT: f32 = 30.;

struct Workspace {
    timeline: gpui::Entity<crate::ui::timeline::Timeline>,
    explorer: gpui::Entity<crate::ui::explorer::Explorer>,
    preview: gpui::Entity<crate::ui::preview::Preview>,
    property_inspector: gpui::Entity<crate::ui::property_inspector::PropertyInspector>,
    animation_curve: gpui::Entity<crate::ui::animation_curve::AnimationCurveEditor>,
    project_controller: gpui::Entity<crate::ui::project::ProjectController>,
    export_controller: gpui::Entity<crate::ui::export::ExportController>,
    notifications: gpui::Entity<crate::ui::session::UiNotifications>,
    focus_handle: FocusHandle,
    _animation_selection_subscription: Subscription,
    _editor_subscription: Subscription,
    _project_subscription: Subscription,
    _export_subscription: Subscription,
    _notification_subscription: Subscription,
}

impl Workspace {
    fn copy_selected_items(
        &mut self,
        _: &CopySelectedItems,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_focused_input(cx) {
            cx.propagate();
            return;
        }
        if self
            .timeline
            .update(cx, |timeline, cx| timeline.copy_selected_items(cx))
        {
            cx.notify();
        }
    }

    fn cut_selected_items(
        &mut self,
        _: &CutSelectedItems,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_focused_input(cx) {
            cx.propagate();
            return;
        }
        if self
            .timeline
            .update(cx, |timeline, cx| timeline.cut_selected_items(cx))
        {
            cx.notify();
        }
    }

    fn paste_items(&mut self, _: &PasteItems, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_focused_input(cx) {
            cx.propagate();
            return;
        }
        if self
            .timeline
            .update(cx, |timeline, cx| timeline.paste_items(cx))
        {
            cx.notify();
        }
    }

    fn delete_selected_item(
        &mut self,
        _: &DeleteSelectedItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_focused_input(cx) {
            cx.propagate();
            return;
        }
        self.timeline
            .update(cx, |timeline, cx| timeline.remove_selected_item(cx));
    }

    fn toggle_playback(&mut self, _: &TogglePlayback, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_focused_input(cx) {
            cx.propagate();
            return;
        }
        self.timeline
            .update(cx, |timeline, cx| timeline.toggle_playback(cx));
    }

    fn undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_focused_input(cx) {
            cx.propagate();
            return;
        }
        self.timeline.update(cx, |timeline, cx| timeline.undo(cx));
    }

    fn redo(&mut self, _: &Redo, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_focused_input(cx) {
            cx.propagate();
            return;
        }
        self.timeline.update(cx, |timeline, cx| timeline.redo(cx));
    }

    fn open_export_dialog(
        &mut self,
        _: &OpenExportDialog,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.export_controller.update(cx, |export, cx| {
            export.open_dialog(window, cx);
        });
    }

    fn new_project(&mut self, _: &NewProject, window: &mut Window, cx: &mut Context<Self>) {
        self.project_controller
            .update(cx, |project, cx| project.request_new(window, cx));
    }

    fn open_project(&mut self, _: &OpenProject, window: &mut Window, cx: &mut Context<Self>) {
        self.project_controller
            .update(cx, |project, cx| project.request_open(window, cx));
    }

    fn save_project(&mut self, _: &SaveProject, window: &mut Window, cx: &mut Context<Self>) {
        self.project_controller
            .update(cx, |project, cx| project.save(window, cx));
    }

    fn save_project_as(&mut self, _: &SaveProjectAs, window: &mut Window, cx: &mut Context<Self>) {
        self.project_controller
            .update(cx, |project, cx| project.save_as(window, cx));
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let background = cx.theme().background;
        let colors = cx.theme().colors;
        let show_animation = self.animation_curve.read(cx).has_selected_curve(cx);
        let status = [
            self.project_controller.read(cx).status(),
            self.export_controller.read(cx).status(),
            self.notifications
                .read(cx)
                .latest()
                .map(|message| message.to_string()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        let status = (!status.is_empty()).then(|| status.join("  /  "));
        let project_controller = self.project_controller.clone();
        let export_controller = self.export_controller.clone();
        let can_undo = self.timeline.read(cx).can_undo(cx);
        let can_redo = self.timeline.read(cx).can_redo(cx);
        let can_copy = self.timeline.read(cx).can_copy_items(cx);
        let can_paste = self.timeline.read(cx).can_paste_items(cx);
        window.set_window_title(&self.project_controller.read(cx).window_title(cx));
        let drawer_layer = Root::render_drawer_layer(window, cx);
        let modal_layer = Root::render_modal_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);

        div()
            .id("workspace")
            .key_context(WORKSPACE_KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::copy_selected_items))
            .on_action(cx.listener(Self::cut_selected_items))
            .on_action(cx.listener(Self::paste_items))
            .on_action(cx.listener(Self::delete_selected_item))
            .on_action(cx.listener(Self::toggle_playback))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::new_project))
            .on_action(cx.listener(Self::open_project))
            .on_action(cx.listener(Self::save_project))
            .on_action(cx.listener(Self::save_project_as))
            .on_action(cx.listener(Self::open_export_dialog))
            .size_full()
            .flex()
            .flex_col()
            .bg(background)
            .child(
                div()
                    .h(px(MENU_BAR_HEIGHT))
                    .w_full()
                    .flex_none()
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(colors.border)
                    .bg(colors.title_bar)
                    .child(
                        div()
                            .h_full()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .items_center()
                            .child(
                                Button::new("file-menu")
                                    .small()
                                    .compact()
                                    .ghost()
                                    .label("ファイル")
                                    .popup_menu(move |menu, _, _| {
                                        let new_controller = project_controller.clone();
                                        let open_controller = project_controller.clone();
                                        let save_controller = project_controller.clone();
                                        let save_as_controller = project_controller.clone();
                                        let settings_controller = project_controller.clone();
                                        let export_controller = export_controller.clone();
                                        menu.item(PopupMenuItem::new("新規プロジェクト").on_click(
                                            move |_, window, cx| {
                                                new_controller.update(cx, |project, cx| {
                                                    project.request_new(window, cx);
                                                });
                                            },
                                        ))
                                        .item(PopupMenuItem::new("プロジェクトを開く…").on_click(
                                            move |_, window, cx| {
                                                open_controller.update(cx, |project, cx| {
                                                    project.request_open(window, cx);
                                                });
                                            },
                                        ))
                                        .separator()
                                        .item(PopupMenuItem::new("保存").on_click(
                                            move |_, window, cx| {
                                                save_controller.update(cx, |project, cx| {
                                                    project.save(window, cx);
                                                });
                                            },
                                        ))
                                        .item(PopupMenuItem::new("名前を付けて保存…").on_click(
                                            move |_, window, cx| {
                                                save_as_controller.update(cx, |project, cx| {
                                                    project.save_as(window, cx);
                                                });
                                            },
                                        ))
                                        .separator()
                                        .item(PopupMenuItem::new("プロジェクト設定…").on_click(
                                            move |_, window, cx| {
                                                settings_controller.update(cx, |project, cx| {
                                                    project.open_settings(window, cx);
                                                });
                                            },
                                        ))
                                        .item(
                                            PopupMenuItem::new("書き出し…").on_click(
                                                move |_, window, cx| {
                                                    export_controller.update(cx, |export, cx| {
                                                        export.open_dialog(window, cx);
                                                    });
                                                },
                                            ),
                                        )
                                    }),
                            )
                            .child(
                                Button::new("edit-menu")
                                    .small()
                                    .compact()
                                    .ghost()
                                    .label("編集")
                                    .popup_menu(move |menu, _, _| {
                                        menu.menu_with_disabled(
                                            "元に戻す",
                                            Box::new(Undo),
                                            !can_undo,
                                        )
                                        .menu_with_disabled("やり直す", Box::new(Redo), !can_redo)
                                        .separator()
                                        .menu_with_disabled(
                                            "コピー",
                                            Box::new(CopySelectedItems),
                                            !can_copy,
                                        )
                                        .menu_with_disabled(
                                            "切り取り",
                                            Box::new(CutSelectedItems),
                                            !can_copy,
                                        )
                                        .menu_with_disabled(
                                            "貼り付け",
                                            Box::new(PasteItems),
                                            !can_paste,
                                        )
                                    }),
                            ),
                    )
                    .when_some(status, |this, status| {
                        this.child(
                            div()
                                .max_w(px(420.))
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .px_3()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(status),
                        )
                    }),
            )
            .child(
                div().flex_1().min_h_0().child(
                    v_resizable("workspace-rows")
                        .child(
                            resizable_panel().child(
                                h_resizable("workspace-columns")
                                    .child(
                                        resizable_panel()
                                            .size(px(240.))
                                            .size_range(px(180.)..px(420.))
                                            .child(self.explorer.clone()),
                                    )
                                    .child(resizable_panel().child(self.preview.clone()))
                                    .child(
                                        resizable_panel()
                                            .size(px(400.))
                                            .size_range(px(200.)..px(600.))
                                            .child(self.property_inspector.clone()),
                                    ),
                            ),
                        )
                        .child(
                            resizable_panel().size(px(400.)).child(
                                h_resizable("workspace-bottom-columns")
                                    .child(resizable_panel().child(self.timeline.clone()))
                                    .when(show_animation, |this| {
                                        this.child(
                                            resizable_panel()
                                                .size(px(800.))
                                                .size_range(px(360.)..px(1000.))
                                                .child(self.animation_curve.clone()),
                                        )
                                    }),
                            ),
                        ),
                ),
            )
            .when_some(drawer_layer, |this, layer| this.child(layer))
            .when_some(modal_layer, |this, layer| this.child(layer))
            .when_some(notification_layer, |this, layer| this.child(layer))
    }
}

pub fn run() {
    Application::new()
        .with_assets(::ui::assets::Assets)
        .run(|cx: &mut App| {
            ::ui::init(cx);
            crate::ui::theme::install(cx);
            cx.bind_keys([
                KeyBinding::new(
                    "ctrl-c",
                    CopySelectedItems,
                    Some(WORKSPACE_SHORTCUT_KEY_CONTEXT),
                ),
                KeyBinding::new(
                    "ctrl-x",
                    CutSelectedItems,
                    Some(WORKSPACE_SHORTCUT_KEY_CONTEXT),
                ),
                KeyBinding::new("ctrl-v", PasteItems, Some(WORKSPACE_SHORTCUT_KEY_CONTEXT)),
                KeyBinding::new(
                    "delete",
                    DeleteSelectedItem,
                    Some(WORKSPACE_SHORTCUT_KEY_CONTEXT),
                ),
                KeyBinding::new(
                    "space",
                    TogglePlayback,
                    Some(WORKSPACE_SHORTCUT_KEY_CONTEXT),
                ),
                KeyBinding::new("ctrl-z", Undo, Some(WORKSPACE_SHORTCUT_KEY_CONTEXT)),
                KeyBinding::new("ctrl-shift-z", Redo, Some(WORKSPACE_SHORTCUT_KEY_CONTEXT)),
                KeyBinding::new("ctrl-y", Redo, Some(WORKSPACE_SHORTCUT_KEY_CONTEXT)),
                KeyBinding::new(
                    "ctrl-shift-e",
                    OpenExportDialog,
                    Some(WORKSPACE_SHORTCUT_KEY_CONTEXT),
                ),
                KeyBinding::new("ctrl-n", NewProject, Some(WORKSPACE_KEY_CONTEXT)),
                KeyBinding::new("ctrl-o", OpenProject, Some(WORKSPACE_KEY_CONTEXT)),
                KeyBinding::new("ctrl-s", SaveProject, Some(WORKSPACE_KEY_CONTEXT)),
                KeyBinding::new("ctrl-shift-s", SaveProjectAs, Some(WORKSPACE_KEY_CONTEXT)),
            ]);
            let bounds = Bounds::centered(None, size(px(1180.), px(780.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(720.), px(520.))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("Zerium");
                    let plugins = crate::plugin_catalog::plugins();
                    let media_readers = crate::engine::media::bundled_media_readers(&plugins)
                        .expect("bundled media readers must be valid");
                    let editor = cx.new(|_| {
                        crate::domain::timeline::TimelineEditor::new(
                            crate::domain::timeline::FrameRate::FPS_30,
                            plugins.clone(),
                        )
                    });
                    let session = cx.new(|_| crate::ui::session::ProjectSession::default());
                    let notifications = cx.new(|_| crate::ui::session::UiNotifications::default());
                    let audio_playback = cx.new(|_| {
                        crate::engine::audio_playback::AudioPlaybackEngine::new(
                            media_readers.clone(),
                        )
                    });
                    let transport = cx.new(|_| {
                        crate::ui::transport::TransportController::new(
                            editor.clone(),
                            audio_playback,
                            notifications.clone(),
                        )
                    });
                    let timeline = cx.new(|cx| {
                        crate::ui::timeline::Timeline::new(
                            editor.clone(),
                            transport.clone(),
                            session.clone(),
                            notifications.clone(),
                            media_readers.clone(),
                            cx,
                        )
                    });
                    let preview = cx.new(|cx| {
                        crate::ui::preview::Preview::new(
                            crate::ui::preview::PreviewDependencies::new(
                                editor.clone(),
                                transport.clone(),
                                session.clone(),
                                notifications.clone(),
                                plugins.clone(),
                                media_readers.clone(),
                            ),
                            window,
                            cx,
                        )
                    });
                    let explorer = cx.new(|cx| {
                        crate::ui::explorer::Explorer::new(
                            session.clone(),
                            notifications.clone(),
                            cx,
                        )
                    });
                    let animation_selection =
                        cx.new(|_| crate::ui::animation_curve::AnimationSelection::default());
                    let animation_curve = cx.new(|cx| {
                        crate::ui::animation_curve::AnimationCurveEditor::new(
                            editor.clone(),
                            transport.clone(),
                            animation_selection.clone(),
                            window,
                            cx,
                        )
                    });
                    let property_inspector = cx.new(|cx| {
                        crate::ui::property_inspector::PropertyInspector::new(
                            editor.clone(),
                            animation_selection.clone(),
                            media_readers.clone(),
                            session.clone(),
                            notifications.clone(),
                            window,
                            cx,
                        )
                    });
                    let render_backend = preview.read(cx).render_backend();
                    let export_controller = cx.new(|cx| {
                        crate::ui::export::ExportController::new(
                            editor.clone(),
                            render_backend,
                            media_readers.clone(),
                            session.clone(),
                            notifications.clone(),
                            cx,
                        )
                    });
                    let project_controller = cx.new(|_| {
                        crate::ui::project::ProjectController::new(
                            editor.clone(),
                            transport.clone(),
                            animation_selection.clone(),
                            session.clone(),
                            notifications.clone(),
                            plugins.clone(),
                        )
                    });
                    let close_project_controller = project_controller.clone();
                    let close_export_controller = export_controller.clone();
                    let close_window_lifetime_guard = preview.read(cx).window_lifetime_guard();
                    let workspace = cx.new(|cx: &mut Context<Workspace>| {
                        let animation_selection_subscription =
                            cx.observe(&animation_selection, |_, _, cx| cx.notify());
                        let editor_subscription =
                            cx.observe_in(&editor, window, |this, _, window, cx| {
                                window.set_window_title(
                                    &this.project_controller.read(cx).window_title(cx),
                                );
                            });
                        let project_subscription =
                            cx.observe(&project_controller, |_, _, cx| cx.notify());
                        let export_subscription =
                            cx.observe(&export_controller, |_, _, cx| cx.notify());
                        let notification_subscription =
                            cx.observe(&notifications, |_, _, cx| cx.notify());
                        Workspace {
                            timeline,
                            explorer,
                            preview,
                            property_inspector,
                            animation_curve,
                            project_controller,
                            export_controller,
                            notifications,
                            focus_handle: cx.focus_handle(),
                            _animation_selection_subscription: animation_selection_subscription,
                            _editor_subscription: editor_subscription,
                            _project_subscription: project_subscription,
                            _export_subscription: export_subscription,
                            _notification_subscription: notification_subscription,
                        }
                    });
                    window.on_window_should_close(cx, move |window, cx| {
                        let _keep_native_window_alive = &close_window_lifetime_guard;
                        let export_busy = close_export_controller.read(cx).is_busy();
                        close_project_controller.update(cx, |project, cx| {
                            project.should_close(export_busy, window, cx)
                        })
                    });
                    let workspace_focus = workspace.read(cx).focus_handle.clone();
                    window.focus(&workspace_focus, cx);
                    cx.new(|cx| Root::new(workspace.into(), window, cx))
                },
            )
            .expect("failed to open the application window");

            cx.activate(true);
        });
}
