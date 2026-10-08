use rust_i18n::t;
use std::sync::Arc;

use ::ui::slider::{SliderEvent, SliderState};
use futures::StreamExt as _;
use gpui::{
    Context, Entity, SharedString, Subscription, Task, WgpuSurfaceHandle, Window, prelude::*,
};

mod editor_overlay;
mod frame;
mod render;

use editor_overlay::PreviewScalarDragOrigin;

use crate::{
    engine::{
        audio_meter::AudioLevelSampler,
        media::MediaReaderRegistry,
        rendering::{RenderError, RenderRuntime, RenderSize, TextFrameCache},
        video_playback::VideoPlaybackEngine,
    },
    project_session::{ProjectSession, ProjectSessionId},
    ui::{session::UiNotifications, transport::TransportController},
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
    video_revision: u64,
    size: RenderSize,
}

pub(crate) struct Preview {
    editor: Entity<TimelineEditor>,
    transport: Entity<TransportController>,
    session: Entity<ProjectSession>,
    session_id: ProjectSessionId,
    notifications: Entity<UiNotifications>,
    video_playback: VideoPlaybackEngine,
    audio_level_sampler: AudioLevelSampler,
    seekbar: Entity<SliderState>,
    surface: Option<WgpuSurfaceHandle>,
    render_runtime: Entity<RenderRuntime>,
    text_frames: TextFrameCache,
    error: Option<SharedString>,
    playback_error: Option<SharedString>,
    rendered_frame: Option<RenderedFrame>,
    editor_drag: Option<PreviewScalarDragOrigin>,
    _subscriptions: Vec<Subscription>,
    _video_playback_task: Task<()>,
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
        let editor_subscription = cx.observe(&editor, |_, _, cx| cx.notify());
        let transport_subscription = cx.observe(&transport, |_, _, cx| cx.notify());
        let session_subscription = cx.observe(&session, |this, _, cx| {
            let session_id = this.session.read(cx).id();
            if session_id == this.session_id {
                return;
            }
            this.session_id = session_id;
            this.text_frames = TextFrameCache::new();
            this.error = None;
            this.playback_error = None;
            this.rendered_frame = None;
            this.editor_drag = None;
            this.audio_level_sampler.clear();
            this.video_playback.reset();
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
                render_runtime.read(cx).create_preview_renderer(
                    Arc::new(surface.device().clone()),
                    Arc::new(surface.queue().clone()),
                )
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
            runtime.set_preview_renderer(renderer);
        });
        let audio_level_sampler = AudioLevelSampler::new(media_readers.clone());
        let (video_playback, mut video_playback_events) = VideoPlaybackEngine::new(media_readers);
        let video_playback_task = cx.spawn(async move |preview, cx| {
            while let Some(event) = video_playback_events.next().await {
                let Some(preview) = preview.upgrade() else {
                    return;
                };
                preview.update(cx, |preview, cx| {
                    let snapshot = preview.video_playback.handle_event(event);
                    preview.handle_playback_snapshot(&snapshot, cx);
                    cx.notify();
                });
            }
        });

        Self {
            editor,
            transport,
            session,
            session_id,
            notifications,
            video_playback,
            audio_level_sampler,
            seekbar,
            surface,
            render_runtime,
            text_frames: TextFrameCache::new(),
            error: None,
            playback_error: None,
            rendered_frame: None,
            editor_drag: None,
            _subscriptions: vec![
                editor_subscription,
                transport_subscription,
                session_subscription,
                seekbar_subscription,
            ],
            _video_playback_task: video_playback_task,
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
