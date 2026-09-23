use std::path::PathBuf;

use ::ui::Root;
use gpui::{
    App, AppContext as _, Application, Bounds, Context, KeyBinding, WindowBounds, WindowIcon,
    WindowOptions, px, size,
};

use super::{
    actions::*,
    workspace::{WORKSPACE_KEY_CONTEXT, WORKSPACE_SHORTCUT_KEY_CONTEXT, Workspace},
};

const WINDOW_ICON_PNG: &[u8] = include_bytes!("../../assets/zerium.png");

pub(crate) fn run(initial_project: Option<PathBuf>) {
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
                KeyBinding::new("left", PreviousFrame, Some(WORKSPACE_SHORTCUT_KEY_CONTEXT)),
                KeyBinding::new("right", NextFrame, Some(WORKSPACE_SHORTCUT_KEY_CONTEXT)),
                KeyBinding::new("ctrl-z", Undo, Some(WORKSPACE_SHORTCUT_KEY_CONTEXT)),
                KeyBinding::new("ctrl-shift-z", Redo, Some(WORKSPACE_SHORTCUT_KEY_CONTEXT)),
                KeyBinding::new("ctrl-y", Redo, Some(WORKSPACE_SHORTCUT_KEY_CONTEXT)),
                KeyBinding::new(
                    "ctrl-shift-e",
                    OpenExportDialog,
                    Some(WORKSPACE_SHORTCUT_KEY_CONTEXT),
                ),
                KeyBinding::new("i", OpenItemPicker, Some(WORKSPACE_SHORTCUT_KEY_CONTEXT)),
                KeyBinding::new("e", OpenEffectPicker, Some(WORKSPACE_SHORTCUT_KEY_CONTEXT)),
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
                    app_icon: Some(
                        WindowIcon::from_png_bytes(WINDOW_ICON_PNG)
                            .expect("bundled window icon must be a valid PNG"),
                    ),
                    ..Default::default()
                },
                move |window, cx| {
                    window.set_window_title("Zerium");
                    let plugins = crate::plugin_loader::plugins();
                    let plugin_shaders = crate::engine::rendering::compile_plugins(&plugins)
                        .expect("bundled plugin shaders must compile");
                    let render_runtime =
                        cx.new(|_| crate::engine::rendering::RenderRuntime::new(plugin_shaders));
                    let media_readers = crate::engine::media::bundled_media_readers(&plugins)
                        .expect("bundled media readers must be valid");
                    let editor = cx.new(|_| {
                        crate::domain::timeline::TimelineEditor::new(
                            crate::domain::timeline::FrameRate::FPS_30,
                            plugins.clone(),
                        )
                    });
                    let session =
                        cx.new(|_| crate::application::project_session::ProjectSession::default());
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
                    let animation_selection =
                        cx.new(|_| crate::ui::animation_curve::AnimationSelection::default());
                    let timeline = cx.new(|cx| {
                        crate::ui::timeline::Timeline::new(
                            editor.clone(),
                            transport.clone(),
                            animation_selection.clone(),
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
                                render_runtime.clone(),
                                media_readers.clone(),
                            ),
                            window,
                            cx,
                        )
                    });
                    let explorer = cx.new(|cx| {
                        crate::ui::explorer::Explorer::new(
                            plugins.clone(),
                            session.clone(),
                            notifications.clone(),
                            cx,
                        )
                    });
                    let animation_curve = cx.new(|cx| {
                        crate::ui::animation_curve::AnimationCurveEditor::new(
                            editor.clone(),
                            transport.clone(),
                            animation_selection.clone(),
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
                    let export_controller = cx.new(|cx| {
                        crate::ui::export::ExportController::new(
                            editor.clone(),
                            render_runtime.clone(),
                            media_readers.clone(),
                            session.clone(),
                            notifications.clone(),
                            cx,
                        )
                    });
                    let project_controller = cx.new(|_| {
                        let runtime = crate::app::project_runtime::ProjectRuntime::new(
                            editor.clone(),
                            transport.clone(),
                            animation_selection.clone(),
                            session.clone(),
                        );
                        crate::app::project_controller::ProjectController::new(
                            runtime,
                            notifications.clone(),
                        )
                    });
                    if let Some(path) = initial_project {
                        project_controller.update(cx, |project, cx| {
                            project.open_path(path, cx);
                        });
                    }
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
                            forwarded_notifications: 0,
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
