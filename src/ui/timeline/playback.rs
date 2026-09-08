use super::*;

impl Timeline {
    pub(super) fn stop_playback(&mut self, cx: &mut Context<Self>) -> bool {
        self.transport
            .update(cx, |transport, cx| transport.stop(cx))
    }

    fn step_frame(&mut self, delta: i64, cx: &mut Context<Self>) {
        self.transport
            .update(cx, |transport, cx| transport.step(delta, cx));
    }

    pub(super) fn previous_frame(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_frame(-1, cx);
    }

    pub(super) fn next_frame(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.step_frame(1, cx);
    }

    pub(crate) fn toggle_playback(&mut self, cx: &mut Context<Self>) {
        self.transport
            .update(cx, |transport, cx| transport.toggle_playback(cx));
    }

    pub(crate) fn undo(&mut self, cx: &mut Context<Self>) {
        self.stop_playback(cx);
        self.finish_playhead_scrub(cx);
        self.item_move_origin = None;
        self.editor.update_if_changed(cx, TimelineEditor::undo);
    }

    pub(crate) fn can_undo(&self, cx: &App) -> bool {
        self.editor.read(cx).can_undo()
    }

    pub(crate) fn redo(&mut self, cx: &mut Context<Self>) {
        self.stop_playback(cx);
        self.finish_playhead_scrub(cx);
        self.item_move_origin = None;
        self.editor.update_if_changed(cx, TimelineEditor::redo);
    }

    pub(crate) fn can_redo(&self, cx: &App) -> bool {
        self.editor.read(cx).can_redo()
    }

    pub(super) fn toggle_playback_button(
        &mut self,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_playback(cx);
    }
}
