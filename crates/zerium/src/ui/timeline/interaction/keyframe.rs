use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gpui::{Context, Empty, EntityId, Render, Window, prelude::*};

use zerium_core::timeline::{AnimationEdit, Frame, PropertyAddress, TimelineTime};

use crate::ui::time_grid;

use crate::ui::timeline::Timeline;

#[derive(Clone)]
pub(in crate::ui::timeline) struct MoveAnimationStop {
    pub(in crate::ui::timeline) timeline_id: EntityId,
    pub(in crate::ui::timeline) address: PropertyAddress,
    pub(in crate::ui::timeline) edit: Rc<RefCell<Option<AnimationEdit>>>,
    pub(in crate::ui::timeline) snap_playhead: Cell<Frame>,
    pub(in crate::ui::timeline) follow_focus: Option<usize>,
}

impl Render for MoveAnimationStop {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl Timeline {
    pub(in crate::ui::timeline) fn move_animation_stop_from_pointer(
        &mut self,
        drag: &MoveAnimationStop,
        pointer_x: f32,
        snap_disabled: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if drag.timeline_id != cx.entity_id()
            || self.animation_selection.read(cx).address() != Some(&drag.address)
        {
            return;
        }

        let mut frame = self.pointer_frame(pointer_x, window, cx);
        let mut edit = drag.edit.borrow_mut();
        let Some(edit) = edit.as_mut() else {
            return;
        };
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
                drag.snap_playhead.get(),
                [],
                self.viewport.pixels_per_second(),
            );
            frame = frame_rate.seconds_to_frame(seconds + offset);
        }
        let changed = self.editor.update(cx, |editor, cx| {
            let changed = editor
                .move_animation_stop(edit, TimelineTime::from_frame(frame))
                .is_some();
            if changed {
                cx.notify();
            }
            changed
        });
        if changed && let Some(segment) = drag.follow_focus {
            self.animation_selection.read(cx).focus_segment(segment);
            self.transport
                .update(cx, |transport, cx| transport.set_playhead(frame, cx));
        }
    }
}
