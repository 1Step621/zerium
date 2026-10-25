use std::{collections::HashSet, path::PathBuf};

use ::ui::ThemeColor;
use gpui::{
    Bounds, Context, Div, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Window, div, point,
    prelude::*, px,
};

use super::Explorer;

#[derive(Clone, Debug)]
pub(super) struct ExplorerMarquee {
    origin: [f32; 2],
    current: [f32; 2],
    baseline: HashSet<PathBuf>,
    active: bool,
}

impl ExplorerMarquee {
    const ACTIVATION_DISTANCE: f32 = 4.;

    fn update(&mut self, position: gpui::Point<Pixels>) {
        self.current = [f32::from(position.x), f32::from(position.y)];
        self.active |= (self.current[0] - self.origin[0]).hypot(self.current[1] - self.origin[1])
            >= Self::ACTIVATION_DISTANCE;
    }

    fn bounds(&self) -> Bounds<Pixels> {
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
}

impl Explorer {
    pub(super) fn select_entry(
        &mut self,
        path: PathBuf,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        let additive = event.modifiers.shift || event.modifiers.control || event.modifiers.platform;
        if additive {
            if !self.selected_paths.insert(path.clone()) {
                self.selected_paths.remove(&path);
            }
        } else if !self.selected_paths.contains(&path) {
            self.selected_paths.clear();
            self.selected_paths.insert(path);
        }
        cx.notify();
        cx.stop_propagation();
    }

    pub(super) fn begin_marquee(
        &mut self,
        event: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let additive = event.modifiers.shift || event.modifiers.control || event.modifiers.platform;
        let baseline = if additive {
            self.selected_paths.clone()
        } else {
            self.selected_paths.clear();
            HashSet::new()
        };
        let position = [f32::from(event.position.x), f32::from(event.position.y)];
        self.marquee = Some(ExplorerMarquee {
            origin: position,
            current: position,
            baseline,
            active: false,
        });
        cx.notify();
    }

    pub(super) fn update_marquee(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !event.dragging() || self.marquee.is_none() {
            return;
        }
        self.marquee
            .as_mut()
            .expect("explorer marquee was checked above")
            .update(event.position);
        if !self.marquee.as_ref().is_some_and(|marquee| marquee.active) {
            return;
        }
        self.apply_marquee();
        cx.notify();
    }

    fn apply_marquee(&mut self) {
        let Some(marquee) = self.marquee.as_ref().filter(|marquee| marquee.active) else {
            return;
        };
        let selection_bounds = marquee.bounds();
        let viewport = self.scroll_handle.bounds();
        let scroll_offset = self.scroll_handle.offset();
        let mut selected = marquee.baseline.clone();
        for (index, entry) in self.entries.iter().enumerate() {
            let Some(mut row) = self.scroll_handle.bounds_for_item(index) else {
                continue;
            };
            row.origin.x += scroll_offset.x;
            row.origin.y += scroll_offset.y;
            if row.intersects(&viewport) && row.intersects(&selection_bounds) {
                selected.insert(entry.path.clone());
            }
        }
        self.selected_paths = selected;
    }

    pub(super) fn finish_marquee(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        let Some(mut marquee) = self.marquee.take() else {
            return;
        };
        marquee.update(event.position);
        if marquee.active {
            self.marquee = Some(marquee);
            self.apply_marquee();
            self.marquee = None;
        }
        cx.notify();
    }

    pub(super) fn marquee_overlay(&self, colors: ThemeColor) -> Option<Div> {
        let marquee = self.marquee.as_ref().filter(|marquee| marquee.active)?;
        let viewport = self.scroll_handle.bounds();
        let bounds = marquee.bounds().intersect(&viewport);
        if bounds.is_empty() {
            return None;
        }
        Some(
            div()
                .absolute()
                .left(bounds.origin.x - viewport.origin.x)
                .top(bounds.origin.y - viewport.origin.y)
                .w(bounds.size.width)
                .h(bounds.size.height)
                .border_1()
                .border_color(colors.primary)
                .bg(colors.primary.opacity(0.12)),
        )
    }
}
