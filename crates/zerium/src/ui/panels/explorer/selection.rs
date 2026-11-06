use std::{collections::HashSet, path::PathBuf};

use ::ui::ThemeColor;
use gpui::{Context, Div, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Window, div, prelude::*};

use super::Explorer;
use crate::ui::components::selection::SelectionMarquee;

#[derive(Clone, Debug)]
pub(super) struct ExplorerMarquee {
    geometry: SelectionMarquee,
    baseline: HashSet<PathBuf>,
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
        self.marquee = Some(ExplorerMarquee {
            geometry: SelectionMarquee::new(event.position),
            baseline,
        });
        cx.notify();
    }

    pub(super) fn update_marquee(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !event.dragging() {
            return;
        }
        let Some(marquee) = &mut self.marquee else {
            return;
        };
        marquee.geometry.update(event.position);
        if !marquee.geometry.is_active() {
            return;
        }
        self.apply_marquee();
        cx.notify();
    }

    fn apply_marquee(&mut self) {
        let Some(marquee) = self
            .marquee
            .as_ref()
            .filter(|marquee| marquee.geometry.is_active())
        else {
            return;
        };
        let selection_bounds = marquee.geometry.bounds();
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
        marquee.geometry.update(event.position);
        if marquee.geometry.is_active() {
            self.marquee = Some(marquee);
            self.apply_marquee();
            self.marquee = None;
        }
        cx.notify();
    }

    pub(super) fn marquee_overlay(&self, colors: ThemeColor) -> Option<Div> {
        let marquee = self
            .marquee
            .as_ref()
            .filter(|marquee| marquee.geometry.is_active())?;
        let viewport = self.scroll_handle.bounds();
        let bounds = marquee.geometry.bounds().intersect(&viewport);
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
