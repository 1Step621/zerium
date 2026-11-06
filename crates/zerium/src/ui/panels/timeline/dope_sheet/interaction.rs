use crate::ui::panels::timeline::LAYER_HEIGHT;
use crate::ui::{
    TimelineEditorEntityExt as _,
    panels::timeline::{Timeline, layer_scroll_base},
};
use gpui::{Bounds, Context, Pixels, point, px};
use std::collections::HashSet;
use zerium_core::animation::StopValueSource;
use zerium_core::timeline::{PropertyAddress, TimelineTime};

pub(in crate::ui::panels::timeline) type StopKeys = HashSet<(PropertyAddress, usize)>;

#[derive(Clone)]
pub(in crate::ui::panels::timeline) struct StopSelection {
    pub(in crate::ui::panels::timeline) keys: StopKeys,
    pub(in crate::ui::panels::timeline) context: zerium_core::timeline::EditContext,
}

impl StopSelection {
    fn new(editor: &zerium_core::timeline::TimelineEditor) -> Self {
        Self {
            keys: HashSet::new(),
            context: editor.edit_context(),
        }
    }

    pub(in crate::ui::panels::timeline) fn validate(
        &mut self,
        editor: &zerium_core::timeline::TimelineEditor,
    ) {
        if !self.context.is_current(editor) {
            self.keys.clear();
            self.context = editor.edit_context();
        }
    }
}

impl Timeline {
    pub(in crate::ui::panels::timeline) fn apply_stop_marquee(
        &mut self,
        bounds: Bounds<Pixels>,
        mut selection: StopSelection,
        cx: &mut Context<Self>,
    ) {
        let editor = self.editor.read(cx);
        if !selection.context.is_current(editor) {
            self.stop_selection = Some(StopSelection::new(editor));
            self.marquee_selection = None;
            return;
        }
        let scroll = layer_scroll_base(&self.layer_scroll);
        let track_bounds = self.track_bounds();
        let origin = track_bounds.origin + scroll.offset();
        let bounds = bounds.intersect(&track_bounds);
        for row in self.rows.animations() {
            let address = &row.address;
            let Some(item) = editor.item(address.item_id) else {
                continue;
            };
            let Some(track) = editor.animation_track(address) else {
                continue;
            };
            let clock = item.animation_clock(track);
            for (index, stop) in track
                .stops()
                .iter()
                .enumerate()
                .skip(1)
                .take(track.stops().len().saturating_sub(2))
            {
                let position = origin
                    + point(
                        px(self.viewport.x_at_seconds(
                            clock.key_time_at(stop.frame()).seconds(editor.frame_rate()),
                        )),
                        px((row.index as f32 + 0.5) * LAYER_HEIGHT),
                    );
                if bounds.contains(&position) {
                    selection.keys.insert((address.clone(), index));
                }
            }
        }
        self.stop_selection = Some(selection);
    }

    pub(in crate::ui::panels::timeline) fn add_animation_stop(
        &mut self,
        address: &PropertyAddress,
        time: TimelineTime,
        source: StopValueSource,
        cx: &mut Context<Self>,
    ) {
        let stop = self.editor.update(cx, |editor, cx| {
            let stop = editor.insert_animation_stop_at_time(address, time, source);
            if stop.is_some() {
                cx.notify();
            }
            stop
        });
        if let Some(stop) = stop {
            self.select_stop(address.clone(), stop, false, cx);
            self.transport.update(cx, |transport, cx| {
                transport.set_playhead(time.nearest_frame(), cx)
            });
        }
    }

    pub(super) fn begin_stop_interaction(
        &mut self,
        address: PropertyAddress,
        stop: usize,
        event: &gpui::MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        self.edit_stops(cx);
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        if !event.modifiers.shift && !self.edit_stops(cx).keys.contains(&(address.clone(), stop)) {
            self.select_stop(address, stop, false, cx);
        }
    }

    pub(in crate::ui::panels::timeline) fn select_stop(
        &mut self,
        address: PropertyAddress,
        stop: usize,
        additive: bool,
        cx: &mut Context<Self>,
    ) {
        let selection = self.edit_stops(cx);
        let key = (address, stop);
        if additive {
            if !selection.keys.remove(&key) {
                selection.keys.insert(key);
            }
        } else {
            selection.keys.clear();
            selection.keys.insert(key);
        }
        cx.notify();
    }

    pub(in crate::ui::panels::timeline) fn open_animation_curve(
        &mut self,
        address: PropertyAddress,
        segment: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        self.edit_stops(cx);
        if let Some(segment) = segment {
            self.animation_selection.update(cx, |selection, cx| {
                selection.focus_segment(address, segment, &self.editor, cx);
            });
        } else {
            self.seek_to_item_if_outside(address.item_id, cx);
            self.animation_selection
                .update(cx, |selection, cx| selection.select(address, cx));
        }
    }

    pub(in crate::ui::panels::timeline) fn delete_selected_stops(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let Some(selection) = &self.stop_selection else {
            return;
        };
        if selection.context.is_current(self.editor.read(cx)) {
            let selected = selection.keys.iter().cloned().collect::<Vec<_>>();
            self.editor
                .update_if_changed(cx, |editor| editor.remove_animation_stops(&selected));
        }
        self.stop_selection = Some(StopSelection::new(self.editor.read(cx)));
        cx.notify();
    }

    pub(in crate::ui::panels::timeline) fn edit_stops(
        &mut self,
        cx: &Context<Self>,
    ) -> &mut StopSelection {
        let editor = self.editor.read(cx);
        let selection = self
            .stop_selection
            .get_or_insert_with(|| StopSelection::new(editor));
        selection.validate(editor);
        selection
    }
}
