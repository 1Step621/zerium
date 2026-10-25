use std::collections::HashSet;

use ::ui::ThemeColor;
use gpui::{
    Bounds, ClickEvent, Context, Div, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Window, div, point, prelude::*, px, size,
};

use zerium_core::timeline::{ItemId, LayerId};

use crate::ui::{
    TimelineEditorEntityExt as _,
    components::pane::PANE_HEADER_HEIGHT,
    panels::timeline::{LAYER_HEADER_WIDTH, Timeline, layer_scroll_base},
};

#[derive(Clone, Debug)]
pub(in crate::ui::panels::timeline) struct MarqueeSelection {
    pub(in crate::ui::panels::timeline) layer_index: usize,
    pub(in crate::ui::panels::timeline) button: MouseButton,
    pub(in crate::ui::panels::timeline) origin: [f32; 2],
    pub(in crate::ui::panels::timeline) current: [f32; 2],
    pub(in crate::ui::panels::timeline) baseline: HashSet<ItemId>,
    pub(in crate::ui::panels::timeline) active: bool,
}

impl MarqueeSelection {
    const ACTIVATION_DISTANCE: f32 = 4.;

    pub(in crate::ui::panels::timeline) fn bounds(&self) -> Bounds<Pixels> {
        Bounds::from_corners(
            point(
                px(self.origin[0].min(self.current[0])),
                px(self.origin[1].min(self.current[1])),
            ),
            point(
                px(self.origin[0].max(self.current[0])),
                px(self.origin[1].max(self.current[1])),
            ),
        )
    }

    pub(in crate::ui::panels::timeline) fn update(&mut self, position: gpui::Point<Pixels>) {
        self.current = [f32::from(position.x), f32::from(position.y)];
        let distance = (self.current[0] - self.origin[0]).hypot(self.current[1] - self.origin[1]);
        self.active |= distance >= Self::ACTIVATION_DISTANCE;
    }
}

pub(in crate::ui::panels::timeline) fn is_marquee_pointer_drag(event: &MouseMoveEvent) -> bool {
    event.pressed_button == Some(MouseButton::Right)
        || (event.pressed_button == Some(MouseButton::Left) && event.modifiers.control)
}

pub(in crate::ui::panels::timeline) fn is_marquee_pointer_down(event: &MouseDownEvent) -> bool {
    event.button == MouseButton::Right
        || (event.button == MouseButton::Left && event.modifiers.control)
}

impl Timeline {
    pub(in crate::ui::panels::timeline) fn begin_marquee_selection(
        &mut self,
        layer_index: usize,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = None;
        let additive = event.modifiers.shift;
        let baseline = if additive {
            self.editor.read(cx).selected_item_ids().collect()
        } else {
            HashSet::new()
        };
        let position = [f32::from(event.position.x), f32::from(event.position.y)];
        self.marquee_selection = Some(MarqueeSelection {
            layer_index,
            button: event.button,
            origin: position,
            current: position,
            baseline,
            active: false,
        });
        cx.notify();
        cx.stop_propagation();
    }

    pub(in crate::ui::panels::timeline) fn begin_primary_track_interaction(
        &mut self,
        layer_index: usize,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cursor_layer = Some(LayerId::new(layer_index as u64));
        if is_marquee_pointer_down(event) {
            self.begin_marquee_selection(layer_index, event, cx);
        } else {
            self.begin_playhead_scrub(event, window, cx);
        }
    }

    pub(in crate::ui::panels::timeline) fn update_marquee_selection(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !is_marquee_pointer_drag(event) || self.marquee_selection.is_none() {
            return;
        }
        self.update_marquee_selection_at(event.position, cx);
    }

    fn update_marquee_selection_at(
        &mut self,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let was_active = self
            .marquee_selection
            .as_ref()
            .is_some_and(|marquee| marquee.active);
        self.marquee_selection
            .as_mut()
            .expect("marquee selection was checked above")
            .update(position);
        let is_active = self
            .marquee_selection
            .as_ref()
            .is_some_and(|marquee| marquee.active);
        if is_active {
            self.apply_marquee_selection(cx);
        }
        if is_active || was_active != is_active {
            cx.notify();
        }
    }

    fn apply_marquee_selection(&mut self, cx: &mut Context<Self>) {
        let Some(marquee) = self
            .marquee_selection
            .as_ref()
            .filter(|marquee| marquee.active)
        else {
            return;
        };
        let selection_bounds = marquee.bounds();
        let list_bounds = layer_scroll_base(&self.layer_scroll).bounds();
        let track_bounds = Bounds {
            origin: point(
                list_bounds.origin.x + px(LAYER_HEADER_WIDTH),
                list_bounds.origin.y,
            ),
            size: size(
                (list_bounds.size.width - px(LAYER_HEADER_WIDTH)).max(px(0.)),
                list_bounds.size.height,
            ),
        };
        let layer_offset = f32::from(layer_scroll_base(&self.layer_scroll).offset().y);
        let mut selected = marquee.baseline.clone();
        let editor = self.editor.read(cx);
        for (item_id, layer, start, end) in editor.item_layouts() {
            let left = f32::from(track_bounds.origin.x)
                + self
                    .viewport
                    .x_at_seconds(editor.frame_rate().frame_to_seconds(start));
            let right = f32::from(track_bounds.origin.x)
                + self
                    .viewport
                    .x_at_seconds(editor.frame_rate().frame_to_seconds(end));
            let top = f32::from(list_bounds.origin.y)
                + layer.get() as f32 * self.viewport.layer_height
                + layer_offset;
            let item_bounds = Bounds {
                origin: point(px(left), px(top)),
                size: size(px((right - left).max(1.)), px(self.viewport.layer_height)),
            };
            if item_bounds.intersects(&track_bounds) && item_bounds.intersects(&selection_bounds) {
                selected.insert(item_id);
            }
        }
        self.editor
            .update_if_changed(cx, |editor| editor.select_items(selected));
    }

    pub(in crate::ui::panels::timeline) fn finish_marquee_selection(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut marquee) = self.marquee_selection.take() else {
            return;
        };
        marquee.update(event.position);
        if marquee.active {
            self.marquee_selection = Some(marquee);
            self.apply_marquee_selection(cx);
            self.marquee_selection = None;
        } else if marquee.button == MouseButton::Right {
            self.open_context_menu(
                marquee.layer_index,
                px(marquee.origin[0]),
                point(px(marquee.origin[0]), px(marquee.origin[1])),
                window,
                cx,
            );
        }
        cx.notify();
    }

    pub(in crate::ui::panels::timeline) fn marquee_overlay(
        &self,
        colors: ThemeColor,
    ) -> Option<Div> {
        let marquee = self
            .marquee_selection
            .as_ref()
            .filter(|marquee| marquee.active)?;
        let list_bounds = layer_scroll_base(&self.layer_scroll).bounds();
        let track_bounds = Bounds::from_corners(
            list_bounds.origin + point(px(LAYER_HEADER_WIDTH), px(0.)),
            list_bounds.bottom_right(),
        );
        let bounds = marquee.bounds().intersect(&track_bounds);
        if bounds.is_empty() {
            return None;
        }

        Some(
            div()
                .absolute()
                .left(bounds.origin.x - list_bounds.origin.x)
                .top(bounds.origin.y - list_bounds.origin.y + px(PANE_HEADER_HEIGHT))
                .w(bounds.size.width)
                .h(bounds.size.height)
                .border_1()
                .border_color(colors.primary)
                .bg(colors.primary.opacity(0.12)),
        )
    }

    pub(in crate::ui::panels::timeline) fn clear_selection_on_double_click(
        &mut self,
        event: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.click_count() < 2 {
            return;
        }
        self.editor.update_if_changed(cx, |editor| {
            editor.select_items(std::iter::empty::<ItemId>())
        });
        self.animation_selection
            .update(cx, |selection, cx| selection.clear(cx));
    }
}
