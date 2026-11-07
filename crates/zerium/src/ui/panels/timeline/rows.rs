//! Animation rows are inserted into an otherwise implicit sequence of layer rows.
use crate::ui::panels::timeline::LAYER_HEIGHT;
use std::{collections::HashSet, rc::Rc};

use gpui::{Context, point, px};

use super::{
    Timeline,
    interaction::item::{ItemMoveDrag, MoveTimelineItem},
    layer_scroll_base,
};

use zerium_core::timeline::{LayerId, PropertyAddress, TimelineEditor};

use crate::ui::components::animation::animation_property_label;

#[derive(Default)]
pub(super) struct TimelineRows {
    animations: Vec<AnimationRow>,
}

pub(super) struct AnimationRow {
    pub(super) index: usize,
    pub(super) layer: LayerId,
    pub(super) address: PropertyAddress,
    pub(super) label: String,
}

pub(super) enum TimelineRow<'a> {
    Layer(LayerId),
    Property(&'a AnimationRow),
}

impl TimelineRow<'_> {
    pub(super) fn layer(&self) -> LayerId {
        match self {
            Self::Layer(layer) => *layer,
            Self::Property(row) => row.layer,
        }
    }
}

impl TimelineRows {
    fn new(editor: &TimelineEditor, expanded: &HashSet<LayerId>) -> Self {
        let mut items = editor
            .selected_item_ids()
            .filter_map(|id| Some((editor.item_layer(id)?, editor.item(id)?)))
            .filter(|(layer, _)| expanded.contains(layer))
            .collect::<Vec<_>>();
        items.sort_by_key(|(layer, item)| (layer.get(), item.start(), item.id.get()));
        let mut animations = Vec::new();
        for (layer, item) in items {
            for (address, _) in item.animation_tracks() {
                let Some(mut label) = animation_property_label(editor, item, &address) else {
                    continue;
                };
                if let Some(effect) = address
                    .effect_id
                    .and_then(|id| item.effects.iter().find(|effect| effect.id == id))
                {
                    label = format!("{} › {label}", effect.schema().label());
                }
                animations.push(AnimationRow {
                    index: layer.get() as usize + animations.len() + 1,
                    layer,
                    address,
                    label,
                });
            }
        }
        Self { animations }
    }

    pub(super) fn animations(&self) -> impl Iterator<Item = &AnimationRow> {
        self.animations.iter()
    }

    pub(super) fn layer_row(&self, layer: LayerId) -> usize {
        layer.get() as usize
            + self
                .animations
                .partition_point(|row| row.layer.get() < layer.get())
    }

    pub(super) fn row(&self, index: usize) -> TimelineRow<'_> {
        let before = self.animations.partition_point(|row| row.index < index);
        if let Some(row) = self.animations.get(before).filter(|row| row.index == index) {
            TimelineRow::Property(row)
        } else {
            TimelineRow::Layer(LayerId::new((index - before) as u64))
        }
    }

    fn anchored_offset(
        &self,
        next: &Self,
        offset: f32,
        height: f32,
        drag_layer: Option<LayerId>,
    ) -> f32 {
        let index = drag_layer.map_or_else(
            || (-offset / height).floor().max(0.) as usize,
            |layer| self.layer_row(layer),
        );
        let viewport_y = offset + index as f32 * height;
        let (next_index, viewport_y) = match self.row(index) {
            TimelineRow::Layer(layer) => (next.layer_row(layer), viewport_y),
            TimelineRow::Property(row) => {
                if let Some(next_row) = next.animations.iter().find(|r| r.address == row.address) {
                    (next_row.index, viewport_y)
                } else {
                    // A hidden property returns to its parent layer header.
                    (next.layer_row(row.layer), 0.)
                }
            }
        };
        (viewport_y - next_index as f32 * height).min(0.)
    }
}

impl Timeline {
    pub(super) fn rebuild_rows(&mut self, cx: &Context<Self>) {
        let moving = cx
            .active_drag_value::<MoveTimelineItem>()
            .is_some_and(|drag| drag.timeline_id == cx.entity_id());
        // Presses and box selection keep their original hit geometry until release.
        if !moving && (self.item_drag.is_some() || self.marquee_selection.is_some()) {
            return;
        }
        let editor = self.editor.read(cx);
        if let Some(selection) = &mut self.stop_selection {
            selection.validate(editor);
        }
        let rows = if moving {
            TimelineRows::default()
        } else {
            let selection = editor
                .selected_item_ids()
                .filter_map(|id| editor.item_layer(id).map(|layer| (id, layer)))
                .collect::<HashSet<_>>();
            if selection != self.last_item_selection {
                self.expanded_layers
                    .extend(selection.iter().map(|(_, layer)| *layer));
                self.last_item_selection = selection;
            }
            TimelineRows::new(editor, &self.expanded_layers)
        };
        let anchor = if moving {
            self.item_drag.as_ref().and_then(ItemMoveDrag::origin_layer)
        } else {
            None
        };
        let scroll = layer_scroll_base(&self.layer_scroll);
        let offset = scroll.offset();
        scroll.set_offset(point(
            offset.x,
            px(self
                .rows
                .anchored_offset(&rows, f32::from(offset.y), LAYER_HEIGHT, anchor)),
        ));
        self.rows = Rc::new(rows);
    }

    pub(super) fn toggle_animation_rows(&mut self, layer: LayerId, cx: &mut Context<Self>) {
        if !self.expanded_layers.remove(&layer) {
            self.expanded_layers.insert(layer);
        }
        let editor = self.editor.read(cx);
        if let Some(selection) = &mut self.stop_selection {
            selection.keys.retain(|(address, _)| {
                editor
                    .item_layer(address.item_id)
                    .is_some_and(|layer| self.expanded_layers.contains(&layer))
            });
        }
        cx.notify();
    }
}
