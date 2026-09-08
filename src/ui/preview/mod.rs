mod video;

use video::VideoPlaybackRequest;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use ::ui::ActiveTheme as _;
use gpui::{
    Context, Entity, Render, SharedString, Subscription, WgpuSurfaceHandle, Window, div,
    prelude::*, wgpu_surface,
};

use crate::{
    domain::{
        plugin::PluginRegistry,
        timeline::{LayerId, TimelineEditor, TimelineItem, TimelineTime},
    },
    engine::{
        media::{MediaReaderRegistry, VideoDecodeSize},
        rendering::{
            FrameRenderer, RenderError, RenderScene, RenderSize, RendererBuilder, TextFrameCache,
        },
    },
    ui::{
        session::{ProjectSession, ProjectSessionId, UiNotifications},
        transport::{PreviewPlaybackMode, TransportController},
    },
};

use self::video::{VideoInputId, VideoPlaybackEngine};

pub(crate) struct RenderBackend {
    renderer: Option<Arc<FrameRenderer>>,
    error: Option<SharedString>,
}

impl RenderBackend {
    pub(crate) fn renderer(&self) -> Option<Arc<FrameRenderer>> {
        self.renderer.clone()
    }

    pub(crate) fn error(&self) -> Option<SharedString> {
        self.error.clone()
    }
}

pub(crate) struct PreviewDependencies {
    editor: Entity<TimelineEditor>,
    transport: Entity<TransportController>,
    session: Entity<ProjectSession>,
    notifications: Entity<UiNotifications>,
    plugins: Arc<PluginRegistry>,
    media_readers: Arc<MediaReaderRegistry>,
}

impl PreviewDependencies {
    pub(crate) fn new(
        editor: Entity<TimelineEditor>,
        transport: Entity<TransportController>,
        session: Entity<ProjectSession>,
        notifications: Entity<UiNotifications>,
        plugins: Arc<PluginRegistry>,
        media_readers: Arc<MediaReaderRegistry>,
    ) -> Self {
        Self {
            editor,
            transport,
            session,
            notifications,
            plugins,
            media_readers,
        }
    }
}

struct PreparedVideoFrames {
    revision: u64,
    frames: HashMap<(u64, VideoInputId), Arc<crate::engine::frame::RgbaFrame>>,
    error: Option<String>,
}

pub(crate) struct Preview {
    editor: Entity<TimelineEditor>,
    transport: Entity<TransportController>,
    session: Entity<ProjectSession>,
    session_id: ProjectSessionId,
    notifications: Entity<UiNotifications>,
    video_playback: Entity<VideoPlaybackEngine>,
    surface: Option<WgpuSurfaceHandle>,
    backend: Entity<RenderBackend>,
    text_frames: TextFrameCache,
    error: Option<SharedString>,
    playback_error: Option<SharedString>,
    rendered_revision: Option<u64>,
    rendered_video_revision: Option<u64>,
    rendered_size: Option<RenderSize>,
    _editor_subscription: Subscription,
    _transport_subscription: Subscription,
    _session_subscription: Subscription,
    _video_playback_subscription: Subscription,
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
            plugins,
            media_readers,
        } = dependencies;
        let session_id = session.read(cx).id();
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
            this.rendered_revision = None;
            this.rendered_video_revision = None;
            this.rendered_size = None;
            cx.notify();
        });
        let surface = window.create_wgpu_surface(
            Self::INITIAL_SIZE.width,
            Self::INITIAL_SIZE.height,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        );
        let (renderer, error) = match &surface {
            Some(surface) => match RendererBuilder::new(
                Arc::new(surface.device().clone()),
                Arc::new(surface.queue().clone()),
            )
            .and_then(|builder| builder.register_plugins(&plugins))
            .map(|builder| Arc::new(builder.build().create_session()))
            {
                Ok(renderer) => (Some(renderer), None),
                Err(error) => (None, Some(error.to_string().into())),
            },
            None => (
                None,
                Some("このプラットフォームではWGPUIのGPUサーフェスを作成できません".into()),
            ),
        };
        if let Some(error) = error.clone() {
            notifications.update(cx, |notifications, cx| {
                notifications.push(format!("プレビュー初期化失敗: {error}"), cx);
            });
        }
        let backend = cx.new(|_| RenderBackend { renderer, error });
        let video_playback = cx.new(|cx| {
            VideoPlaybackEngine::new(media_readers, session.clone(), notifications.clone(), cx)
        });
        let video_playback_subscription = cx.observe(&video_playback, |_, _, cx| cx.notify());

        Self {
            editor,
            transport,
            session,
            session_id,
            notifications,
            video_playback,
            surface,
            backend,
            text_frames: TextFrameCache::new(),
            error: None,
            playback_error: None,
            rendered_revision: None,
            rendered_video_revision: None,
            rendered_size: None,
            _editor_subscription: editor_subscription,
            _transport_subscription: transport_subscription,
            _session_subscription: session_subscription,
            _video_playback_subscription: video_playback_subscription,
        }
    }

    pub(crate) fn render_backend(&self) -> Entity<RenderBackend> {
        self.backend.clone()
    }

    /// Keeps the native window alive until GPUI has dropped its WGPU renderer.
    /// GPUI's Wayland window currently stores the native handle before the renderer,
    /// so retaining this handle in the window close callback prevents Vulkan from
    /// destroying its swapchain after the Wayland surface has already been freed.
    pub(crate) fn window_lifetime_guard(&self) -> Option<WgpuSurfaceHandle> {
        self.surface.clone()
    }

    fn prepare_scene(
        &mut self,
        active_items: &[(LayerId, TimelineItem)],
        render_time: TimelineTime,
        size: RenderSize,
        cx: &mut Context<Self>,
    ) -> Result<(RenderScene, PreparedVideoFrames), RenderError> {
        let (frame_rate, mode, resolution) = {
            let editor = self.editor.read(cx);
            (
                editor.frame_rate(),
                self.transport.read(cx).playback_mode(),
                editor.resolution(),
            )
        };
        let composition_size = RenderSize::from(resolution);
        let mut requested_time_keys = HashSet::new();
        let mut requested_times = Vec::new();
        let probe_scene = {
            let editor = self.editor.clone();
            let editor = editor.read(cx);
            let text_frames = &mut self.text_frames;
            RenderScene::from_timeline(
                editor,
                render_time,
                size,
                |request| {
                    let key = request.time.frames().to_bits();
                    if requested_time_keys.insert(key) {
                        requested_times.push(request.time);
                    }
                    Ok(None)
                },
                |item, schema, size| text_frames.frame_for(item, schema, size, composition_size),
            )?
        };
        let has_media_requests = !requested_times.is_empty();
        let timed_items = if has_media_requests {
            let editor = self.editor.read(cx);
            requested_times
                .into_iter()
                .map(|time| (time, editor.active_items_at_time(time)))
                .collect::<Vec<_>>()
        } else {
            vec![(render_time, active_items.to_vec())]
        };
        let effect_size = {
            let editor = self.editor.read(cx);
            RenderScene::effect_render_size_for_timeline(editor, render_time, size)?
        };
        let playback = self.prepare_video_frames(
            &timed_items,
            frame_rate,
            mode,
            VideoDecodeSize {
                max_width: effect_size.width,
                max_height: effect_size.height,
            },
            cx,
        );
        if !has_media_requests {
            return Ok((probe_scene, playback));
        }

        let scene = {
            let editor = self.editor.clone();
            let editor = editor.read(cx);
            let text_frames = &mut self.text_frames;
            RenderScene::from_timeline(
                editor,
                render_time,
                size,
                |request| {
                    Ok(playback
                        .frames
                        .get(&(
                            request.time.frames().to_bits(),
                            VideoInputId {
                                item_id: request.item_id,
                                input_id: request.input_id.to_owned(),
                            },
                        ))
                        .cloned())
                },
                |item, schema, size| text_frames.frame_for(item, schema, size, composition_size),
            )?
        };
        Ok((scene, playback))
    }

    fn prepare_video_frames(
        &mut self,
        requests: &[(
            TimelineTime,
            Vec<(
                crate::domain::timeline::LayerId,
                crate::domain::timeline::TimelineItem,
            )>,
        )],
        frame_rate: crate::domain::timeline::FrameRate,
        mode: PreviewPlaybackMode,
        size: VideoDecodeSize,
        cx: &mut Context<Self>,
    ) -> PreparedVideoFrames {
        let mut prepared = PreparedVideoFrames {
            revision: 0,
            frames: HashMap::new(),
            error: None,
        };
        for (time, active_items) in requests {
            let snapshot = self.video_playback.update(cx, |playback, cx| {
                playback.prepare(
                    VideoPlaybackRequest {
                        active_items,
                        playhead: time.nearest_frame(),
                        frame_rate,
                        playback_seconds: Some(time.seconds(frame_rate)),
                        mode,
                        size,
                    },
                    cx,
                )
            });
            prepared.revision = snapshot.revision;
            if snapshot.error.is_some() {
                prepared.error = snapshot.error;
            }
            prepared.frames.extend(
                snapshot
                    .frames
                    .into_iter()
                    .map(|(input, frame)| ((time.frames().to_bits(), input), frame)),
            );
        }
        prepared
    }

    fn render_latest_frame(&mut self, cx: &mut Context<Self>) {
        let Some(surface) = self.surface.clone() else {
            return;
        };
        let (width, height) = surface.size();
        let size = RenderSize { width, height };
        let (active_items, render_time, revision) = {
            let editor = self.editor.read(cx);
            let frame_rate = editor.frame_rate();
            let render_time = editor
                .playback_time_seconds()
                .and_then(|seconds| TimelineTime::from_seconds(seconds, frame_rate))
                .unwrap_or_else(|| TimelineTime::from_frame(editor.playhead()));
            (
                editor.active_items_at_time(render_time),
                render_time,
                editor.render_revision(),
            )
        };
        self.text_frames
            .retain_active(active_items.iter().map(|(_, item)| item.id));
        let (scene, playback) = match self.prepare_scene(&active_items, render_time, size, cx) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.report_error(format!("プレビューシーンの構築に失敗しました: {error}"), cx);
                return;
            }
        };
        self.playback_error = playback.error.clone().map(Into::into);

        let Some(renderer) = self.backend.read(cx).renderer() else {
            return;
        };
        if self.rendered_revision == Some(revision)
            && self.rendered_video_revision == Some(playback.revision)
            && self.rendered_size == Some(size)
        {
            return;
        }
        let Some((target_view, actual_size)) = surface.back_view_with_size() else {
            return;
        };
        let size = RenderSize {
            width: actual_size.0,
            height: actual_size.1,
        };
        let (scene, playback) = if actual_size == (width, height) {
            (scene, playback)
        } else {
            match self.prepare_scene(&active_items, render_time, size, cx) {
                Ok(prepared) => prepared,
                Err(error) => {
                    self.report_error(
                        format!("プレビューシーンの再構築に失敗しました: {error}"),
                        cx,
                    );
                    return;
                }
            }
        };
        match renderer.render_to_view(&scene, &target_view) {
            Ok(submission) => {
                drop(target_view);
                surface.present_synced_silent(submission);
                self.rendered_revision = Some(revision);
                self.rendered_video_revision = Some(playback.revision);
                self.rendered_size = Some(size);
                self.error = None;
            }
            Err(error) => {
                self.report_error(format!("プレビュー描画に失敗しました: {error}"), cx);
            }
        }
    }

    fn report_error(&mut self, message: String, cx: &mut Context<Self>) {
        let message = SharedString::from(message);
        if self.error.as_ref() == Some(&message) {
            return;
        }
        self.error = Some(message.clone());
        self.notifications
            .update(cx, |notifications, cx| notifications.push(message, cx));
    }
}

impl Render for Preview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.transport.read(cx).is_playing() {
            self.transport
                .update(cx, |transport, cx| transport.advance(cx));
            window.request_animation_frame();
        }
        self.render_latest_frame(cx);

        let colors = cx.theme().colors;
        let surface = self.surface.clone();
        let aspect_ratio = self.editor.read(cx).resolution().aspect_ratio();
        let error = self
            .backend
            .read(cx)
            .error()
            .or_else(|| self.error.clone())
            .or_else(|| self.playback_error.clone());

        div()
            .size_full()
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .bg(colors.background)
            .when_some(surface, |this, surface| {
                this.child(
                    wgpu_surface(surface)
                        .w_full()
                        .max_h_full()
                        .aspect_ratio(aspect_ratio)
                        .defer_resize_until_mouse_up(true),
                )
            })
            .when_some(error, |this, error| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_sm()
                        .text_color(colors.danger)
                        .child(error),
                )
            })
    }
}
