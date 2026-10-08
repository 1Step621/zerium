//! Builds and presents preview frames, including asynchronous media demand.
use std::collections::HashMap;

use gpui::{Context, SharedString};
use rust_i18n::t;
use zerium_core::timeline::TimelineTime;

use super::{Preview, RenderedFrame};
use crate::engine::{
    media::{MediaInputId, VideoDecodeSize},
    rendering::{RenderError, RenderQuality, RenderScene, RenderSize},
    video_playback::{RequestedVideoFrame, VideoPlaybackMode, VideoPlaybackSnapshot},
};

impl Preview {
    const REALTIME_TEMPORAL_SAMPLES: usize = 4;

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
        let mut recorded_by_time: HashMap<u64, HashMap<MediaInputId, RequestedVideoFrame>> =
            HashMap::new();
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
                let input = MediaInputId {
                    item_id: request.item_id,
                    effect_id: request.effect_id,
                    input_id: request.input_id.to_owned(),
                };
                let recorded = recorded_by_time.entry(time_bits).or_insert_with(|| {
                    let items = editor.active_items_at_time(request.time);
                    playback
                        .record_media_requests(
                            request.time,
                            &items,
                            editor.media_cache(),
                            frame_rate,
                            |input| {
                                let target = items
                                    .iter()
                                    .find(|(_, item)| item.id == input.item_id)
                                    .and_then(|(_, item)| {
                                        RenderScene::render_size_for_item(item, size).ok().map(
                                            |target| {
                                                RenderScene::media_raster_size_for_input(
                                                    item,
                                                    input.effect_id,
                                                    &input.input_id,
                                                    target,
                                                    composition_size,
                                                )
                                            },
                                        )
                                    })
                                    .unwrap_or(request.target_size);
                                VideoDecodeSize {
                                    max_width: target.width,
                                    max_height: target.height,
                                }
                            },
                        )
                        .into_iter()
                        .collect()
                });
                Ok::<_, RenderError>(
                    recorded
                        .get(&input)
                        .and_then(|requested| playback.present_recorded_frame(&input, requested)),
                )
            },
            |request| text_frames.frame_for(request, composition_size),
        )?;
        let snapshot = playback.finish_frame_demand();
        Ok((scene, snapshot))
    }

    pub(super) fn handle_playback_snapshot(
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

    pub(super) fn render_latest_frame(&mut self, cx: &mut Context<Self>) {
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
            .retain_active(active_items.iter().map(|(_, item)| item));
        let (scene, playback) = match self.prepare_scene(render_time, size, cx) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.report_error(
                    t!("preview.build_scene_failed", error = error).to_string(),
                    cx,
                );
                return;
            }
        };
        self.handle_playback_snapshot(&playback, cx);

        let Some(renderer) = self.render_runtime.read(cx).renderer() else {
            return;
        };
        if self.rendered_frame
            == Some(RenderedFrame {
                editor_revision: revision,
                video_revision: playback.revision,
                size,
            })
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
                        t!("preview.rebuild_scene_failed", error = error).to_string(),
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
                self.rendered_frame = Some(RenderedFrame {
                    editor_revision: revision,
                    video_revision: playback.revision,
                    size,
                });
                self.error = None;
            }
            Err(error) => {
                self.report_error(t!("preview.render_failed", error = error).to_string(), cx);
            }
        }
    }

    pub(super) fn report_error(&mut self, message: String, cx: &mut Context<Self>) {
        let message = SharedString::from(message);
        if self.error.as_ref() == Some(&message) {
            return;
        }
        self.error = Some(message.clone());
        self.notifications
            .update(cx, |notifications, cx| notifications.push(message, cx));
    }
}
