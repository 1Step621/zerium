use super::*;
use zerium_core::timeline::{FrameDuration, ItemInterval, TimelineEditError};

const INITIAL_DURATION_SECONDS: f64 = 3.;

impl TextPlacement {
    pub(super) fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self
            .rows(cx)
            .iter()
            .map(|row| row.address.item_id)
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return;
        }
        self.transport
            .update(cx, |transport, cx| transport.stop_in_place(cx));
        self.editor.update(cx, |editor, cx| {
            if editor.remove_items(ids) {
                cx.notify();
            }
        });
        self.inputs = InputStore::default();
        self.stamp_target = None;
        self.error = None;
        self.scroll.scroll_to_item(0);
        self.focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn load_text(
        &mut self,
        text: &str,
        target: &Target,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let lines = text
            .trim_start_matches('\u{feff}')
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>();
        if lines.is_empty() {
            return Err(t!("text_placement.empty").to_string());
        }
        let layer = self
            .layer
            .ok_or_else(|| t!("text_placement.choose_layer").to_string())?;
        let editor = self.editor.read(cx);
        let field = target
            .schema(editor)
            .ok_or_else(|| t!("text_placement.target_unavailable").to_string())?;
        let defaults = target
            .initial_properties(editor, cx)
            .ok_or_else(|| t!("text_placement.target_unavailable").to_string())?;
        let duration = editor
            .frame_rate()
            .seconds_to_duration(INITIAL_DURATION_SECONDS);
        let mut start = editor
            .items_on_layer(layer)
            .iter()
            .map(|item| item.end_exclusive(editor.frame_rate()))
            .max()
            .unwrap_or(editor.playhead())
            .max(editor.playhead());
        let mut items = Vec::with_capacity(lines.len());
        for line in lines {
            let mut properties = defaults.clone();
            properties
                .set(&field, PropertyValue::String(line.to_owned()))
                .map_err(|error| error.to_string())?;
            items.push(ItemCreation {
                start,
                duration: Some(duration),
                properties,
            });
            start = Frame::new(
                start
                    .get()
                    .checked_add(duration.get())
                    .ok_or_else(|| TimelineEditError::PlacementUnavailable.to_string())?,
            );
        }
        let ids = self
            .editor
            .update(cx, |editor, cx| {
                let ids = editor.add_items(layer, &target.source, items)?;
                let next = ids.get(1).copied().unwrap_or(ids[0]);
                editor.select(next);
                cx.notify();
                Ok::<_, TimelineEditError>(ids)
            })
            .map_err(|error| error.to_string())?;
        self.stamp_target = Some(if let Some(id) = ids.get(1) {
            StampTarget::Start(*id)
        } else {
            StampTarget::End(ids[0])
        });
        self.error = None;
        self.scroll.scroll_to_item(
            self.rows(cx)
                .iter()
                .position(|row| row.address.item_id == ids[0])
                .unwrap_or(0),
        );
        cx.notify();
        Ok(())
    }

    pub(super) fn write_text(
        &mut self,
        address: &PropertyAddress,
        text: String,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self
            .rows(cx)
            .iter()
            .any(|row| row.address == *address && row.editable)
        {
            return false;
        }
        let value = PropertyValue::String(text);
        if !self
            .editor
            .read(cx)
            .property_schema(address)
            .is_some_and(|schema| schema.accepts_value(&value))
        {
            return false;
        }
        let result = self.editor.update(cx, |editor, cx| {
            let selected = editor.select(address.item_id);
            let result = editor.edit_property(address, value);
            if selected || result.as_ref().is_ok_and(|changed| *changed) {
                cx.notify();
            }
            result
        });
        self.error = result.err().map(|error| error.to_string());
        cx.notify();
        self.error.is_none()
    }

    pub(crate) fn stamp(&mut self, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        let Some(target) = self.current_stamp(&rows, cx) else {
            return;
        };
        let Some(index) = rows
            .iter()
            .position(|row| row.address.item_id == target.item())
        else {
            return;
        };
        let frame = self.editor.read(cx).playhead();
        let result = match target {
            StampTarget::Start(_) => self.stamp_start(&rows, index, frame, cx),
            StampTarget::End(id) => interval(id, rows[index].start, frame).and_then(|update| {
                self.editor
                    .update(cx, |editor, cx| {
                        let result = editor.set_item_intervals(vec![update]);
                        if result.is_ok() {
                            cx.notify();
                        }
                        result
                    })
                    .map(|_| StampTarget::Done(id))
            }),
            StampTarget::Done(_) => return,
        };
        match result {
            Ok(next) => {
                self.stamp_target = Some(next);
                self.editor.update(cx, |editor, cx| {
                    if editor.select(next.item()) {
                        cx.notify();
                    }
                });
                self.scroll.scroll_to_item(
                    rows.iter()
                        .position(|row| row.address.item_id == next.item())
                        .unwrap_or(index),
                );
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
        cx.notify();
    }

    fn stamp_start(
        &self,
        rows: &[Row],
        index: usize,
        frame: Frame,
        cx: &mut Context<Self>,
    ) -> Result<StampTarget, TimelineEditError> {
        let row = &rows[index];
        let mut updates = Vec::new();
        if let Some(previous) = index.checked_sub(1).map(|index| &rows[index])
            && (previous.end == row.start || previous.end > frame)
        {
            updates.push(interval(previous.address.item_id, previous.start, frame)?);
        }
        let mut end = if frame < row.end {
            row.end
        } else {
            Frame::new(
                frame
                    .get()
                    .checked_add(row.end.get() - row.start.get())
                    .ok_or(TimelineEditError::PlacementUnavailable)?,
            )
        };
        updates.push(interval(row.address.item_id, frame, end)?);
        // Keep following captions in order when the current one moves past them.
        for following in &rows[index + 1..] {
            if following.start >= end {
                break;
            }
            let start = end;
            end = Frame::new(
                start
                    .get()
                    .checked_add(following.end.get() - following.start.get())
                    .ok_or(TimelineEditError::PlacementUnavailable)?,
            );
            updates.push(interval(following.address.item_id, start, end)?);
        }
        self.editor.update(cx, |editor, cx| {
            let result = editor.set_item_intervals(updates);
            if result.is_ok() {
                cx.notify();
            }
            result
        })?;
        Ok(rows
            .get(index + 1)
            .map_or(StampTarget::End(row.address.item_id), |next| {
                StampTarget::Start(next.address.item_id)
            }))
    }
}

fn interval(id: ItemId, start: Frame, end: Frame) -> Result<ItemInterval, TimelineEditError> {
    let duration = end
        .get()
        .checked_sub(start.get())
        .and_then(FrameDuration::new)
        .ok_or(TimelineEditError::PlacementUnavailable)?;
    Ok(ItemInterval {
        item_id: id,
        start,
        duration,
    })
}
