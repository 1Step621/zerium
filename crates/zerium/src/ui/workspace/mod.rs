pub(crate) mod notifications;
pub(crate) mod operation_hints;

use rust_i18n::t;

use ::ui::{
    ActiveTheme as _, ContextModal as _, Disableable as _, IconName, Root, Selectable as _,
    Sizable as _,
    button::{Button, ButtonVariants as _},
    menu::popup_menu::PopupMenuExt as _,
    notification::Notification,
    resizable::{ResizableState, h_resizable, resizable_panel, v_resizable},
    tab::Tab,
};
use gpui::{Context, FocusHandle, KeyUpEvent, Render, Subscription, Window, div, prelude::*, px};

use self::operation_hints::OperationHintExt as _;
use crate::{app::actions::*, ui::components::scene_menu::scene_library_menu};

pub(crate) const WORKSPACE_KEY_CONTEXT: &str = "ZeriumWorkspace";
pub(crate) const WORKSPACE_SHORTCUT_KEY_CONTEXT: &str = "ZeriumWorkspace && !Input";
const MENU_BAR_HEIGHT: f32 = 30.;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum InspectorTab {
    #[default]
    Properties,
    SceneSettings,
}

impl InspectorTab {
    const ALL: [Self; 2] = [Self::Properties, Self::SceneSettings];
}

pub(crate) struct Workspace {
    pub(crate) timeline: gpui::Entity<crate::ui::panels::timeline::Timeline>,
    pub(crate) explorer: gpui::Entity<crate::ui::panels::explorer::Explorer>,
    pub(crate) preview: gpui::Entity<crate::ui::panels::preview::Preview>,
    pub(crate) scene_settings: gpui::Entity<crate::ui::panels::scene_settings::SceneSettings>,
    pub(crate) property_inspector:
        gpui::Entity<crate::ui::panels::property_inspector::PropertyInspector>,
    pub(crate) text_placement: gpui::Entity<crate::ui::panels::text_placement::TextPlacement>,
    pub(crate) placement: Option<PlacementKind>,
    pub(crate) audio_placement: gpui::Entity<crate::ui::panels::audio_placement::AudioPlacement>,
    pub(crate) audio_placement_layout: gpui::Entity<ResizableState>,
    pub(crate) text_placement_layout: gpui::Entity<ResizableState>,
    pub(crate) inspector_tab: InspectorTab,
    pub(crate) animation_curve:
        gpui::Entity<crate::ui::panels::animation_curve::AnimationCurveEditor>,
    pub(crate) animation_curve_layout: gpui::Entity<ResizableState>,
    pub(crate) animation_selection:
        gpui::Entity<crate::ui::panels::animation_curve::AnimationSelection>,
    pub(crate) project_controller: gpui::Entity<crate::app::project::ProjectController>,
    pub(crate) export_controller: gpui::Entity<crate::ui::dialogs::export::ExportController>,
    pub(crate) notifications: gpui::Entity<crate::ui::workspace::notifications::UiNotifications>,
    pub(crate) forwarded_notifications: u64,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) operation_hints:
        gpui::Entity<crate::ui::workspace::operation_hints::OperationHintBar>,
    pub(crate) _subscriptions: Vec<Subscription>,
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

    fn split_selected_items(
        &mut self,
        _: &SplitSelectedItems,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.timeline
            .update(cx, |timeline, cx| timeline.split_selected_items(cx));
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

    fn open_log_folder(&mut self, _: &OpenLogFolder, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = crate::logging::log_file() {
            cx.reveal_path(path);
        }
    }

    fn open_about_dialog(
        &mut self,
        _: &OpenAboutDialog,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::ui::dialogs::about::open(window, cx);
    }

    fn open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        crate::ui::dialogs::config::open(window, cx);
    }

    fn open_scene_library(
        &mut self,
        _: &OpenSceneLibrary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::ui::dialogs::scene_library::open(window, cx);
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

    fn save_project(&mut self, _: &SaveProject, _window: &mut Window, cx: &mut Context<Self>) {
        self.project_controller
            .update(cx, |project, cx| project.save(cx));
    }

    fn save_project_as(&mut self, _: &SaveProjectAs, _window: &mut Window, cx: &mut Context<Self>) {
        self.project_controller
            .update(cx, |project, cx| project.save_as(cx));
    }

    fn import_scene(&mut self, action: &ImportScene, window: &mut Window, cx: &mut Context<Self>) {
        self.project_controller.update(cx, |project, cx| {
            project.import_scene(action.path.clone(), window, cx);
        });
    }

    fn can_stamp_placement(&self, cx: &gpui::App) -> bool {
        match self.placement {
            Some(PlacementKind::Text) => self.text_placement.read(cx).can_stamp(cx),
            Some(PlacementKind::Audio) => self.audio_placement.read(cx).can_stamp(cx),
            None => false,
        }
    }

    fn stamp_placement(&mut self, _: &StampPlacement, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_modal(cx) || !self.can_stamp_placement(cx) {
            cx.propagate();
            return;
        }
        match self.placement {
            Some(PlacementKind::Text) => {
                self.text_placement.update(cx, |panel, cx| panel.stamp(cx))
            }
            Some(PlacementKind::Audio) => {
                self.audio_placement.update(cx, |panel, cx| panel.stamp(cx))
            }
            None => {}
        }
    }

    fn open_placement(
        &mut self,
        action: &OpenPlacement,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.animation_selection
            .update(cx, |selection, cx| selection.clear(cx));
        let layer = zerium_core::timeline::LayerId::new(action.layer);
        match action.kind {
            PlacementKind::Text => self
                .text_placement
                .update(cx, |panel, cx| panel.open_layer(layer, cx)),
            PlacementKind::Audio => self
                .audio_placement
                .update(cx, |panel, cx| panel.open_layer(layer, cx)),
        }
        self.placement = Some(action.kind);
        cx.notify();
    }

    fn save_scene_file(&mut self, _: &SaveSceneFile, _window: &mut Window, cx: &mut Context<Self>) {
        self.project_controller
            .update(cx, |project, cx| project.save_scene_file(cx));
    }

    fn open_project_settings(
        &mut self,
        _: &OpenProjectSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.project_controller
            .update(cx, |project, cx| project.open_settings(window, cx));
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let background = cx.theme().background;
        let colors = cx.theme().colors;
        let timing_enabled = !window.has_active_modal(cx) && self.can_stamp_placement(cx);
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
        let cancelling = self.export_controller.read(cx).is_cancelling();
        let edit_timeline = self.timeline.clone();
        let edit_inspector = self.property_inspector.clone();
        let scene_focus = self.focus_handle.clone();
        window.set_window_title(&self.project_controller.read(cx).window_title(cx));
        let drawer_layer = Root::render_drawer_layer(window, cx);
        let modal_layer = Root::render_modal_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);

        div()
            .id("workspace")
            .key_context(if timing_enabled {
                "ZeriumWorkspace PlacementTiming"
            } else {
                WORKSPACE_KEY_CONTEXT
            })
            .track_focus(&self.focus_handle)
            .track_operation_hints(&self.operation_hints)
            .on_action(cx.listener(Self::copy_selected_items))
            .on_action(cx.listener(Self::cut_selected_items))
            .on_action(cx.listener(Self::paste_items))
            .on_action(cx.listener(Self::split_selected_items))
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
            .on_action(cx.listener(Self::import_scene))
            .on_action(cx.listener(Self::open_placement))
            .on_action(cx.listener(Self::save_scene_file))
            .on_action(cx.listener(Self::open_project_settings))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::open_scene_library))
            .on_action(cx.listener(Self::open_export_dialog))
            .on_action(cx.listener(Self::open_log_folder))
            .on_action(cx.listener(Self::open_about_dialog))
            .on_action(cx.listener(Self::open_item_picker))
            .on_action(cx.listener(Self::open_effect_picker))
            .on_action(cx.listener(|this, _: &HidePreviewOverlay, _, cx| {
                this.preview.update(cx, |preview, cx| {
                    preview.set_editor_overlay_hidden(true, cx);
                });
            }))
            .capture_action(cx.listener(Self::stamp_placement))
            .capture_key_up(cx.listener(|this, event: &KeyUpEvent, _, cx| {
                if event.keystroke.key.eq_ignore_ascii_case("h") {
                    this.preview.update(cx, |preview, cx| {
                        preview.set_editor_overlay_hidden(false, cx);
                    });
                }
            }))
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
                                    .popup_menu(|menu, _, _| {
                                        menu.menu(
                                            t!("menu.new_project").to_string(),
                                            Box::new(NewProject),
                                        )
                                        .menu(
                                            t!("menu.open_project").to_string(),
                                            Box::new(OpenProject),
                                        )
                                        .separator()
                                        .menu(t!("menu.save").to_string(), Box::new(SaveProject))
                                        .menu(
                                            t!("menu.save_as").to_string(),
                                            Box::new(SaveProjectAs),
                                        )
                                        .separator()
                                        .menu(
                                            t!("menu.export").to_string(),
                                            Box::new(OpenExportDialog),
                                        )
                                    }),
                            )
                            .child(
                                Button::new("settings-menu")
                                    .small()
                                    .compact()
                                    .ghost()
                                    .label(t!("menu.settings").to_string())
                                    .popup_menu(|menu, _, _| {
                                        menu.menu(
                                            t!("menu.project_settings").to_string(),
                                            Box::new(OpenProjectSettings),
                                        )
                                        .menu(
                                            t!("menu.preferences").to_string(),
                                            Box::new(OpenSettings),
                                        )
                                    }),
                            )
                            .child(
                                Button::new("edit-menu")
                                    .small()
                                    .compact()
                                    .ghost()
                                    .label(t!("menu.edit").to_string())
                                    .popup_menu(move |menu, _, cx| {
                                        let timeline = edit_timeline.read(cx);
                                        let can_undo = timeline.can_undo(cx);
                                        let can_redo = timeline.can_redo(cx);
                                        let can_copy = timeline.can_copy_items(cx);
                                        let can_paste = timeline.can_paste_items(cx);
                                        let can_split = timeline.can_split_selected_items(cx);
                                        let can_add_effect =
                                            edit_inspector.read(cx).can_add_effect(cx);
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
                                            t!("menu.cut").to_string(),
                                            Box::new(CutSelectedItems),
                                            !can_copy,
                                        )
                                        .menu_with_disabled(
                                            t!("menu.copy").to_string(),
                                            Box::new(CopySelectedItems),
                                            !can_copy,
                                        )
                                        .menu_with_disabled(
                                            t!("menu.paste").to_string(),
                                            Box::new(PasteItems),
                                            !can_paste,
                                        )
                                        .separator()
                                        .menu_with_disabled(
                                            t!("menu.split_items").to_string(),
                                            Box::new(SplitSelectedItems),
                                            !can_split,
                                        )
                                        .separator()
                                        .menu(
                                            t!("menu.add_item").to_string(),
                                            Box::new(OpenItemPicker),
                                        )
                                        .menu_with_disabled(
                                            t!("inspector.add_effect").to_string(),
                                            Box::new(OpenEffectPicker),
                                            !can_add_effect,
                                        )
                                    }),
                            )
                            .child(
                                Button::new("scene-menu")
                                    .small()
                                    .compact()
                                    .ghost()
                                    .label(t!("menu.scene").to_string())
                                    .popup_menu(move |menu, window, cx| {
                                        menu.action_context(scene_focus.clone())
                                            .menu(
                                                t!("menu.save_scene_file").to_string(),
                                                Box::new(SaveSceneFile),
                                            )
                                            .menu(
                                                t!("menu.import_scene").to_string(),
                                                Box::new(ImportScene::default()),
                                            )
                                            .separator()
                                            .menu(
                                                t!("menu.scene_library").to_string(),
                                                Box::new(OpenSceneLibrary),
                                            )
                                            .map(|menu| {
                                                scene_library_menu(
                                                    menu,
                                                    scene_focus.clone(),
                                                    window,
                                                    cx,
                                                )
                                            })
                                    }),
                            )
                            .child(
                                Button::new("help-menu")
                                    .small()
                                    .compact()
                                    .ghost()
                                    .label(t!("menu.help").to_string())
                                    .popup_menu(|menu, _, _| {
                                        menu.menu_with_disabled(
                                            t!("menu.open_log_folder").to_string(),
                                            Box::new(OpenLogFolder),
                                            crate::logging::log_file().is_none(),
                                        )
                                        .separator()
                                        .menu(
                                            t!("menu.about").to_string(),
                                            Box::new(OpenAboutDialog),
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
                                        .child(if cancelling {
                                            t!("export.cancelling").to_string()
                                        } else {
                                            t!(
                                                "workspace.exporting",
                                                completed = completed,
                                                total = total
                                            )
                                            .to_string()
                                        }),
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
                                )
                                .child(
                                    Button::new("cancel-export")
                                        .small()
                                        .ghost()
                                        .label(t!("common.cancel").to_string())
                                        .disabled(cancelling)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.export_controller.update(cx, |export, cx| {
                                                export.cancel(cx);
                                            });
                                        })),
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
                            resizable_panel()
                                .size(px(400.))
                                .child(self.timeline_panels(window, cx)),
                        ),
                ),
            )
            .child(self.operation_hints.clone())
            .when_some(drawer_layer, |this, layer| this.child(layer))
            .when_some(modal_layer, |this, layer| this.child(layer))
            .when_some(notification_layer, |this, layer| this.child(layer))
    }
}

impl Workspace {
    fn timeline_panels(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let show_animation = self.animation_curve.read(cx).has_visible_curve(cx);
        let max_width = (window.viewport_size().width - px(300.)).min(px(1000.));
        let (layout, size, panel) = match self.placement {
            Some(kind) => (
                match kind {
                    PlacementKind::Text => &self.text_placement_layout,
                    PlacementKind::Audio => &self.audio_placement_layout,
                },
                px(520.),
                self.placement_panel(kind, cx).into_any_element(),
            ),
            None => (
                &self.animation_curve_layout,
                px(800.),
                div()
                    .size_full()
                    .overflow_hidden()
                    .child(self.animation_curve.clone())
                    .into_any_element(),
            ),
        };

        h_resizable("workspace-timeline-columns")
            .state(layout.clone())
            .child(
                resizable_panel()
                    .size_range(px(300.)..gpui::Pixels::MAX)
                    .child(self.timeline.clone()),
            )
            .child(
                resizable_panel()
                    .size(size)
                    .size_range(px(200.)..max_width)
                    .visible(self.placement.is_some() || show_animation)
                    .child(panel),
            )
    }

    fn placement_panel(&self, kind: PlacementKind, cx: &mut Context<Self>) -> gpui::Div {
        let (title, content) = match kind {
            PlacementKind::Text => (
                t!("text_placement.title"),
                self.text_placement.clone().into_any_element(),
            ),
            PlacementKind::Audio => (
                t!("audio_placement.title"),
                self.audio_placement.clone().into_any_element(),
            ),
        };
        div()
            .size_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                crate::ui::components::pane::pane_header(cx.theme().colors)
                    .child(title.to_string())
                    .child(
                        Button::new("close-placement")
                            .small()
                            .compact()
                            .ghost()
                            .icon(IconName::Xmark)
                            .tooltip(t!("common.close").to_string())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.placement = None;
                                this.focus_handle.focus(window, cx);
                                cx.notify();
                            })),
                    ),
            )
            .child(div().flex_1().min_h_0().child(content))
    }

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
                crate::ui::components::pane::pane_header(cx.theme().colors)
                    .id("inspector-tabs")
                    .px_0()
                    .gap_0()
                    .justify_start()
                    .children(tabs),
            )
            .child(div().flex_1().min_h_0().overflow_hidden().child(content))
    }
}
