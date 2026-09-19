mod video;

use std::{collections::HashMap, sync::Arc};

use ::ui::{
    ActiveTheme as _,
    slider::{Slider, SliderEvent, SliderState},
};
use gpui::{
    Context, Entity, Hsla, MouseButton, MouseDownEvent, MouseUpEvent, Render, SharedString,
    Subscription, WgpuSurfaceHandle, Window, div, prelude::*, px, relative, wgpu_surface,
};

use crate::{
    domain::{
        plugin::PluginRegistry,
        timeline::{Frame, LayerId, TimelineEditor, TimelineItem, TimelineTime},
    },
    engine::{
        audio_meter::AudioLevelSampler,
        media::{MediaReaderRegistry, VideoDecodeSize},
        rendering::{
            FrameRenderer, RenderError, RenderScene, RenderSize, RendererBuilder, RendererDevice,
            TextFrameCache,
        },
    },
    ui::{
        session::{ProjectSession, ProjectSessionId, UiNotifications},
        time_grid,
        transport::{ScrubSource, TransportController},
    },
};

use self::video::{RequestedVideoFrame, VideoInputId, VideoPlaybackEngine};

pub(crate) struct RenderBackend {
    renderer: Option<Arc<FrameRenderer>>,
    export_device: Option<Arc<RendererDevice>>,
    error: Option<SharedString>,
}

impl RenderBackend {
    pub(crate) fn renderer(&self) -> Option<Arc<FrameRenderer>> {
        self.renderer.clone()
    }

    /// Rendering session on the dedicated export device, creating the device
    /// on first use.
    pub(crate) fn export_session(
        &mut self,
        plugins: &PluginRegistry,
    ) -> Result<Arc<FrameRenderer>, RenderError> {
        let device = match &self.export_device {
            Some(device) => device.clone(),
            None => {
                let device = RendererDevice::create_headless(plugins)?;
                self.export_device = Some(device.clone());
                device
            }
        };
        Ok(Arc::new(device.create_session()))
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

pub(crate) struct Preview {
    editor: Entity<TimelineEditor>,
    transport: Entity<TransportController>,
    session: Entity<ProjectSession>,
    session_id: ProjectSessionId,
    notifications: Entity<UiNotifications>,
    video_playback: Entity<VideoPlaybackEngine>,
    audio_level_sampler: AudioLevelSampler,
    seekbar: Entity<SliderState>,
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
    _seekbar_subscription: Subscription,
}

impl Preview {
    const METER_MIN_DB: f32 = -60.;
    const INITIAL_SIZE: RenderSize = RenderSize {
        width: 640,
        height: 360,
    };
    const BAR_THICKNESS: f32 = 6.;

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
            this.rendered_revision = None;
            this.rendered_video_revision = None;
            this.rendered_size = None;
            this.audio_level_sampler.clear();
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
        let backend = cx.new(|_| RenderBackend {
            renderer,
            export_device: None,
            error,
        });
        let audio_level_sampler = AudioLevelSampler::new(media_readers.clone());
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
            audio_level_sampler,
            seekbar,
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
            _seekbar_subscription: seekbar_subscription,
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
        render_time: TimelineTime,
        size: RenderSize,
        cx: &mut Context<Self>,
    ) -> Result<(RenderScene, video::VideoPlaybackSnapshot), RenderError> {
        let (frame_rate, mode, resolution) = {
            let editor = self.editor.read(cx);
            (
                editor.frame_rate(),
                self.transport.read(cx).playback_mode(),
                editor.resolution(),
            )
        };
        let composition_size = RenderSize::from(resolution);
        let effect_size = {
            let editor = self.editor.read(cx);
            RenderScene::effect_render_size_for_timeline(editor, render_time, size)?
        };
        let decode_size = VideoDecodeSize {
            max_width: effect_size.width,
            max_height: effect_size.height,
        };
        let video_playback = self.video_playback.clone();
        video_playback.update(cx, |playback, cx| {
            playback.begin_frame_demand(mode);
            let mut items_by_time: HashMap<u64, Vec<(LayerId, TimelineItem)>> = HashMap::new();
            let mut recorded: HashMap<(u64, VideoInputId), RequestedVideoFrame> = HashMap::new();
            let scene = {
                let editor = self.editor.clone();
                let editor = editor.read(cx);
                let text_frames = &mut self.text_frames;
                RenderScene::from_timeline(
                    editor,
                    render_time,
                    size,
                    |request| {
                        let time_bits = request.time.frames().to_bits();
                        let input = VideoInputId {
                            item_id: request.item_id,
                            input_id: request.input_id.to_owned(),
                        };
                        if !recorded.contains_key(&(time_bits, input.clone())) {
                            let items = items_by_time
                                .entry(time_bits)
                                .or_insert_with(|| editor.active_items_at_time(request.time));
                            for (input, requested) in playback.record_media_requests(
                                request.time,
                                items,
                                frame_rate,
                                decode_size,
                            ) {
                                recorded.insert((time_bits, input), requested);
                            }
                        }
                        Ok(recorded
                            .get(&(time_bits, input.clone()))
                            .and_then(|requested| {
                                playback.present_recorded_frame(&input, requested)
                            }))
                    },
                    |item, schema, size| {
                        text_frames.frame_for(item, schema, size, composition_size)
                    },
                )?
            };
            let snapshot = playback.finish_frame_demand(cx);
            Ok((scene, snapshot))
        })
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
        let (scene, playback) = match self.prepare_scene(render_time, size, cx) {
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
            match self.prepare_scene(render_time, size, cx) {
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

    fn render_controls(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let (current, end, frame_rate) = {
            let editor = self.editor.read(cx);
            let frame_rate = editor.frame_rate();
            let end = editor.end_frame_exclusive();
            let current = editor
                .playback_time_seconds()
                .map(|seconds| frame_rate.seconds_to_frame(seconds))
                .unwrap_or_else(|| editor.playhead());
            (current, end, frame_rate)
        };
        let ratio = if end.get() == 0 {
            0.
        } else {
            (current.get() as f32 / end.get() as f32).clamp(0., 1.)
        };
        if (self.seekbar.read(cx).value().start() - ratio).abs() > f32::EPSILON {
            self.seekbar
                .update(cx, |seekbar, cx| seekbar.set_value(ratio, window, cx));
        }
        let current_label = format!(
            "{}  {}f",
            time_grid::format_timestamp(frame_rate.frame_to_seconds(current)),
            current.get()
        );
        let total_label = format!(
            "{}  {}f",
            time_grid::format_timestamp(frame_rate.frame_to_seconds(end)),
            end.get()
        );
        let begin_scrub = cx.listener(|this, event: &MouseDownEvent, _, cx| {
            if event.button == MouseButton::Left {
                this.transport.update(cx, |transport, cx| {
                    transport.begin_scrub(ScrubSource::Preview, cx)
                });
            }
        });
        let end_scrub = cx.listener(|this, event: &MouseUpEvent, _, cx| {
            if event.button == MouseButton::Left {
                this.transport.update(cx, |transport, cx| {
                    transport.end_scrub(ScrubSource::Preview, cx)
                });
            }
        });
        div()
            .w_full()
            .h(px(50.))
            .flex_none()
            .flex()
            .items_center()
            .bg(colors.background)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .h_full()
                    .justify_center()
                    .gap_0()
                    .border_t_1()
                    .border_color(colors.border.opacity(0.55))
                    .px_3()
                    .py_1()
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .justify_between()
                            .text_sm()
                            .text_color(colors.muted_foreground)
                            .child(current_label)
                            .child(total_label),
                    )
                    .child(
                        div()
                            .id("preview-seekbar")
                            .w_full()
                            .capture_any_mouse_down(begin_scrub)
                            .capture_any_mouse_up(end_scrub)
                            .child(
                                div()
                                    .relative()
                                    .w_full()
                                    .h(px(20.))
                                    .child(
                                        div()
                                            .absolute()
                                            .left_0()
                                            .right_0()
                                            .top(px(7.))
                                            .h(px(Self::BAR_THICKNESS))
                                            .bg(colors.slider_bar.opacity(0.75)),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .left_0()
                                            .top(px(7.))
                                            .h(px(Self::BAR_THICKNESS))
                                            .w(relative(ratio))
                                            .bg(colors.primary),
                                    )
                                    .child(
                                        Slider::new(&self.seekbar)
                                            .horizontal()
                                            .bg(colors.background.opacity(0.)),
                                    ),
                            ),
                    ),
            )
    }

    fn render_audio_meter(level: f32, background: Hsla, primary: Hsla) -> impl IntoElement {
        let level = if level > 0. {
            ((20. * level.log10() - Self::METER_MIN_DB) / -Self::METER_MIN_DB).clamp(0., 1.)
        } else {
            0.
        };
        div()
            .w(px(Self::BAR_THICKNESS))
            .h_full()
            .relative()
            .bg(background.opacity(0.75))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(relative(level))
                    .bg(primary),
            )
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
        let (frame, aspect_ratio) = {
            let editor = self.editor.read(cx);
            (editor.playhead(), editor.resolution().aspect_ratio())
        };
        let levels = if self.transport.read(cx).is_playing() {
            self.transport.read(cx).audio_levels(cx)
        } else {
            let editor = self.editor.read(cx);
            self.audio_level_sampler
                .levels_at(editor.visible_items(), frame, editor.frame_rate())
        };
        let error = self
            .backend
            .read(cx)
            .error
            .clone()
            .or_else(|| self.error.clone())
            .or_else(|| self.playback_error.clone());

        div()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(colors.background)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .overflow_hidden()
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
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .bottom_0()
                            .w(px(Self::BAR_THICKNESS))
                            .child(Self::render_audio_meter(
                                levels[0],
                                colors.slider_bar,
                                colors.primary,
                            )),
                    )
                    .child(
                        div()
                            .absolute()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .w(px(Self::BAR_THICKNESS))
                            .child(Self::render_audio_meter(
                                levels[1],
                                colors.slider_bar,
                                colors.primary,
                            )),
                    ),
            )
            .child(self.render_controls(window, cx))
    }
}
