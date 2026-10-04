use rust_i18n::t;

use ::ui::{
    ActiveTheme as _, ContextModal as _, Root, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    menu::{PopupMenuItem, popup_menu::PopupMenuExt as _},
    notification::Notification,
    resizable::{h_resizable, resizable_panel, v_resizable},
    tab::Tab,
};
use gpui::{Context, FocusHandle, Render, Subscription, Window, div, prelude::*, px};

use super::actions::*;

pub(super) const WORKSPACE_KEY_CONTEXT: &str = "ZeriumWorkspace";
pub(super) const WORKSPACE_SHORTCUT_KEY_CONTEXT: &str = "ZeriumWorkspace && !Input";
pub(super) const MENU_BAR_HEIGHT: f32 = 30.;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum InspectorTab {
    #[default]
    Properties,
    SceneSettings,
}

impl InspectorTab {
    const ALL: [Self; 2] = [Self::Properties, Self::SceneSettings];
}

pub(super) struct Workspace {
    pub(super) timeline: gpui::Entity<crate::ui::timeline::Timeline>,
    pub(super) explorer: gpui::Entity<crate::ui::explorer::Explorer>,
    pub(super) preview: gpui::Entity<crate::ui::preview::Preview>,
    pub(super) scene_settings: gpui::Entity<crate::ui::scene_settings::SceneSettings>,
    pub(super) property_inspector: gpui::Entity<crate::ui::property_inspector::PropertyInspector>,
    pub(super) inspector_tab: InspectorTab,
    pub(super) animation_curve: gpui::Entity<crate::ui::animation_curve::AnimationCurveEditor>,
    pub(super) project_controller: gpui::Entity<crate::app::project_controller::ProjectController>,
    pub(super) export_controller: gpui::Entity<crate::ui::export::ExportController>,
    pub(super) notifications: gpui::Entity<crate::ui::session::UiNotifications>,
    pub(super) forwarded_notifications: u64,
    pub(super) focus_handle: FocusHandle,
    pub(super) _animation_selection_subscription: Subscription,
    pub(super) _editor_subscription: Subscription,
    pub(super) _project_subscription: Subscription,
    pub(super) _export_subscription: Subscription,
    pub(super) _notification_subscription: Subscription,
    pub(super) _inspector_subscription: Subscription,
}

impl Workspace {
    fn copy_selected_items(
        &mut self,
        _: &CopySelectedItems,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .timeline
            .update(cx, |timeline, cx| timeline.cut_selected_items(cx))
        {
            cx.notify();
        }
    }

    fn paste_items(&mut self, _: &PasteItems, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .timeline
            .update(cx, |timeline, cx| timeline.paste_items(window, cx))
        {
            cx.notify();
        }
    }

    fn delete_selected_item(
        &mut self,
        _: &DeleteSelectedItem,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.timeline
            .update(cx, |timeline, cx| timeline.remove_selected_item(cx));
    }

    fn toggle_playback(
        &mut self,
        _: &TogglePlayback,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.timeline
            .update(cx, |timeline, cx| timeline.toggle_playback(cx));
    }

    fn toggle_playback_in_place(
        &mut self,
        _: &TogglePlaybackInPlace,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.timeline
            .update(cx, |timeline, cx| timeline.toggle_playback_in_place(cx));
    }

    fn previous_frame(&mut self, _: &PreviousFrame, _window: &mut Window, cx: &mut Context<Self>) {
        self.timeline
            .update(cx, |timeline, cx| timeline.step_frame(-1, cx));
    }

    fn next_frame(&mut self, _: &NextFrame, _window: &mut Window, cx: &mut Context<Self>) {
        self.timeline
            .update(cx, |timeline, cx| timeline.step_frame(1, cx));
    }

    fn undo(&mut self, _: &Undo, _window: &mut Window, cx: &mut Context<Self>) {
        self.timeline.update(cx, |timeline, cx| timeline.undo(cx));
    }

    fn redo(&mut self, _: &Redo, _window: &mut Window, cx: &mut Context<Self>) {
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

    fn open_item_picker(
        &mut self,
        _: &OpenItemPicker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.timeline.update(cx, |timeline, cx| {
            timeline.open_item_picker_at_cursor(window, cx)
        });
    }

    fn open_effect_picker(
        &mut self,
        _: &OpenEffectPicker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.inspector_tab = InspectorTab::Properties;
        self.property_inspector
            .update(cx, |inspector, cx| inspector.open_effect_picker(window, cx));
        cx.notify();
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
        // Forward new messages to the toast layer. Only unseen messages are
        // pushed, so re-renders never duplicate toasts and the push itself
        // (which notifies the toast list, not this workspace) cannot loop.
        let seen = self.forwarded_notifications;
        let (unseen, next) = self.notifications.read(cx).unseen_since(seen);
        self.forwarded_notifications = next;
        for (message, success) in unseen {
            let notification = if success {
                Notification::success(message)
            } else {
                Notification::error(message)
            };
            window.push_notification(notification, cx);
        }
        let export_progress = self.export_controller.read(cx).export_progress();
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
            .on_action(cx.listener(Self::toggle_playback_in_place))
            .on_action(cx.listener(Self::previous_frame))
            .on_action(cx.listener(Self::next_frame))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::new_project))
            .on_action(cx.listener(Self::open_project))
            .on_action(cx.listener(Self::save_project))
            .on_action(cx.listener(Self::save_project_as))
            .on_action(cx.listener(Self::open_export_dialog))
            .on_action(cx.listener(Self::open_item_picker))
            .on_action(cx.listener(Self::open_effect_picker))
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
                                    .label(t!("menu.file").to_string())
                                    .popup_menu(move |menu, _, _| {
                                        let new_controller = project_controller.clone();
                                        let open_controller = project_controller.clone();
                                        let save_controller = project_controller.clone();
                                        let save_as_controller = project_controller.clone();
                                        let settings_controller = project_controller.clone();
                                        let export_controller = export_controller.clone();
                                        menu.item(
                                            PopupMenuItem::new(t!("menu.new_project").to_string())
                                                .on_click(move |_, window, cx| {
                                                    new_controller.update(cx, |project, cx| {
                                                        project.request_new(window, cx);
                                                    });
                                                }),
                                        )
                                        .item(
                                            PopupMenuItem::new(t!("menu.open_project").to_string())
                                                .on_click(move |_, window, cx| {
                                                    open_controller.update(cx, |project, cx| {
                                                        project.request_open(window, cx);
                                                    });
                                                }),
                                        )
                                        .separator()
                                        .item(
                                            PopupMenuItem::new(t!("menu.save").to_string())
                                                .on_click(move |_, window, cx| {
                                                    save_controller.update(cx, |project, cx| {
                                                        project.save(window, cx);
                                                    });
                                                }),
                                        )
                                        .item(
                                            PopupMenuItem::new(t!("menu.save_as").to_string())
                                                .on_click(move |_, window, cx| {
                                                    save_as_controller.update(cx, |project, cx| {
                                                        project.save_as(window, cx);
                                                    });
                                                }),
                                        )
                                        .separator()
                                        .item(
                                            PopupMenuItem::new(
                                                t!("menu.project_settings").to_string(),
                                            )
                                            .on_click(
                                                move |_, window, cx| {
                                                    settings_controller.update(
                                                        cx,
                                                        |project, cx| {
                                                            project.open_settings(window, cx);
                                                        },
                                                    );
                                                },
                                            ),
                                        )
                                        .item(
                                            PopupMenuItem::new(t!("menu.export").to_string())
                                                .on_click(move |_, window, cx| {
                                                    export_controller.update(cx, |export, cx| {
                                                        export.open_dialog(window, cx);
                                                    });
                                                }),
                                        )
                                    }),
                            )
                            .child(
                                Button::new("edit-menu")
                                    .small()
                                    .compact()
                                    .ghost()
                                    .label(t!("menu.edit").to_string())
                                    .popup_menu(move |menu, _, _| {
                                        menu.menu_with_disabled(
                                            t!("menu.undo").to_string(),
                                            Box::new(Undo),
                                            !can_undo,
                                        )
                                        .menu_with_disabled(
                                            t!("menu.redo").to_string(),
                                            Box::new(Redo),
                                            !can_redo,
                                        )
                                        .separator()
                                        .menu_with_disabled(
                                            t!("menu.copy").to_string(),
                                            Box::new(CopySelectedItems),
                                            !can_copy,
                                        )
                                        .menu_with_disabled(
                                            t!("menu.cut").to_string(),
                                            Box::new(CutSelectedItems),
                                            !can_copy,
                                        )
                                        .menu_with_disabled(
                                            t!("menu.paste").to_string(),
                                            Box::new(PasteItems),
                                            !can_paste,
                                        )
                                    }),
                            ),
                    )
                    .when_some(export_progress, |this, (completed, total)| {
                        let fraction = if total == 0 {
                            0.
                        } else {
                            (completed.min(total) as f32 / total as f32).clamp(0., 1.)
                        };
                        this.child(
                            div()
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap_2()
                                .px_3()
                                .child(
                                    div()
                                        .text_xs()
                                        .whitespace_nowrap()
                                        .text_color(colors.muted_foreground)
                                        .child(
                                            t!(
                                                "workspace.exporting",
                                                completed = completed,
                                                total = total
                                            )
                                            .to_string(),
                                        ),
                                )
                                .child(
                                    div()
                                        .w(px(120.))
                                        .h(px(4.))
                                        .rounded_full()
                                        .bg(colors.border)
                                        .overflow_hidden()
                                        .child(
                                            div()
                                                .h_full()
                                                .w(gpui::relative(fraction))
                                                .rounded_full()
                                                .bg(colors.foreground),
                                        ),
                                ),
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
                                            .size(px(300.))
                                            .size_range(px(200.)..px(600.))
                                            .child(self.explorer.clone()),
                                    )
                                    .child(resizable_panel().child(self.preview.clone()))
                                    .child(
                                        resizable_panel()
                                            .size(px(420.))
                                            .size_range(px(420.)..px(600.))
                                            .child(self.inspector_panel(cx)),
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

impl Workspace {
    fn inspector_panel(&self, cx: &mut Context<Self>) -> gpui::Div {
        let labels = [
            t!("inspector.properties").to_string(),
            t!("args.scene_settings").to_string(),
        ];
        let tabs =
            InspectorTab::ALL
                .into_iter()
                .zip(labels)
                .enumerate()
                .map(|(index, (tab, label))| {
                    Tab::new(label)
                        .id(index)
                        .small()
                        .selected(self.inspector_tab == tab)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.inspector_tab = tab;
                            this.focus_handle.focus(window, cx);
                            cx.notify();
                        }))
                });
        let content = match self.inspector_tab {
            InspectorTab::Properties => self.property_inspector.clone().into_any_element(),
            InspectorTab::SceneSettings => self.scene_settings.clone().into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                crate::ui::pane::pane_header(cx.theme().colors)
                    .id("inspector-tabs")
                    .px_0()
                    .gap_0()
                    .justify_start()
                    .children(tabs),
            )
            .child(div().flex_1().min_h_0().overflow_hidden().child(content))
    }
}
