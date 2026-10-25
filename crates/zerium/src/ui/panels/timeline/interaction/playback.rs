use gpui::{App, ClickEvent, Context, Window};

use zerium_core::timeline::TimelineEditor;

use crate::ui::{TimelineEditorEntityExt as _, panels::timeline::Timeline};

impl Timeline {
    pub(in crate::ui::panels::timeline) fn stop_playback(
        &mut self,
        cx: &mut Context<Self>,
    ) -> bool {
        self.transport
            .update(cx, |transport, cx| transport.stop(cx))
    }

    pub(crate) fn step_frame(&mut self, delta: i64, cx: &mut Context<Self>) {
        self.transport
            .update(cx, |transport, cx| transport.step(delta, cx));
    }

    pub(in crate::ui::panels::timeline) fn previous_frame(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_frame(-1, cx);
    }

    pub(in crate::ui::panels::timeline) fn next_frame(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_frame(1, cx);
    }

    pub(crate) fn toggle_playback(&mut self, cx: &mut Context<Self>) {
        self.transport
            .update(cx, |transport, cx| transport.toggle_playback(cx));
    }

    pub(crate) fn toggle_playback_in_place(&mut self, cx: &mut Context<Self>) {
        self.transport
            .update(cx, |transport, cx| transport.toggle_playback_in_place(cx));
    }

    pub(crate) fn undo(&mut self, cx: &mut Context<Self>) {
        self.stop_playback(cx);
        self.finish_playhead_scrub(cx);
        self.item_drag = None;
        self.editor.update_if_changed(cx, TimelineEditor::undo);
    }

    pub(crate) fn can_undo(&self, cx: &App) -> bool {
        self.editor.read(cx).can_undo()
    }

    pub(crate) fn redo(&mut self, cx: &mut Context<Self>) {
        self.stop_playback(cx);
        self.finish_playhead_scrub(cx);
        self.item_drag = None;
        self.editor.update_if_changed(cx, TimelineEditor::redo);
    }

    pub(crate) fn can_redo(&self, cx: &App) -> bool {
        self.editor.read(cx).can_redo()
    }

    pub(in crate::ui::panels::timeline) fn toggle_playback_button(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_playback(cx);
    }
}
