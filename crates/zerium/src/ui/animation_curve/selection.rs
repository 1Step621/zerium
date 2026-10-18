use super::*;

pub(crate) struct AnimationSelection {
    address: Option<PropertyAddress>,
    // The focused interval follows the playhead while the curve is queried,
    // so it intentionally does not trigger a second render notification.
    focused_segment: Cell<Option<usize>>,
    _editor_subscription: Subscription,
}

impl AnimationSelection {
    pub(crate) fn new(editor: Entity<TimelineEditor>, cx: &mut Context<Self>) -> Self {
        let mut document = (
            editor.read(cx).snapshot().project_id(),
            editor.read(cx).active_scene_id(),
        );
        let subscription = cx.observe(&editor, move |this, editor, cx| {
            let editor = editor.read(cx);
            let next = (editor.snapshot().project_id(), editor.active_scene_id());
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

    pub(crate) fn focused_segment(&self) -> Option<usize> {
        self.focused_segment.get()
    }

    pub(crate) fn focus_segment(&self, segment: usize) {
        self.focused_segment.set(Some(segment));
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
