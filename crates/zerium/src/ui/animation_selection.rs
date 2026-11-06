//! Animation focus shared by the inspector, timeline, and curve editor.
use gpui::{Context, Entity, Subscription};
use std::cell::Cell;
use zerium_core::timeline::{PropertyAddress, TimelineEditor, TimelineTime};

pub(crate) struct AnimationSelection {
    address: Option<PropertyAddress>,
    // The focused interval follows the playhead while either view is rendered,
    // so it intentionally does not trigger a second render notification.
    focused_segment: Cell<Option<usize>>,
    _editor_subscription: Subscription,
}

impl AnimationSelection {
    pub(crate) fn new(editor: Entity<TimelineEditor>, cx: &mut Context<Self>) -> Self {
        let mut document = editor.read(cx).active_scene_id();
        let subscription = cx.observe(&editor, move |this, editor, cx| {
            let editor = editor.read(cx);
            let next = editor.active_scene_id();
            if document != next {
                document = next;
                this.clear(cx);
            } else if let Some(address) = this.address.clone() {
                if editor.is_item_selected(address.item_id)
                    && editor.animation_track(&address).is_some()
                {
                    return;
                }
                let target = editor
                    .selected_item_ids()
                    .find_map(|id| Self::matching_track(&address, id, editor));
                match target {
                    Some(target) => this.select(target, cx),
                    None => this.clear(cx),
                }
            }
        });
        Self {
            address: None,
            focused_segment: Cell::new(None),
            _editor_subscription: subscription,
        }
    }

    fn matching_track(
        address: &PropertyAddress,
        item_id: zerium_core::timeline::ItemId,
        editor: &TimelineEditor,
    ) -> Option<PropertyAddress> {
        let schema = editor.property_schema(address)?;
        let target = editor.corresponding_property_address(address, item_id)?;
        (editor.property_schema(&target)?.same_type(schema)
            && editor.animation_track(&target).is_some())
        .then_some(target)
    }

    pub(crate) fn select_item(
        &mut self,
        item_id: zerium_core::timeline::ItemId,
        editor: &Entity<TimelineEditor>,
        cx: &mut Context<Self>,
    ) {
        let target = self
            .address
            .as_ref()
            .and_then(|address| Self::matching_track(address, item_id, editor.read(cx)));
        match target {
            Some(target) => self.select(target, cx),
            None => self.clear(cx),
        }
    }

    pub(crate) fn address(&self) -> Option<&PropertyAddress> {
        self.address.as_ref()
    }

    pub(crate) fn focused_segment(&self, editor: &TimelineEditor) -> Option<usize> {
        let address = self.address()?;
        let item = editor.item(address.item_id)?;
        let track = editor.animation_track(address)?;
        let frame = item
            .animation_clock(track)
            .local_frame_at(TimelineTime::from_frame(editor.playhead()));
        if !frame.is_finite() {
            return None;
        }
        let stops = track.stops();
        let last_segment = stops.len().checked_sub(2)?;
        if let Some(segment) = self
            .focused_segment
            .get()
            .filter(|segment| *segment <= last_segment)
        {
            let start = stops[segment].frame().get() as f64;
            let end = stops[segment + 1].frame().get() as f64;
            // Shared endpoints keep the clicked or previously focused interval.
            if frame >= start && frame <= end {
                return Some(segment);
            }
        }
        let segment = stops
            .partition_point(|stop| (stop.frame().get() as f64) < frame)
            .saturating_sub(1)
            .min(last_segment);
        self.focused_segment.set(Some(segment));
        Some(segment)
    }

    pub(crate) fn focus_segment(
        &mut self,
        address: PropertyAddress,
        segment: usize,
        editor: &Entity<TimelineEditor>,
        cx: &mut Context<Self>,
    ) {
        let valid = {
            let editor = editor.read(cx);
            editor.is_item_selected(address.item_id)
                && editor
                    .animation_track(&address)
                    .is_some_and(|track| track.interpolations().get(segment).is_some())
        };
        if !valid {
            return;
        }
        editor.update(cx, |editor, _| editor.finish_history_group());
        self.select(address, cx);
        self.focused_segment.set(Some(segment));
        cx.notify();
    }

    pub(crate) fn select(&mut self, address: PropertyAddress, cx: &mut Context<Self>) {
        if self.address.as_ref() == Some(&address) {
            return;
        }
        self.address = Some(address);
        self.focused_segment.set(None);
        cx.notify();
    }

    pub(crate) fn clear_if(&mut self, address: &PropertyAddress, cx: &mut Context<Self>) {
        if self.address.as_ref() == Some(address) {
            self.address = None;
            self.focused_segment.set(None);
            cx.notify();
        }
    }

    pub(crate) fn clear(&mut self, cx: &mut Context<Self>) {
        if self.address.take().is_some() {
            self.focused_segment.set(None);
            cx.notify();
        }
    }
}
