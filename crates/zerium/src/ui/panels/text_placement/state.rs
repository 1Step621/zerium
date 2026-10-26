use std::collections::HashSet;

use super::*;

impl TextPlacement {
    pub(super) fn set_target(&mut self, target: Target, cx: &mut Context<Self>) {
        if self.target.as_ref() == Some(&target) || target.schema(self.editor.read(cx)).is_none() {
            return;
        }
        self.target = Some(target);
        self.stamp_target = None;
        self.error = None;
        self.scroll.scroll_to_item(0);
        cx.notify();
    }

    pub(crate) fn can_stamp(&self, cx: &App) -> bool {
        matches!(
            self.current_stamp(&self.rows(cx), cx),
            Some(StampTarget::Start(_) | StampTarget::End(_))
        )
    }

    pub(super) fn sync_scope(&mut self, cx: &App) {
        let session_id = self.session.read(cx).id();
        let scene_id = self.editor.read(cx).active_scene_id();
        if self.session_id != session_id || self.scene_id != scene_id {
            self.session_id = session_id;
            self.scene_id = scene_id;
            self.layer = None;
            self.inputs = InputStore::default();
            self.stamp_target = None;
            self.error = None;
            self.target = Target::available(self.editor.read(cx))
                .into_iter()
                .next()
                .map(|(target, _)| target);
        }
    }

    pub(super) fn rows(&self, cx: &App) -> Vec<Row> {
        let (Some(layer), Some(target)) = (self.layer, self.target.as_ref()) else {
            return Vec::new();
        };
        let Some(field) = target.schema(self.editor.read(cx)) else {
            return Vec::new();
        };
        let editor = self.editor.read(cx);
        let mut rows = editor
            .items_on_layer(layer)
            .into_iter()
            .filter(|item| target.matches(item))
            .filter_map(|item| {
                let address = PropertyAddress {
                    item_id: item.id,
                    effect_id: None,
                    property_id: field.id().to_owned(),
                    element_id: None,
                    scalar_index: None,
                };
                let schema = editor.property_schema(&address)?;
                let PropertyValue::String(text) = editor.property_value(&address)? else {
                    return None;
                };
                let bound = editor
                    .active_scene()
                    .variables()
                    .iter()
                    .flat_map(|variable| &variable.bindings)
                    .any(|binding| {
                        binding.item_id() == item.id
                            && binding.owner().effect_id().is_none()
                            && binding.property_id() == field.id()
                    });
                Some(Row {
                    address,
                    start: item.start(),
                    end: item.end_exclusive(editor.frame_rate()),
                    text,
                    editable: schema.is_editable(None) && !bound,
                })
            })
            .collect::<Vec<_>>();
        rows.sort_by_key(|row| (row.start, row.address.item_id.get()));
        rows
    }

    pub(super) fn sync_inputs(
        &mut self,
        rows: &[Row],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.inputs.retain(
            &rows
                .iter()
                .map(|row| row.address.clone())
                .collect::<HashSet<_>>(),
        );
        let (session_id, scene_id) = (self.session_id, self.scene_id);
        for row in rows {
            let address = row.address.clone();
            self.inputs.ensure_text(
                address.clone(),
                row.text.clone(),
                TextOptions {
                    sync: TextSync::PreserveDraft,
                    ..Default::default()
                },
                |input, window, cx| {
                    vec![
                        cx.subscribe_in(input, window, move |this, input, event, window, cx| {
                            if this.session.read(cx).id() != session_id
                                || this.editor.read(cx).active_scene_id() != scene_id
                            {
                                return;
                            }
                            match event {
                                InputEvent::Focus => this.select_row(address.item_id, cx),
                                InputEvent::Change => {
                                    this.inputs.set_draft(&address, true);
                                    if this.write_text(
                                        &address,
                                        input.read(cx).value().to_string(),
                                        cx,
                                    ) {
                                        this.inputs.set_draft(&address, false);
                                    }
                                }
                                InputEvent::Blur | InputEvent::PressEnter { .. } => {
                                    this.inputs.set_draft(&address, false);
                                    if let Some(row) =
                                        this.rows(cx).into_iter().find(|row| row.address == address)
                                    {
                                        crate::ui::components::input::set_input_text(
                                            input, row.text, window, cx,
                                        );
                                    }
                                    cx.notify();
                                }
                                _ => {}
                            }
                        }),
                    ]
                },
                window,
                cx,
            );
        }
    }

    pub(super) fn select_row(&mut self, id: ItemId, cx: &mut Context<Self>) {
        let Some(row) = self
            .rows(cx)
            .into_iter()
            .find(|row| row.address.item_id == id)
        else {
            return;
        };
        self.stamp_target = Some(StampTarget::Start(id));
        self.editor.update(cx, |editor, cx| {
            if editor.select(id) {
                cx.notify();
            }
        });
        self.transport
            .update(cx, |transport, cx| transport.set_playhead(row.start, cx));
        cx.notify();
    }

    pub(super) fn current_stamp(&self, rows: &[Row], cx: &App) -> Option<StampTarget> {
        let selected = rows
            .iter()
            .find(|row| self.editor.read(cx).is_item_selected(row.address.item_id))
            .or_else(|| rows.first())?;
        match self.stamp_target {
            Some(target) if target.item() == selected.address.item_id => Some(target),
            _ => Some(StampTarget::Start(selected.address.item_id)),
        }
    }
}
