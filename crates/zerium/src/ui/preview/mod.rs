use crate::engine::gpu::GpuContext;
use rust_i18n::t;
use std::sync::Arc;

use ::ui::slider::{SliderEvent, SliderState};
use futures::StreamExt as _;
use gpui::{
    App, Context, Entity, SharedString, Subscription, Task, WgpuSurfaceHandle, Window, prelude::*,
};

mod editor_overlay;
mod frame;
mod render;

use editor_overlay::PreviewEditorDrag;

use crate::{
    engine::{
        audio_meter::AudioLevelSampler,
        media::MediaReaderRegistry,
        preview::PreviewFrameScheduler,
        rendering::{RenderError, RenderRuntime, RenderSize, SceneEvaluator},
    },
    project_session::{ProjectSession, ProjectSessionId},
    ui::{notifications::UiNotifications, transport::TransportController},
};
use zerium_core::timeline::{Frame, TimelineEditor};

pub(crate) struct PreviewDependencies {
    editor: Entity<TimelineEditor>,
    transport: Entity<TransportController>,
    session: Entity<ProjectSession>,
    notifications: Entity<UiNotifications>,
    render_runtime: Entity<RenderRuntime>,
    media_readers: Arc<MediaReaderRegistry>,
}

impl PreviewDependencies {
    pub(crate) fn new(
        editor: Entity<TimelineEditor>,
        transport: Entity<TransportController>,
        session: Entity<ProjectSession>,
        notifications: Entity<UiNotifications>,
        render_runtime: Entity<RenderRuntime>,
        media_readers: Arc<MediaReaderRegistry>,
    ) -> Self {
        Self {
            editor,
            transport,
            session,
            notifications,
            render_runtime,
            media_readers,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct RenderedFrame {
    editor_revision: u64,
    frame_revision: u64,
    size: RenderSize,
}

pub(crate) struct Preview {
    editor: Entity<TimelineEditor>,
    transport: Entity<TransportController>,
    session: Entity<ProjectSession>,
    session_id: ProjectSessionId,
    notifications: Entity<UiNotifications>,
    frame_scheduler: PreviewFrameScheduler,
    audio_level_sampler: AudioLevelSampler,
    seekbar: Entity<SliderState>,
    surface: Option<WgpuSurfaceHandle>,
    render_runtime: Entity<RenderRuntime>,
    scene_evaluator: SceneEvaluator,
    error: Option<SharedString>,
    frame_error: Option<SharedString>,
    rendered_frame: Option<RenderedFrame>,
    editor_drag: Option<PreviewEditorDrag>,
    editor_overlay_hidden: bool,
    _subscriptions: Vec<Subscription>,
    _frame_task: Task<()>,
}

impl Preview {
    const INITIAL_SIZE: RenderSize = RenderSize {
        width: 640,
        height: 360,
    };

    pub(crate) fn new(
        dependencies: PreviewDependencies,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let PreviewDependencies {
            editor,
            transport,
            session,
            notifications,
            render_runtime,
            media_readers,
        } = dependencies;
        let session_id = session.read(cx).id();
        let seekbar = cx.new(|_| SliderState::new().min(0.).max(1.).step(0.001));
        let seekbar_subscription = cx.subscribe(&seekbar, |preview, _, event: &SliderEvent, cx| {
            let SliderEvent::Change(value) = event;
            let end = preview.editor.read(cx).end_frame_exclusive().get();
            let frame =
                Frame::new((f64::from(value.end().clamp(0., 1.)) * end as f64).round() as u64);
            preview
                .transport
                .update(cx, |transport, cx| transport.set_playhead(frame, cx));
        });
        let editor_subscription = cx.observe_in(&editor, window, |preview, _, window, cx| {
            preview.sync_seekbar(window, cx);
            cx.notify();
        });
        let transport_subscription = cx.observe(&transport, |_, _, cx| cx.notify());
        let activation_subscription = cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.set_editor_overlay_hidden(false, cx);
            }
        });
        let session_subscription = cx.observe(&session, |this, _, cx| {
            let session_id = this.session.read(cx).id();
            if session_id == this.session_id {
                return;
            }
            this.session_id = session_id;
            this.scene_evaluator = SceneEvaluator::new();
            this.error = None;
            this.frame_error = None;
            this.rendered_frame = None;
            this.editor_drag = None;
            this.audio_level_sampler.clear();
            this.frame_scheduler.reset();
            cx.notify();
        });
        let surface = window.create_wgpu_surface(
            Self::INITIAL_SIZE.width,
            Self::INITIAL_SIZE.height,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        );
        let renderer = surface
            .as_ref()
            .ok_or_else(|| {
                RenderError::backend("Cannot create a WGPUI GPU surface on this platform")
            })
            .and_then(|surface| {
                render_runtime
                    .read(cx)
                    .create_preview_session(GpuContext::new(
                        surface.device().clone(),
                        surface.queue().clone(),
                    ))
            });
        let error: Option<SharedString> = renderer
            .as_ref()
            .err()
            .map(|error| error.to_string().into());
        if let Some(error) = error.clone() {
            notifications.update(cx, |notifications, cx| {
                notifications.push(
                    t!("preview.initialize_failed", error = error).to_string(),
                    cx,
                );
            });
        }
        render_runtime.update(cx, |runtime, _| {
            runtime.set_preview_session(renderer);
        });
        let audio_level_sampler = AudioLevelSampler::new(media_readers.clone());
        let (frame_scheduler, mut frame_events) =
            PreviewFrameScheduler::new(media_readers, render_runtime.read(cx).preview_session());
        let frame_task = cx.spawn(async move |preview, cx| {
            while let Some(event) = frame_events.next().await {
                let Some(preview) = preview.upgrade() else {
                    return;
                };
                preview.update(cx, |preview, cx| {
                    let snapshot = preview.frame_scheduler.handle_event(event);
                    let redraw = preview.frame_scheduler.needs_redraw()
                        || preview.frame_error.as_deref() != snapshot.error.as_deref()
                        || !snapshot.notifications.is_empty();
                    preview.handle_frame_status(&snapshot, cx);
                    if redraw {
                        cx.notify();
                    }
                });
            }
        });

        Self {
            editor,
            transport,
            session,
            session_id,
            notifications,
            frame_scheduler,
            audio_level_sampler,
            seekbar,
            surface,
            render_runtime,
            scene_evaluator: SceneEvaluator::new(),
            error: None,
            frame_error: None,
            rendered_frame: None,
            editor_drag: None,
            editor_overlay_hidden: false,
            _subscriptions: vec![
                editor_subscription,
                transport_subscription,
                session_subscription,
                seekbar_subscription,
                activation_subscription,
            ],
            _frame_task: frame_task,
        }
    }

    pub(crate) fn set_editor_overlay_hidden(&mut self, hidden: bool, cx: &mut Context<Self>) {
        if self.editor_overlay_hidden != hidden {
            self.editor_overlay_hidden = hidden;
            cx.notify();
        }
    }

    fn seekbar_progress(&self, cx: &App) -> f32 {
        let editor = self.editor.read(cx);
        let end = editor.end_frame_exclusive().get();
        if end == 0 {
            0.
        } else {
            (editor.playhead().get() as f32 / end as f32).clamp(0., 1.)
        }
    }

    fn sync_seekbar(&self, window: &mut Window, cx: &mut Context<Self>) {
        let ratio = self.seekbar_progress(cx);
        if (self.seekbar.read(cx).value().start() - ratio).abs() > f32::EPSILON {
            self.seekbar
                .update(cx, |seekbar, cx| seekbar.set_value(ratio, window, cx));
        }
    }

    /// Keeps the native window alive until GPUI has dropped its WGPU renderer.
    /// GPUI's Wayland window currently stores the native handle before the renderer,
    /// so retaining this handle in the window close callback prevents Vulkan from
    /// destroying its swapchain after the Wayland surface has already been freed.
    pub(crate) fn window_lifetime_guard(&self) -> Option<WgpuSurfaceHandle> {
        self.surface.clone()
    }
}
