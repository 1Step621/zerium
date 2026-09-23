use std::{collections::HashMap, sync::Arc};

use ::ui::{
    ActiveTheme as _,
    slider::{Slider, SliderEvent, SliderState},
};
use futures::StreamExt as _;
use gpui::{
    Context, DragMoveEvent, Entity, Hsla, MouseButton, MouseDownEvent, MouseUpEvent, Render,
    SharedString, Subscription, Task, WgpuSurfaceHandle, Window, div, prelude::*, px, relative,
    wgpu_surface,
};

mod editor_overlay;

use editor_overlay::{PreviewEditorDrag, PreviewEditorDragState};

use crate::{
    app::project_session::{ProjectSession, ProjectSessionId},
    domain::timeline::{Frame, ItemId, LayerId, TimelineEditor, TimelineItem, TimelineTime},
    engine::{
        audio_meter::AudioLevelSampler,
        media::{MediaReaderRegistry, VideoDecodeSize},
        rendering::{
            CompiledPluginShaders, FrameRenderer, RenderError, RenderQuality, RenderRuntime,
            RenderScene, RenderSize, TextFrameCache,
        },
        video_playback::{
            RequestedVideoFrame, VideoInputId, VideoPlaybackEngine, VideoPlaybackMode,
            VideoPlaybackSnapshot,
        },
    },
    ui::{
        session::UiNotifications,
        time_grid,
        transport::{ScrubSource, TransportController},
    },
};

pub(crate) struct PreviewDependencies {
    editor: Entity<TimelineEditor>,
    transport: Entity<TransportController>,
    session: Entity<ProjectSession>,
    notifications: Entity<UiNotifications>,
    render_runtime: Entity<RenderRuntime>,
    plugin_shaders: Arc<CompiledPluginShaders>,
    media_readers: Arc<MediaReaderRegistry>,
}

impl PreviewDependencies {
    pub(crate) fn new(
        editor: Entity<TimelineEditor>,
        transport: Entity<TransportController>,
        session: Entity<ProjectSession>,
        notifications: Entity<UiNotifications>,
        render_runtime: Entity<RenderRuntime>,
        plugin_shaders: Arc<CompiledPluginShaders>,
        media_readers: Arc<MediaReaderRegistry>,
    ) -> Self {
        Self {
            editor,
            transport,
            session,
            notifications,
            render_runtime,
            plugin_shaders,
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
    video_playback: VideoPlaybackEngine,
    audio_level_sampler: AudioLevelSampler,
    seekbar: Entity<SliderState>,
    surface: Option<WgpuSurfaceHandle>,
    render_runtime: Entity<RenderRuntime>,
    text_frames: TextFrameCache,
    error: Option<SharedString>,
    playback_error: Option<SharedString>,
    rendered_revision: Option<u64>,
    rendered_video_revision: Option<u64>,
    rendered_size: Option<RenderSize>,
    editor_drag: PreviewEditorDragState,
    _editor_subscription: Subscription,
    _transport_subscription: Subscription,
    _session_subscription: Subscription,
    _seekbar_subscription: Subscription,
    _video_playback_task: Task<()>,
}

impl Preview {
    const METER_MIN_DB: f32 = -60.;
    const INITIAL_SIZE: RenderSize = RenderSize {
        width: 640,
        height: 360,
    };
    const REALTIME_TEMPORAL_SAMPLES: usize = 4;
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
            render_runtime,
            plugin_shaders,
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
            this.editor_drag.clear();
            this.audio_level_sampler.clear();
            this.video_playback.reset();
            cx.notify();
        });
        let surface = window.create_wgpu_surface(
            Self::INITIAL_SIZE.width,
            Self::INITIAL_SIZE.height,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        );
        let (renderer, error): (Option<Arc<FrameRenderer>>, Option<SharedString>) = match &surface {
            Some(surface) => match RenderRuntime::preview_renderer(
                Arc::new(surface.device().clone()),
                Arc::new(surface.queue().clone()),
                &plugin_shaders,
            ) {
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
        render_runtime.update(cx, |runtime, _| {
            runtime.set_preview_renderer(renderer, error.clone().map(|error| error.to_string()));
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
            rendered_revision: None,
            rendered_video_revision: None,
            rendered_size: None,
            editor_drag: PreviewEditorDragState::default(),
            _editor_subscription: editor_subscription,
            _transport_subscription: transport_subscription,
            _session_subscription: session_subscription,
            _seekbar_subscription: seekbar_subscription,
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

    fn prepare_scene(
        &mut self,
        render_time: TimelineTime,
        size: RenderSize,
        cx: &mut Context<Self>,
    ) -> Result<(RenderScene, VideoPlaybackSnapshot), RenderError> {
        let (frame_rate, mode, resolution) = {
            let editor = self.editor.read(cx);
            (
                editor.frame_rate(),
                self.transport.read(cx).playback_mode(),
                editor.resolution(),
            )
        };
        let composition_size = RenderSize::from(resolution);
        self.video_playback.begin_frame_demand(mode);
        let mut items_by_time: HashMap<u64, Vec<(LayerId, TimelineItem)>> = HashMap::new();
        let mut decode_sizes_by_time: HashMap<u64, HashMap<ItemId, VideoDecodeSize>> =
            HashMap::new();
        let mut recorded: HashMap<(u64, VideoInputId), RequestedVideoFrame> = HashMap::new();
        let editor = self.editor.clone();
        let editor = editor.read(cx);
        let playback = &mut self.video_playback;
        let text_frames = &mut self.text_frames;
        let scene = RenderScene::from_timeline(
            editor,
            render_time,
            size,
            match mode {
                VideoPlaybackMode::Idle => RenderQuality::Full,
                VideoPlaybackMode::Playing | VideoPlaybackMode::Scrubbing => {
                    RenderQuality::Realtime {
                        max_temporal_samples: Self::REALTIME_TEMPORAL_SAMPLES,
                    }
                }
            },
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
                    let decode_sizes = decode_sizes_by_time.entry(time_bits).or_insert_with(|| {
                        items
                            .iter()
                            .filter_map(|(_, item)| {
                                RenderScene::render_size_for_item(item, size)
                                    .ok()
                                    .map(|size| {
                                        (
                                            item.id,
                                            VideoDecodeSize {
                                                max_width: size.width,
                                                max_height: size.height,
                                            },
                                        )
                                    })
                            })
                            .collect()
                    });
                    for (input, requested) in
                        playback.record_media_requests(request.time, items, frame_rate, |item_id| {
                            decode_sizes
                                .get(&item_id)
                                .copied()
                                .unwrap_or(VideoDecodeSize {
                                    max_width: request.target_size.width,
                                    max_height: request.target_size.height,
                                })
                        })
                    {
                        recorded.insert((time_bits, input), requested);
                    }
                }
                Ok(recorded
                    .get(&(time_bits, input.clone()))
                    .and_then(|requested| playback.present_recorded_frame(&input, requested)))
            },
            |item, schema, size| text_frames.frame_for(item, schema, size, composition_size),
        )?;
        let snapshot = playback.finish_frame_demand();
        Ok((scene, snapshot))
    }

    fn handle_playback_snapshot(
        &mut self,
        playback: &VideoPlaybackSnapshot,
        cx: &mut Context<Self>,
    ) {
        self.playback_error = playback.error.clone().map(Into::into);
        for message in &playback.notifications {
            self.notifications.update(cx, |notifications, cx| {
                notifications.push(message.clone(), cx);
            });
        }
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
        self.handle_playback_snapshot(&playback, cx);

        let Some(renderer) = self.render_runtime.read(cx).renderer() else {
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
                Ok(prepared) => {
                    self.handle_playback_snapshot(&prepared.1, cx);
                    prepared
                }
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
        let (frame, resolution) = {
            let editor = self.editor.read(cx);
            (editor.playhead(), editor.resolution())
        };
        let aspect_ratio = resolution.aspect_ratio();
        let levels = if self.transport.read(cx).is_playing() {
            self.transport.read(cx).audio_levels(cx)
        } else {
            let editor = self.editor.read(cx);
            self.audio_level_sampler
                .levels_at(editor.visible_items(), frame, editor.frame_rate())
        };
        let error = self
            .render_runtime
            .read(cx)
            .error()
            .map(SharedString::from)
            .or_else(|| self.error.clone())
            .or_else(|| self.playback_error.clone());
        let overlay = self.selected_editor_overlay(frame, cx);
        let composition_units_per_pixel = surface.as_ref().map_or(0., |surface| {
            let logical_width = surface.size().0 as f32 / window.scale_factor();
            resolution.width() as f32 / logical_width.max(1.)
        });

        div()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(colors.background)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .overflow_hidden()
                    .child(Self::render_audio_meter(
                        levels[0],
                        colors.slider_bar,
                        colors.primary,
                    ))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .overflow_hidden()
                            .when_some(surface, |this, surface| {
                                this.child(
                                    div()
                                        .relative()
                                        .w_full()
                                        .max_h_full()
                                        .aspect_ratio(aspect_ratio)
                                        .on_drag_move(cx.listener(
                                            |this,
                                             event: &DragMoveEvent<PreviewEditorDrag>,
                                             window,
                                             cx| {
                                                let drag = event.drag(cx).clone();
                                                this.move_editor_control_from_pointer(
                                                    &drag,
                                                    [
                                                        f32::from(event.event.position.x),
                                                        f32::from(event.event.position.y),
                                                    ],
                                                    window,
                                                    cx,
                                                );
                                            },
                                        ))
                                        .capture_any_mouse_up(cx.listener(
                                            |this, event: &MouseUpEvent, _, cx| {
                                                if event.button == MouseButton::Left {
                                                    let changed = this.editor_drag.clear();
                                                    if changed {
                                                        cx.notify();
                                                    }
                                                }
                                            },
                                        ))
                                        .child(
                                            wgpu_surface(surface)
                                                .absolute()
                                                .inset_0()
                                                .size_full()
                                                .defer_resize_until_mouse_up(true),
                                        )
                                        .when_some(overlay, |this, overlay| {
                                            this.child(self.editor_overlay(
                                                overlay,
                                                resolution,
                                                composition_units_per_pixel,
                                                colors.primary,
                                                cx,
                                            ))
                                        }),
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
                            }),
                    )
                    .child(Self::render_audio_meter(
                        levels[1],
                        colors.slider_bar,
                        colors.primary,
                    )),
            )
            .child(self.render_controls(window, cx))
    }
}
