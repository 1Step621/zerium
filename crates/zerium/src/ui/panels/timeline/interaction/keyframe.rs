use gpui::{Context, Empty, EntityId, MouseButton, Render, Window, prelude::*};

use zerium_core::timeline::{AnimationEdit, Frame, PropertyAddress, TimelineEditor, TimelineTime};

use crate::ui::{TimelineEditorEntityExt as _, components::time_grid, panels::timeline::Timeline};

#[derive(Clone, PartialEq, Eq)]
pub(in crate::ui::panels::timeline) struct MoveAnimationStop {
    pub(in crate::ui::panels::timeline) timeline_id: EntityId,
    pub(in crate::ui::panels::timeline) address: PropertyAddress,
    pub(in crate::ui::panels::timeline) stop: usize,
}

/// The timeline owns the edit from press through release, independently of
/// marker redraws. The GPUI drag payload only identifies this source.
pub(in crate::ui::panels::timeline) struct AnimationStopDrag {
    source: MoveAnimationStop,
    edit: AnimationEdit,
    snap_playhead: Frame,
}

impl AnimationStopDrag {
    fn new(
        source: MoveAnimationStop,
        selected: &[(PropertyAddress, usize)],
        editor: &TimelineEditor,
    ) -> Option<Self> {
        let edit = editor.begin_animation_stop_edit(selected)?;
        let snap_playhead = editor.playhead();
        Some(Self {
            source,
            edit,
            snap_playhead,
        })
    }
}

impl Render for MoveAnimationStop {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl Timeline {
    pub(in crate::ui::panels::timeline) fn begin_stop_drag(
        &mut self,
        source: MoveAnimationStop,
        cx: &mut Context<Self>,
    ) {
        let anchor = (source.address.clone(), source.stop);
        let selection = self.edit_stops(cx);
        selection.keys.insert(anchor.clone());
        let mut selected = vec![anchor.clone()];
        selected.extend(selection.keys.iter().filter(|key| **key != anchor).cloned());
        self.animation_drag = AnimationStopDrag::new(source, &selected, self.editor.read(cx));
    }

    pub(in crate::ui::panels::timeline) fn move_animation_stop_from_pointer(
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
        if let Some(selection) = &mut self.stop_selection {
            selection.context = self.editor.read(cx).edit_context();
        }
    }
}
