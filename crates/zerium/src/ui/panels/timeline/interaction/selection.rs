use crate::ui::panels::timeline::LAYER_HEIGHT;
use std::collections::HashSet;

use ::ui::ThemeColor;
use gpui::{
    Bounds, ClickEvent, Context, DispatchPhase, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, Window, canvas, point, prelude::*, px, quad, size,
};

use zerium_core::timeline::{ItemId, LayerId, PropertyAddress};

use crate::ui::{
    TimelineEditorEntityExt as _,
    panels::timeline::{StopSelection, Timeline, TimelineRow, layer_scroll_base},
};

enum MarqueeBaseline {
    Items(HashSet<ItemId>),
    Stops(StopSelection),
}

pub(in crate::ui::panels::timeline) struct MarqueeSelection {
    layer_index: usize,
    button: MouseButton,
    stop: Option<(PropertyAddress, usize)>,
    origin: Point<Pixels>,
    current: Point<Pixels>,
    baseline: MarqueeBaseline,
    active: bool,
}

impl MarqueeSelection {
    const ACTIVATION_DISTANCE: f64 = 4.;

    fn bounds(&self) -> Bounds<Pixels> {
        Bounds::from_corners(
            self.origin.min(&self.current),
            self.origin.max(&self.current),
        )
    }

    fn update(&mut self, position: Point<Pixels>) {
        self.current = position;
        self.active |= (self.current - self.origin).magnitude() >= Self::ACTIVATION_DISTANCE;
    }
}

fn is_marquee_pointer_down(event: &MouseDownEvent) -> bool {
    event.button == MouseButton::Right
        || (event.button == MouseButton::Left && event.modifiers.control)
}

impl Timeline {
    pub(in crate::ui::panels::timeline) fn begin_stop_context_menu(
        &mut self,
        address: PropertyAddress,
        stop: usize,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        let Some(layer) = self.editor.read(cx).item_layer(address.item_id) else {
            return;
        };
        self.begin_marquee_selection(layer.get() as usize, event, cx);
        if let Some(marquee) = self.marquee_selection.as_mut() {
            marquee.stop = Some((address, stop));
        }
    }

    fn begin_marquee_selection(
        &mut self,
        layer_index: usize,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = None;
        let baseline = if let Some(TimelineRow::Property(_)) = self.row_at_position(event.position)
        {
            let mut selection = self.edit_stops(cx).clone();
            if !event.modifiers.shift {
                selection.keys.clear();
            }
            MarqueeBaseline::Stops(selection)
        } else {
            self.stop_selection = None;
            MarqueeBaseline::Items(if event.modifiers.shift {
                self.editor.read(cx).selected_item_ids().collect()
            } else {
                HashSet::new()
            })
        };
        let position = event.position;
        self.marquee_selection = Some(MarqueeSelection {
            layer_index,
            button: event.button,
            stop: None,
            origin: position,
            current: position,
            baseline,
            active: false,
        });
        cx.notify();
        cx.stop_propagation();
    }

    pub(in crate::ui::panels::timeline) fn begin_track_interaction(
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
            self.stop_selection = None;
            self.begin_playhead_scrub(event, window, cx);
        }
    }

    fn update_marquee_selection(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) -> bool {
        let Some(marquee) = &mut self.marquee_selection else {
            return false;
        };
        if event.pressed_button != Some(marquee.button) {
            self.marquee_selection = None;
            cx.notify();
            return true;
        }
        marquee.update(event.position);
        if marquee.active {
            self.apply_marquee_selection(cx);
            cx.notify();
        }
        true
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
        let mut selected = match &marquee.baseline {
            MarqueeBaseline::Stops(baseline) => {
                self.apply_stop_marquee(selection_bounds, baseline.clone(), cx);
                return;
            }
            MarqueeBaseline::Items(baseline) => baseline.clone(),
        };
        self.stop_selection = None;
        let list_bounds = layer_scroll_base(&self.layer_scroll).bounds();
        let track_bounds = self.track_bounds();
        let layer_offset = f32::from(layer_scroll_base(&self.layer_scroll).offset().y);
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
                + self.rows.layer_row(layer) as f32 * LAYER_HEIGHT
                + layer_offset;
            let item_bounds = Bounds {
                origin: point(px(left), px(top)),
                size: size(px((right - left).max(1.)), px(LAYER_HEIGHT)),
            };
            if item_bounds.intersects(&track_bounds) && item_bounds.intersects(&selection_bounds) {
                selected.insert(item_id);
            }
        }
        self.editor
            .update_if_changed(cx, |editor| editor.select_items(selected));
    }

    fn finish_marquee_selection(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self
            .marquee_selection
            .as_ref()
            .is_none_or(|marquee| marquee.button != event.button)
        {
            return false;
        }
        let Some(mut marquee) = self.marquee_selection.take() else {
            return false;
        };
        marquee.update(event.position);
        if marquee.active {
            self.marquee_selection = Some(marquee);
            self.apply_marquee_selection(cx);
            self.marquee_selection = None;
        } else if marquee.button == MouseButton::Right {
            let position = marquee.origin;
            if let Some((address, stop)) = marquee.stop {
                self.open_animation_context_menu(&address, Some(stop), position, window, cx);
            } else {
                self.open_context_menu(marquee.layer_index, position.x, position, window, cx);
            }
        } else if let MarqueeBaseline::Stops(baseline) = marquee.baseline {
            self.stop_selection = Some(baseline);
        }
        cx.notify();
        true
    }

    pub(in crate::ui::panels::timeline) fn marquee_overlay(
        &self,
        colors: ThemeColor,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let selection = self
            .marquee_selection
            .as_ref()
            .filter(|marquee| marquee.active)
            .map(|marquee| marquee.bounds().intersect(&self.track_bounds()))
            .filter(|bounds| !bounds.is_empty());
        let timeline = cx.entity();
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                if let Some(bounds) = selection {
                    window.paint_quad(quad(
                        bounds,
                        px(0.),
                        colors.primary.opacity(0.12),
                        px(1.),
                        colors.primary,
                        Default::default(),
                    ));
                }
                // Register before a press so short gestures do not depend on a repaint.
                let moving = timeline.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture
                        && moving.read(cx).marquee_selection.is_some()
                        && moving.update(cx, |this, cx| this.update_marquee_selection(event, cx))
                    {
                        cx.stop_propagation();
                    }
                });
                let releasing = timeline.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture
                        && releasing.read(cx).marquee_selection.is_some()
                        && releasing.update(cx, |this, cx| {
                            this.finish_marquee_selection(event, window, cx)
                        })
                    {
                        cx.stop_propagation();
                    }
                });
            },
        )
        .absolute()
        .inset_0()
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
        self.stop_selection = None;
        self.editor.update_if_changed(cx, |editor| {
            editor.select_items(std::iter::empty::<ItemId>())
        });
        self.animation_selection
            .update(cx, |selection, cx| selection.clear(cx));
    }
}
