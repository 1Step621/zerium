use gpui::{Context, Empty, EntityId, MouseButton, Render, Window, prelude::*};

use zerium_core::timeline::{
    AnimationEdit, AnimationEditTarget, Frame, PropertyAddress, TimelineEditor, TimelineTime,
};

use crate::ui::TimelineEditorEntityExt as _;
use crate::ui::time_grid;

use crate::ui::timeline::Timeline;

#[derive(Clone, PartialEq, Eq)]
pub(in crate::ui::timeline) struct MoveAnimationStop {
    pub(in crate::ui::timeline) timeline_id: EntityId,
    pub(in crate::ui::timeline) address: PropertyAddress,
    pub(in crate::ui::timeline) stop: usize,
}

/// The timeline owns the edit from press through release, independently of
/// marker redraws. The GPUI drag payload only identifies this source.
pub(in crate::ui::timeline) struct AnimationStopDrag {
    source: MoveAnimationStop,
    edit: AnimationEdit,
    snap_playhead: Frame,
}

impl AnimationStopDrag {
    pub(in crate::ui::timeline) fn new(
        source: MoveAnimationStop,
        editor: &TimelineEditor,
        synchronize: bool,
    ) -> Option<Self> {
        let edit = editor.begin_animation_edit(
            &source.address,
            AnimationEditTarget::Stop(source.stop),
            synchronize,
        )?;
        let snap_playhead = editor.playhead();
        Some(Self {
            source,
            edit,
            snap_playhead,
        })
    }

    pub(in crate::ui::timeline) fn current_edit(
        &self,
        editor: &TimelineEditor,
    ) -> Option<(&MoveAnimationStop, &AnimationEdit)> {
        self.edit
            .is_current(editor)
            .then_some((&self.source, &self.edit))
    }
}

impl Render for MoveAnimationStop {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl Timeline {
    pub(in crate::ui::timeline) fn add_animation_stop_at_time(
        &mut self,
        address: &PropertyAddress,
        time: TimelineTime,
        cx: &mut Context<Self>,
    ) {
        let changed = self.editor.update_if_changed(cx, |editor| {
            editor
                .insert_animation_stop_at_time(address, time)
                .is_some()
        });
        if changed {
            self.transport.update(cx, |transport, cx| {
                transport.set_playhead(time.nearest_frame(), cx)
            });
        }
    }

    pub(in crate::ui::timeline) fn move_animation_stop_from_pointer(
        &mut self,
        drag: &MoveAnimationStop,
        pointer_x: f32,
        snap_disabled: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if drag.timeline_id != cx.entity_id() {
            return;
        }
        if window.pressed_mouse_button() != Some(MouseButton::Left) {
            self.animation_drag = None;
            cx.notify();
            return;
        }

        let mut frame = self.pointer_frame(pointer_x, window, cx);
        let Some(state) = self
            .animation_drag
            .as_mut()
            .filter(|state| state.source == *drag && state.edit.is_current(self.editor.read(cx)))
        else {
            return;
        };
        let edit = &mut state.edit;
        let Some(range) = edit.time_range() else {
            return;
        };
        frame = Frame::new(frame.get().clamp(
            range.start().nearest_frame().get(),
            range.end().nearest_frame().get(),
        ));
        if !snap_disabled {
            let editor = self.editor.read(cx);
            let frame_rate = editor.frame_rate();
            let seconds = frame_rate.frame_to_seconds(frame);
            let offset = time_grid::snap_offset_seconds(
                &[seconds],
                editor,
                state.snap_playhead,
                [],
                self.viewport.pixels_per_second(),
            );
            frame = frame_rate.seconds_to_frame(seconds + offset);
        }
        self.editor.update_if_changed(cx, |editor| {
            editor.move_animation_stop(edit, TimelineTime::from_frame(frame))
        });
    }
}
