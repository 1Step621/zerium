//! Builds and presents preview frames, including asynchronous media demand.
use gpui::{Context, SharedString};
use rust_i18n::t;
use zerium_core::timeline::TimelineTime;

use super::{Preview, RenderedFrame};
use crate::engine::{
    rendering::{RenderError, RenderScene, RenderSize},
    video_playback::VideoPlaybackSnapshot,
};

impl Preview {
    fn prepare_scene(
        &mut self,
        render_time: TimelineTime,
        size: RenderSize,
        cx: &mut Context<Self>,
    ) -> Result<(RenderScene, VideoPlaybackSnapshot), RenderError> {
        let composition_size = RenderSize::from(self.editor.read(cx).resolution());
        self.video_playback.begin_frame_demand();
        let editor = self.editor.clone();
        let editor = editor.read(cx);
        let playback = &mut self.video_playback;
        let text_frames = &mut self.text_frames;
        let scene = RenderScene::from_timeline(
            editor,
            render_time,
            size,
            |request| Ok::<_, RenderError>(playback.request_frame(&request)),
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
        let (render_time, revision) = {
            let editor = self.editor.read(cx);
            let frame_rate = editor.frame_rate();
            let render_time = editor
                .playback_time_seconds()
                .and_then(|seconds| TimelineTime::from_seconds(seconds, frame_rate))
                .unwrap_or_else(|| TimelineTime::from_frame(editor.playhead()));
            (render_time, editor.render_revision())
        };
        // Decoder events keep demand progressing even while this scene is unchanged.
        if self.error.is_none()
            && self.rendered_frame.is_some_and(|rendered| {
                rendered.editor_revision == revision && rendered.size == size
            })
            && !self.video_playback.needs_redraw()
        {
            return;
        }
        let active_items = self.editor.read(cx).active_items_at_time(render_time);
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
