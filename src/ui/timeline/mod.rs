use std::{collections::HashSet, rc::Rc, sync::Arc};

use ::ui::{
    ActiveTheme as _, Icon, IconName, Sizable as _, ThemeColor,
    button::{Button, ButtonVariants as _},
    menu::{PopupMenu, PopupMenuItem},
};
use gpui::{
    App, Bounds, ClickEvent, Context, Corner, CursorStyle, DismissEvent, Div, DragMoveEvent, Empty,
    Entity, EntityId, FocusHandle, Focusable as _, Hsla, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels, Render, ScrollWheelEvent, SharedString,
    Stateful, Subscription, Task, UniformListScrollHandle, Window, anchored, canvas, deferred, div,
    point, prelude::*, px, relative, size, uniform_list,
};

use crate::{
    domain::timeline::{
        Frame, FrameDuration, FrameRate, ItemId, LayerId, ResizeEdge, SceneId, TimelineEditError,
        TimelineEditor, TimelineItem,
    },
    engine::media::MediaReaderRegistry,
    plugin_catalog::plugins,
};

use super::{
    TimelineEditorEntityExt as _,
    explorer::ExplorerFileDrag,
    pane::PANE_HEADER_HEIGHT,
    search_picker::{SearchPicker, SearchPickerEntry},
    session::{ProjectActivity, ProjectSession, ProjectSessionId, UiNotifications},
    time_grid,
    transport::{ScrubSource, TransportController},
};

mod clipboard;
mod model;
mod playback;
mod viewport;

use viewport::TimelineViewport;

const LAYER_HEADER_WIDTH: f32 = 200.;
const MIN_DYNAMIC_LAYER_COUNT: usize = 32;
const EXTRA_DYNAMIC_LAYERS: usize = 8;
const ZOOM_STEP: f32 = 1.05;

#[derive(Clone)]
struct LayerScrollHandle(UniformListScrollHandle);

impl LayerScrollHandle {
    fn new() -> Self {
        Self(UniformListScrollHandle::new())
    }

    fn base(&self) -> gpui::ScrollHandle {
        self.0.0.borrow().base_handle.clone()
    }

    fn offset(&self) -> gpui::Point<Pixels> {
        self.base().offset()
    }

    fn max_offset(&self) -> gpui::Size<Pixels> {
        self.base().max_offset()
    }

    fn bounds(&self) -> Bounds<Pixels> {
        self.base().bounds()
    }

    fn set_offset(&self, offset: gpui::Point<Pixels>) {
        self.base().set_offset(offset);
    }
}

#[derive(Clone)]
struct ResizeTimelineItem {
    timeline_id: EntityId,
    origin: TimelineItem,
    edge: ResizeEdge,
}

impl Render for ResizeTimelineItem {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(Clone)]
struct MoveTimelineItem {
    timeline_id: EntityId,
    item_id: ItemId,
}

impl Render for MoveTimelineItem {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(Clone, Copy, Debug)]
struct MovingItemOrigin {
    item_id: ItemId,
    source_layer: LayerId,
    start: Frame,
    duration: FrameDuration,
}

#[derive(Clone, Debug)]
struct ItemMoveOrigin {
    item_id: ItemId,
    items: Vec<MovingItemOrigin>,
    pointer_x: f32,
    pointer_y: f32,
}

#[derive(Clone, Copy, Debug)]
struct ContextTarget {
    layer: LayerId,
    start: Frame,
    item: Option<ItemId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExplorerDropTarget {
    layer: LayerId,
    start: Frame,
}

#[derive(Clone, Debug)]
struct MarqueeSelection {
    layer_index: usize,
    button: MouseButton,
    origin: [f32; 2],
    current: [f32; 2],
    baseline: HashSet<ItemId>,
    active: bool,
}

#[derive(Clone)]
struct TimelineContextMenu {
    content: TimelineContextMenuContent,
    position: gpui::Point<Pixels>,
}

#[derive(Clone)]
enum TimelineContextMenuContent {
    Commands(Entity<PopupMenu>),
    ItemPicker(Entity<SearchPicker<ItemPickerTarget>>),
}

#[derive(Clone)]
enum ItemPickerTarget {
    Plugin { plugin_id: String, item_id: String },
    Scene(SceneId),
    Paste,
}

impl MarqueeSelection {
    const ACTIVATION_DISTANCE: f32 = 4.;

    fn bounds(&self) -> Bounds<Pixels> {
        let left = self.origin[0].min(self.current[0]);
        let top = self.origin[1].min(self.current[1]);
        Bounds {
            origin: point(px(left), px(top)),
            size: size(
                px((self.origin[0] - self.current[0]).abs()),
                px((self.origin[1] - self.current[1]).abs()),
            ),
        }
    }

    fn update(&mut self, position: gpui::Point<Pixels>) {
        self.current = [f32::from(position.x), f32::from(position.y)];
        let distance = (self.current[0] - self.origin[0]).hypot(self.current[1] - self.origin[1]);
        self.active |= distance >= Self::ACTIVATION_DISTANCE;
    }
}

fn is_marquee_pointer_drag(event: &MouseMoveEvent) -> bool {
    event.pressed_button == Some(MouseButton::Right)
        || (event.pressed_button == Some(MouseButton::Left) && event.modifiers.control)
}

fn is_marquee_pointer_down(event: &MouseDownEvent) -> bool {
    event.button == MouseButton::Right
        || (event.button == MouseButton::Left && event.modifiers.control)
}

#[derive(Clone)]
struct TimelineGrid {
    major_ticks: Rc<Vec<(f64, f32)>>,
    minor_ticks: Rc<Vec<f32>>,
}

#[derive(Clone)]
struct LayerRenderState {
    editor: Entity<TimelineEditor>,
    colors: ThemeColor,
    layer_height: f32,
    viewport: TimelineViewport,
    viewport_width: f32,
    frame_rate: FrameRate,
    playhead_seconds: f64,
    selected_item_ids: Rc<HashSet<ItemId>>,
    hidden_layers: Rc<HashSet<LayerId>>,
    hidden_items: Rc<HashSet<ItemId>>,
    explorer_drop_target: Option<ExplorerDropTarget>,
    grid: TimelineGrid,
}

struct TimelineItemRenderData {
    item: TimelineItem,
    label: String,
}

/// A timeline whose layer rows are generated only while they are visible.
pub(crate) struct Timeline {
    editor: Entity<TimelineEditor>,
    transport: Entity<TransportController>,
    session: Entity<ProjectSession>,
    session_id: ProjectSessionId,
    notifications: Entity<UiNotifications>,
    media_readers: Arc<MediaReaderRegistry>,
    layer_scroll: LayerScrollHandle,
    viewport: TimelineViewport,
    context_target: Option<ContextTarget>,
    explorer_drop_target: Option<ExplorerDropTarget>,
    file_drop_error: Option<SharedString>,
    item_move_origin: Option<ItemMoveOrigin>,
    marquee_selection: Option<MarqueeSelection>,
    context_menu: Option<TimelineContextMenu>,
    scrubbing_playhead: bool,
    focus_handle: FocusHandle,
    _import_tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl Timeline {
    pub(crate) fn new(
        editor: Entity<TimelineEditor>,
        transport: Entity<TransportController>,
        session: Entity<ProjectSession>,
        notifications: Entity<UiNotifications>,
        media_readers: Arc<MediaReaderRegistry>,
        cx: &mut Context<Self>,
    ) -> Self {
        let session_id = session.read(cx).id();
        let subscriptions = vec![
            cx.observe(&editor, |_, _, cx| cx.notify()),
            cx.observe(&transport, |_, _, cx| cx.notify()),
            cx.observe(&session, |this, _, cx| {
                let session_id = this.session.read(cx).id();
                if session_id == this.session_id {
                    return;
                }
                this.session_id = session_id;
                this.cancel_async_work();
                this.viewport = TimelineViewport::default();
                this.layer_scroll.set_offset(point(px(0.), px(0.)));
                this.context_target = None;
                this.explorer_drop_target = None;
                this.item_move_origin = None;
                this.marquee_selection = None;
                this.context_menu = None;
                cx.notify();
            }),
        ];
        Self {
            editor,
            transport,
            session,
            session_id,
            notifications,
            media_readers,
            layer_scroll: LayerScrollHandle::new(),
            viewport: TimelineViewport::default(),
            context_target: None,
            explorer_drop_target: None,
            file_drop_error: None,
            item_move_origin: None,
            marquee_selection: None,
            context_menu: None,
            scrubbing_playhead: false,
            focus_handle: cx.focus_handle(),
            _import_tasks: Vec::new(),
            _subscriptions: subscriptions,
        }
    }

    fn dynamic_layer_count(&self, window: &Window, cx: &Context<Self>) -> usize {
        let highest_occupied_layer = self
            .editor
            .read(cx)
            .item_layouts()
            .map(|(_, layer, _, _)| layer.get())
            .max()
            .and_then(|layer| usize::try_from(layer).ok());
        let visible = (f32::from(window.viewport_size().height) / self.viewport.layer_height)
            .ceil()
            .max(1.) as usize;
        model::virtual_layer_count(
            highest_occupied_layer,
            visible,
            MIN_DYNAMIC_LAYER_COUNT,
            EXTRA_DYNAMIC_LAYERS,
        )
    }

    fn cancel_async_work(&mut self) {
        self._import_tasks.clear();
        self.file_drop_error = None;
    }

    fn track_viewport_width(window: &Window) -> f32 {
        (f32::from(window.viewport_size().width) - LAYER_HEADER_WIDTH).max(1.)
    }

    fn ruler_tick_label(seconds: f64) -> String {
        time_grid::format_timestamp(seconds)
    }

    fn visible_ticks(
        viewport: TimelineViewport,
        viewport_width: f32,
        step: f64,
    ) -> Vec<(f64, f32)> {
        let (visible_start, visible_end) = viewport.visible_time_range(viewport_width);
        time_grid::visible_seconds(visible_start, visible_end, step)
            .into_iter()
            .map(|seconds| (seconds, viewport.x_at_seconds(seconds)))
            .collect()
    }

    fn visible_frame_ticks(
        viewport: TimelineViewport,
        viewport_width: f32,
        frame_rate: FrameRate,
        step_frames: u64,
    ) -> Vec<(Frame, f32)> {
        let (visible_start, visible_end) = viewport.visible_time_range(viewport_width);
        time_grid::visible_frames(visible_start, visible_end, frame_rate, step_frames)
            .into_iter()
            .map(|frame| {
                let seconds = frame_rate.frame_to_seconds(frame);
                (frame, viewport.x_at_seconds(seconds))
            })
            .collect()
    }

    fn timeline_grid(
        viewport: TimelineViewport,
        viewport_width: f32,
        frame_rate: FrameRate,
    ) -> TimelineGrid {
        let major_ticks = Self::visible_ticks(viewport, viewport_width, viewport.ruler_step());
        let minor_ticks = Self::visible_frame_ticks(
            viewport,
            viewport_width,
            frame_rate,
            viewport.frame_grid_step(frame_rate),
        )
        .into_iter()
        .map(|(_, x)| x)
        .collect();
        TimelineGrid {
            major_ticks: Rc::new(major_ticks),
            minor_ticks: Rc::new(minor_ticks),
        }
    }

    fn paint_vertical_lines(
        bounds: Bounds<Pixels>,
        positions: impl IntoIterator<Item = f32>,
        color: Hsla,
        window: &mut Window,
    ) {
        let mut builder = PathBuilder::stroke(px(1.));
        for tick_x in positions {
            if tick_x < -1. || tick_x > f32::from(bounds.size.width) + 1. {
                continue;
            }
            let x = bounds.origin.x + px(tick_x) + px(0.5);
            builder.move_to(point(x, bounds.origin.y));
            builder.line_to(point(x, bounds.origin.y + bounds.size.height));
        }
        if let Ok(path) = builder.build() {
            window.paint_path(path, color);
        }
    }

    fn grid_canvas(grid: TimelineGrid, color_major: Hsla, color_minor: Hsla) -> impl IntoElement {
        canvas(
            move |_, _, _| grid,
            move |bounds, grid, window, _| {
                Self::paint_vertical_lines(
                    bounds,
                    grid.minor_ticks.iter().copied(),
                    color_minor,
                    window,
                );
                Self::paint_vertical_lines(
                    bounds,
                    grid.major_ticks.iter().map(|(_, x)| *x),
                    color_major,
                    window,
                );
            },
        )
        .absolute()
        .size_full()
    }

    fn dominant_scroll_delta(event: &ScrollWheelEvent, window: &Window) -> f32 {
        let delta = event.delta.pixel_delta(window.line_height());
        let x = f32::from(delta.x);
        let y = f32::from(delta.y);

        if x.abs() > y.abs() { x } else { y }
    }

    fn zoom_factor(event: &ScrollWheelEvent, window: &Window) -> Option<f32> {
        let delta = Self::dominant_scroll_delta(event, window);
        if delta > 0. {
            Some(ZOOM_STEP)
        } else if delta < 0. {
            Some(1. / ZOOM_STEP)
        } else {
            None
        }
    }

    fn zoom_horizontal(
        &mut self,
        event: &ScrollWheelEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(factor) = Self::zoom_factor(event, window) else {
            return;
        };

        let viewport_width = Self::track_viewport_width(window);
        let cursor_x = (f32::from(event.position.x) - LAYER_HEADER_WIDTH).clamp(0., viewport_width);
        if self.viewport.zoom_horizontal(factor, cursor_x) {
            cx.notify();
        }
    }

    fn zoom_vertical(&mut self, event: &ScrollWheelEvent, window: &Window, cx: &mut Context<Self>) {
        let Some(factor) = Self::zoom_factor(event, window) else {
            return;
        };

        let Some(scroll_ratio) = self.viewport.zoom_vertical(factor) else {
            return;
        };

        let old_offset = self.layer_scroll.offset();
        self.layer_scroll
            .set_offset(point(old_offset.x, old_offset.y * scroll_ratio));
        cx.notify();
    }

    fn on_track_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.modifiers.control {
            if event.modifiers.shift {
                self.zoom_vertical(event, window, cx);
            } else {
                self.zoom_horizontal(event, window, cx);
            }
        } else if event.modifiers.alt {
            let delta = event.delta.pixel_delta(window.line_height());
            let old_offset = self.layer_scroll.offset();
            let max_offset = self.layer_scroll.max_offset().height;
            let new_y = (old_offset.y + delta.y).clamp(-max_offset, px(0.));
            self.layer_scroll.set_offset(point(old_offset.x, new_y));
            cx.notify();
        } else {
            let delta = Self::dominant_scroll_delta(event, window);
            if self.viewport.scroll_horizontal(delta) {
                cx.notify();
            }
        }

        cx.stop_propagation();
    }

    fn on_label_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.modifiers.control {
            if event.modifiers.shift {
                self.zoom_vertical(event, window, cx);
            } else {
                self.zoom_horizontal(event, window, cx);
            }
        } else {
            let delta = event.delta.pixel_delta(window.line_height());
            let old_offset = self.layer_scroll.offset();
            let max_offset = self.layer_scroll.max_offset().height;
            let new_y = (old_offset.y + delta.y).clamp(-max_offset, px(0.));
            self.layer_scroll.set_offset(point(old_offset.x, new_y));
            cx.notify();
        }

        cx.stop_propagation();
    }

    fn pointer_frame(&self, position_x: f32, window: &Window, cx: &Context<Self>) -> Frame {
        let viewport_width = Self::track_viewport_width(window);
        self.viewport.frame_at_x(
            position_x,
            LAYER_HEADER_WIDTH,
            viewport_width,
            self.editor.read(cx).frame_rate(),
        )
    }
    fn seek_to_x(&mut self, position_x: f32, window: &mut Window, cx: &mut Context<Self>) {
        let frame = self.pointer_frame(position_x, window, cx);
        self.transport
            .update(cx, |transport, cx| transport.seek(frame, cx));
    }

    fn begin_playhead_scrub(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.scrubbing_playhead = true;
        self.transport.update(cx, |transport, cx| {
            transport.begin_scrub(ScrubSource::Timeline, cx);
        });
        self.seek_to_x(f32::from(event.position.x), window, cx);
    }

    fn update_playhead_scrub(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.scrubbing_playhead {
            return;
        }
        if !event.dragging() {
            self.finish_playhead_scrub(cx);
            return;
        }

        self.seek_to_x(f32::from(event.position.x), window, cx);
    }

    fn finish_playhead_scrub(&mut self, cx: &mut Context<Self>) {
        if !self.scrubbing_playhead {
            return;
        }
        self.scrubbing_playhead = false;
        self.transport.update(cx, |transport, cx| {
            transport.end_scrub(ScrubSource::Timeline, cx);
        });
    }

    fn begin_marquee_selection(
        &mut self,
        layer_index: usize,
        event: &MouseDownEvent,
        window: &mut Window,
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
        window.focus(&self.focus_handle, cx);
        cx.notify();
        cx.stop_propagation();
    }

    fn begin_primary_track_interaction(
        &mut self,
        layer_index: usize,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if is_marquee_pointer_down(event) {
            self.begin_marquee_selection(layer_index, event, window, cx);
        } else {
            self.begin_playhead_scrub(event, window, cx);
        }
    }

    fn update_marquee_selection(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !is_marquee_pointer_drag(event) || self.marquee_selection.is_none() {
            return;
        }
        let was_active = self
            .marquee_selection
            .as_ref()
            .is_some_and(|marquee| marquee.active);
        self.marquee_selection
            .as_mut()
            .expect("marquee selection was checked above")
            .update(event.position);
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
        let list_bounds = self.layer_scroll.bounds();
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
        let layer_offset = f32::from(self.layer_scroll.offset().y);
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

    fn finish_marquee_selection(
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

    fn marquee_overlay(&self, colors: ThemeColor) -> Option<Div> {
        let marquee = self
            .marquee_selection
            .as_ref()
            .filter(|marquee| marquee.active)?;
        let list_bounds = self.layer_scroll.bounds();
        let bounds = marquee.bounds();
        let left =
            f32::from(bounds.origin.x).max(f32::from(list_bounds.origin.x) + LAYER_HEADER_WIDTH);
        let right = f32::from(bounds.origin.x + bounds.size.width)
            .min(f32::from(list_bounds.origin.x + list_bounds.size.width));
        let top = f32::from(bounds.origin.y).max(f32::from(list_bounds.origin.y));
        let bottom = f32::from(bounds.origin.y + bounds.size.height)
            .min(f32::from(list_bounds.origin.y + list_bounds.size.height));
        if right <= left || bottom <= top {
            return None;
        }

        Some(
            div()
                .absolute()
                .left(px(left - f32::from(list_bounds.origin.x)))
                .top(px(
                    top - f32::from(list_bounds.origin.y) + PANE_HEADER_HEIGHT
                ))
                .w(px(right - left))
                .h(px(bottom - top))
                .border_1()
                .border_color(colors.primary)
                .bg(colors.primary.opacity(0.12)),
        )
    }

    fn resize_item_from_pointer(
        &mut self,
        drag: &ResizeTimelineItem,
        pointer_x: f32,
        snap_disabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if drag.timeline_id != cx.entity_id() {
            return;
        }

        let mut pointer = self.pointer_frame(pointer_x, window, cx);
        if !snap_disabled {
            pointer = self.snap_frame(pointer, None, Some(drag.origin.id), cx);
        }
        self.editor.update_if_changed(cx, |editor| {
            editor.resize_item(&drag.origin, drag.edge, pointer)
        });
    }

    fn prepare_item_move(
        &mut self,
        item_id: ItemId,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        if !self.editor.read(cx).is_item_selected(item_id) {
            self.editor
                .update_if_changed(cx, |editor| editor.select(item_id));
        }
        let items = {
            let editor = self.editor.read(cx);
            if editor.item(item_id).is_none() || editor.item_layer(item_id).is_none() {
                return;
            }
            let mut items = editor
                .selected_item_ids()
                .filter_map(|selected_id| {
                    let item = editor.item(selected_id)?;
                    let source_layer = editor.item_layer(selected_id)?;
                    Some(MovingItemOrigin {
                        item_id: selected_id,
                        source_layer,
                        start: item.start,
                        duration: item.duration,
                    })
                })
                .collect::<Vec<_>>();
            items.sort_unstable_by_key(|item| item.item_id.get());
            items
        };

        self.item_move_origin = Some(ItemMoveOrigin {
            item_id,
            items,
            pointer_x: f32::from(event.position.x),
            pointer_y: f32::from(event.position.y),
        });
        window.focus(&self.focus_handle, cx);
        cx.stop_propagation();
    }

    fn begin_item_interaction(
        &mut self,
        item_id: ItemId,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.modifiers.shift {
            self.item_move_origin = None;
            self.editor.update(cx, |editor, cx| {
                editor.finish_history_group();
                if editor.toggle_item_selection(item_id) {
                    cx.notify();
                }
            });
            window.focus(&self.focus_handle, cx);
            cx.stop_propagation();
            return;
        }
        self.prepare_item_move(item_id, event, window, cx);
    }

    fn select_item(&mut self, item_id: ItemId, event: &MouseDownEvent, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            editor.finish_history_group();
            let changed = if event.modifiers.shift {
                editor.toggle_item_selection(item_id)
            } else {
                editor.select(item_id)
            };
            if changed {
                cx.notify();
            }
        });
        cx.stop_propagation();
    }

    fn moved_items_delta(
        origin: &ItemMoveOrigin,
        pointer_x: f32,
        pointer_y: f32,
        pixels_per_second: f64,
        frame_rate: FrameRate,
        layer_height: f32,
    ) -> (i64, i64) {
        let delta_seconds = (pointer_x - origin.pointer_x) as f64 / pixels_per_second;
        let delta_frames = frame_rate.seconds_delta_to_frames(delta_seconds);
        let layer_delta = ((pointer_y - origin.pointer_y) / layer_height).round() as i64;
        Self::clamp_item_move_delta(origin, delta_frames, layer_delta)
    }

    fn clamp_item_move_delta(
        origin: &ItemMoveOrigin,
        frame_delta: i64,
        layer_delta: i64,
    ) -> (i64, i64) {
        let origins = origin
            .items
            .iter()
            .map(|item| model::MoveOrigin {
                start: item.start.get(),
                duration: item.duration.get(),
                layer: item.source_layer.get(),
            })
            .collect::<Vec<_>>();
        model::clamp_move_delta(&origins, frame_delta, layer_delta)
    }

    fn move_item_from_pointer(
        &mut self,
        drag: &MoveTimelineItem,
        pointer_x: f32,
        pointer_y: f32,
        snap_disabled: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if drag.timeline_id != cx.entity_id() {
            return;
        }
        let Some(origin) = self
            .item_move_origin
            .as_ref()
            .filter(|origin| origin.item_id == drag.item_id)
        else {
            return;
        };

        let frame_rate = self.editor.read(cx).frame_rate();
        let (mut frame_delta, layer_delta) = Self::moved_items_delta(
            origin,
            pointer_x,
            pointer_y,
            self.viewport.pixels_per_second(),
            frame_rate,
            self.viewport.layer_height,
        );
        if !snap_disabled {
            frame_delta = self.snap_item_move_delta(origin, frame_delta, layer_delta, cx);
        }
        let origins = origin
            .items
            .iter()
            .map(|item| (item.item_id, item.start, item.source_layer))
            .collect::<Vec<_>>();
        self.editor.update_if_changed(cx, |editor| {
            editor.move_items_from(&origins, frame_delta, layer_delta)
        });
    }

    fn snap_item_move_delta(
        &self,
        origin: &ItemMoveOrigin,
        frame_delta: i64,
        layer_delta: i64,
        cx: &Context<Self>,
    ) -> i64 {
        let editor = self.editor.read(cx);
        let frame_rate = editor.frame_rate();
        let moving_ids = origin
            .items
            .iter()
            .map(|item| item.item_id)
            .collect::<HashSet<_>>();
        let moving_times = origin
            .items
            .iter()
            .flat_map(|item| {
                let start = item
                    .start
                    .get()
                    .checked_add_signed(frame_delta)
                    .unwrap_or(0);
                let end = start.saturating_add(item.duration.get());
                [
                    frame_rate.frame_to_seconds(Frame::new(start)),
                    frame_rate.frame_to_seconds(Frame::new(end)),
                ]
            })
            .collect::<Vec<_>>();
        let mut targets = vec![editor.playhead_seconds()];
        for (_, start, end) in editor
            .item_time_ranges()
            .filter(|(item_id, _, _)| !moving_ids.contains(item_id))
        {
            targets.push(frame_rate.frame_to_seconds(start));
            targets.push(frame_rate.frame_to_seconds(end));
        }
        let offset = model::snap_offset_seconds(
            &moving_times,
            &targets,
            self.viewport.ruler_step(),
            self.viewport.pixels_per_second(),
        );
        let snapped = frame_delta.saturating_add(frame_rate.seconds_delta_to_frames(offset));
        Self::clamp_item_move_delta(origin, snapped, layer_delta).0
    }

    fn snap_frame(
        &self,
        frame: Frame,
        moving_duration: Option<FrameDuration>,
        excluded_item: Option<ItemId>,
        cx: &Context<Self>,
    ) -> Frame {
        let editor = self.editor.read(cx);
        let frame_rate = editor.frame_rate();
        let start_seconds = frame_rate.frame_to_seconds(frame);
        let mut moving_times = vec![start_seconds];
        if let Some(duration) = moving_duration {
            moving_times.push(
                frame_rate.frame_to_seconds(Frame::new(frame.get().saturating_add(duration.get()))),
            );
        }

        let mut targets = vec![editor.playhead_seconds()];
        for (_, start, end) in editor
            .item_time_ranges()
            .filter(|(item_id, _, _)| Some(*item_id) != excluded_item)
        {
            targets.push(frame_rate.frame_to_seconds(start));
            targets.push(frame_rate.frame_to_seconds(end));
        }
        let offset = model::snap_offset_seconds(
            &moving_times,
            &targets,
            self.viewport.ruler_step(),
            self.viewport.pixels_per_second(),
        );
        frame_rate.seconds_to_frame((start_seconds + offset).max(0.))
    }

    fn finish_item_move(&mut self, cx: &mut Context<Self>) {
        self.item_move_origin = None;
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
    }

    fn prepare_context_target(
        &mut self,
        layer_index: usize,
        pointer_x: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let raw_start = self.pointer_frame(f32::from(pointer_x), window, cx);
        let start = if window.modifiers().alt {
            raw_start
        } else {
            self.snap_frame(raw_start, None, None, cx)
        };
        self.context_target = Some(ContextTarget {
            layer: LayerId::new(layer_index as u64),
            start,
            item: self
                .editor
                .read(cx)
                .items_on_layer(LayerId::new(layer_index as u64))
                .into_iter()
                .rev()
                .find(|item| item.start <= raw_start && raw_start < item.end_exclusive())
                .map(|item| item.id),
        });
        if let Some(item_id) = self.context_target.and_then(|target| target.item)
            && !self.editor.read(cx).is_item_selected(item_id)
        {
            self.editor
                .update_if_changed(cx, |editor| editor.select(item_id));
        }
        window.focus(&self.focus_handle, cx);
    }

    fn open_context_menu(
        &mut self,
        layer_index: usize,
        pointer_x: Pixels,
        position: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prepare_context_target(layer_index, pointer_x, window, cx);
        let target_item = self.context_target.and_then(|target| target.item);
        let can_group = self.editor.read(cx).selected_item_ids().count() >= 1;
        let can_paste = self.can_paste_items(cx);
        let paste_target = self
            .context_target
            .map(|target| (target.layer, target.start));
        let timeline = cx.entity();
        let content = if let Some(item_id) = target_item {
            let menu = PopupMenu::build(window, cx, move |menu, _, _| {
                let copy_timeline = timeline.clone();
                let cut_timeline = timeline.clone();
                let paste_timeline = timeline.clone();
                menu.item(PopupMenuItem::new("コピー").on_click(move |_, window, cx| {
                    let focus_handle = copy_timeline.read(cx).focus_handle.clone();
                    copy_timeline.update(cx, |timeline, cx| {
                        timeline.copy_selected_items(cx);
                    });
                    window.focus(&focus_handle, cx);
                }))
                .item(
                    PopupMenuItem::new("切り取り").on_click(move |_, window, cx| {
                        let focus_handle = cut_timeline.read(cx).focus_handle.clone();
                        cut_timeline.update(cx, |timeline, cx| {
                            timeline.cut_selected_items(cx);
                        });
                        window.focus(&focus_handle, cx);
                    }),
                )
                .when(can_paste, |menu| {
                    menu.item(
                        PopupMenuItem::new("貼り付け").on_click(move |_, window, cx| {
                            let focus_handle = paste_timeline.read(cx).focus_handle.clone();
                            paste_timeline.update(cx, |timeline, cx| {
                                timeline.paste_items_at(paste_target, cx);
                            });
                            window.focus(&focus_handle, cx);
                        }),
                    )
                })
                .separator()
                .when(can_group, |menu| {
                    let group_timeline = timeline.clone();
                    menu.item(PopupMenuItem::new("シーンにまとめる").on_click(
                        move |_, window, cx| {
                            let focus_handle = group_timeline.read(cx).focus_handle.clone();
                            group_timeline.update(cx, |timeline, cx| {
                                timeline.group_selected_as_scene(cx);
                            });
                            window.focus(&focus_handle, cx);
                        },
                    ))
                    .separator()
                })
                .item(PopupMenuItem::new("削除").on_click(move |_, window, cx| {
                    let focus_handle = timeline.read(cx).focus_handle.clone();
                    timeline.update(cx, |timeline, cx| {
                        timeline.remove_item(item_id, cx);
                    });
                    window.focus(&focus_handle, cx);
                }))
            });
            cx.subscribe(&menu, |this, _, _: &DismissEvent, cx| {
                this.context_menu = None;
                cx.notify();
            })
            .detach();
            menu.read(cx).focus_handle(cx).focus(window, cx);
            TimelineContextMenuContent::Commands(menu)
        } else {
            let mut entries = plugins()
                .items()
                .map(|(plugin_id, schema)| {
                    SearchPickerEntry::from_plugin_schema(
                        plugin_id,
                        schema,
                        ItemPickerTarget::Plugin {
                            plugin_id: plugin_id.to_owned(),
                            item_id: schema.id().to_owned(),
                        },
                    )
                })
                .collect::<Vec<_>>();
            if can_paste {
                entries.insert(
                    0,
                    SearchPickerEntry::new("貼り付け", "クリップボード", ItemPickerTarget::Paste)
                        .search_terms(["paste"]),
                );
            }
            let editor = self.editor.read(cx);
            entries.extend(
                editor
                    .scenes()
                    .filter(|scene| editor.can_add_scene_instance(scene.id))
                    .map(|scene| {
                        SearchPickerEntry::new(
                            scene.name.clone(),
                            "シーン",
                            ItemPickerTarget::Scene(scene.id),
                        )
                        .search_terms(["scene"])
                    }),
            );
            let picker_timeline = timeline.clone();
            let picker = cx.new(|cx| {
                SearchPicker::new(
                    entries,
                    "アイテムを検索",
                    move |target, window, cx| {
                        let focus_handle = picker_timeline.read(cx).focus_handle.clone();
                        picker_timeline.update(cx, |timeline, cx| {
                            timeline.add_picker_item(target, cx);
                        });
                        window.focus(&focus_handle, cx);
                    },
                    window,
                    cx,
                )
            });
            cx.subscribe(&picker, |this, _, _: &DismissEvent, cx| {
                this.context_menu = None;
                cx.notify();
            })
            .detach();
            picker.focus_handle(cx).focus(window, cx);
            TimelineContextMenuContent::ItemPicker(picker)
        };
        self.context_menu = Some(TimelineContextMenu { content, position });
        cx.notify();
    }

    fn context_menu_overlay(
        &self,
        colors: ThemeColor,
        timeline: Entity<Self>,
    ) -> Option<impl IntoElement> {
        let context_menu = self.context_menu.as_ref()?;
        let content = match &context_menu.content {
            TimelineContextMenuContent::Commands(menu) => menu.clone().into_any_element(),
            TimelineContextMenuContent::ItemPicker(picker) => picker.clone().into_any_element(),
        };
        Some(
            deferred(
                anchored()
                    .position(context_menu.position)
                    .snap_to_window_with_margin(px(8.))
                    .anchor(Corner::TopLeft)
                    .child(
                        div()
                            .occlude()
                            .bg(colors.background)
                            .border_1()
                            .border_color(colors.border)
                            .rounded_md()
                            .shadow_md()
                            .on_mouse_down_out(move |_, _, cx| {
                                timeline.update(cx, |timeline, cx| {
                                    timeline.context_menu = None;
                                    cx.notify();
                                });
                            })
                            .child(content),
                    ),
            )
            .with_priority(1),
        )
    }

    fn add_item_at(
        &mut self,
        layer: LayerId,
        start: Frame,
        plugin_id: &str,
        item_id: &str,
        cx: &mut Context<Self>,
    ) -> Result<ItemId, TimelineEditError> {
        self.editor.update(cx, |editor, cx| {
            let item = editor.add_item(layer, start, plugin_id, item_id);
            if item.is_ok() {
                cx.notify();
            }
            item
        })
    }

    fn add_picker_item(&mut self, item: ItemPickerTarget, cx: &mut Context<Self>) {
        let Some(target) = self.context_target else {
            return;
        };
        match item {
            ItemPickerTarget::Plugin { plugin_id, item_id } => {
                if let Err(error) =
                    self.add_item_at(target.layer, target.start, &plugin_id, &item_id, cx)
                {
                    self.notifications.update(cx, |notifications, cx| {
                        notifications.push(format!("アイテムを追加できません: {error}"), cx);
                    });
                }
            }
            ItemPickerTarget::Scene(scene_id) => {
                let result = self.editor.update(cx, |editor, cx| {
                    let result = editor.add_scene_instance(target.layer, target.start, scene_id);
                    if result.is_ok() {
                        cx.notify();
                    }
                    result
                });
                if let Err(error) = result {
                    self.notifications.update(cx, |notifications, cx| {
                        notifications.push(format!("シーンを追加できません: {error}"), cx);
                    });
                }
            }
            ItemPickerTarget::Paste => {
                self.paste_items_at(Some((target.layer, target.start)), cx);
            }
        }
    }

    fn update_explorer_drop_target(
        &mut self,
        layer_index: usize,
        event: &DragMoveEvent<ExplorerFileDrag>,
        cx: &mut Context<Self>,
    ) {
        if !event.bounds.contains(&event.event.position) {
            return;
        }

        let local_x = f32::from(event.event.position.x - event.bounds.origin.x);
        let viewport_width = f32::from(event.bounds.size.width).max(1.);
        let mut start = self.viewport.frame_at_x(
            local_x,
            0.,
            viewport_width,
            self.editor.read(cx).frame_rate(),
        );
        if !event.event.modifiers.alt {
            start = self.snap_frame(start, None, None, cx);
        }
        let target = ExplorerDropTarget {
            layer: LayerId::new(layer_index as u64),
            start,
        };
        if self.explorer_drop_target != Some(target) {
            self.explorer_drop_target = Some(target);
            cx.notify();
        }
    }

    fn clear_explorer_drop_target(
        &mut self,
        layer_index: usize,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        if !hovered
            && self
                .explorer_drop_target
                .is_some_and(|target| target.layer == LayerId::new(layer_index as u64))
        {
            self.explorer_drop_target = None;
            cx.notify();
        }
    }

    fn drop_explorer_items(
        &mut self,
        layer_index: usize,
        drag: &ExplorerFileDrag,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let layer = LayerId::new(layer_index as u64);
        let raw_pointer = self.pointer_frame(f32::from(window.mouse_position().x), window, cx);
        let start = if window.modifiers().alt {
            raw_pointer
        } else {
            self.explorer_drop_target
                .filter(|target| target.layer == layer)
                .map(|target| target.start)
                .unwrap_or_else(|| self.snap_frame(raw_pointer, None, None, cx))
        };
        self.explorer_drop_target = None;
        window.focus(&self.focus_handle, cx);
        self.file_drop_error = None;

        let imports = drag
            .files()
            .iter()
            .map(|file| {
                (
                    file.path().to_path_buf(),
                    file.plugin_id().to_owned(),
                    file.item_id().to_owned(),
                    file.input_id().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        let editor = self.editor.clone();
        let media_readers = self.media_readers.clone();
        let session = self.session.clone();
        let operation =
            session.update(cx, |session, cx| session.begin(ProjectActivity::Import, cx));
        let task = cx.spawn(async move |timeline, cx| {
            let results = cx
                .background_spawn(async move {
                    imports
                        .into_iter()
                        .map(|(path, plugin_id, item_id, input_id)| {
                            let name = path
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| path.display().to_string());
                            let result =
                                media_readers.probe_for_item(path, &plugin_id, &item_id, &input_id);
                            (name, plugin_id, item_id, result)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            if !session.update(cx, |session, _| session.operation_is_current(operation)) {
                return;
            }
            let mut errors = Vec::new();
            let changed = editor.update(cx, |editor, cx| {
                let mut next_start = start;
                let mut added = Vec::new();
                for (name, plugin_id, schema_item_id, result) in results {
                    let imported = match result {
                        Ok(imported) => imported,
                        Err(error) => {
                            errors.push(format!("{name}: {error}"));
                            continue;
                        }
                    };
                    let item_id =
                        match editor.add_item(layer, next_start, &plugin_id, &schema_item_id) {
                            Ok(item_id) => item_id,
                            Err(error) => {
                                errors.push(format!("{name}: {error}"));
                                continue;
                            }
                        };
                    if let Err(error) = editor.set_item_asset(item_id, imported) {
                        editor.remove_item(item_id);
                        errors.push(format!("{name}: {error}"));
                        continue;
                    }
                    next_start = editor
                        .item(item_id)
                        .map(|item| item.end_exclusive())
                        .unwrap_or(next_start);
                    added.push(item_id);
                }
                if added.len() > 1 {
                    editor.select_items(added.iter().copied());
                }
                if !added.is_empty() {
                    cx.notify();
                }
                !added.is_empty()
            });
            let error = if errors.is_empty() {
                None
            } else if errors.len() == 1 {
                Some(SharedString::from(errors.remove(0)))
            } else {
                Some(SharedString::from(format!(
                    "{}件の読み込みに失敗しました: {}",
                    errors.len(),
                    errors.join(" / ")
                )))
            };
            if let Some(error_message) = error.clone() {
                timeline
                    .update(cx, |timeline, cx| {
                        timeline.notifications.update(cx, |notifications, cx| {
                            notifications.push(error_message, cx);
                        });
                    })
                    .ok();
            }
            timeline
                .update(cx, |timeline, cx| {
                    timeline.file_drop_error = error;
                    if changed || timeline.file_drop_error.is_some() {
                        cx.notify();
                    }
                })
                .ok();
            session.update(cx, |session, cx| {
                session.finish(operation, cx);
            });
        });
        self._import_tasks.push(task);
    }

    fn remove_item(&mut self, item_id: ItemId, cx: &mut Context<Self>) {
        self.stop_playback(cx);
        self.editor
            .update_if_changed(cx, |editor| editor.remove_item(item_id));
    }

    fn group_selected_as_scene(&mut self, cx: &mut Context<Self>) {
        self.stop_playback(cx);
        self.editor.update(cx, |editor, cx| {
            if editor.group_selected_as_scene().is_some() {
                cx.notify();
            }
        });
    }

    fn open_scene_item(&mut self, item_id: ItemId, event: &ClickEvent, cx: &mut Context<Self>) {
        if event.click_count() < 2 {
            return;
        }
        let scene_id = self
            .editor
            .read(cx)
            .item(item_id)
            .and_then(TimelineItem::scene_id);
        let Some(scene_id) = scene_id else {
            return;
        };
        self.stop_playback(cx);
        self.editor
            .update_if_changed(cx, |editor| editor.open_scene(scene_id));
        self.viewport = TimelineViewport::default();
        self.layer_scroll.set_offset(point(px(0.), px(0.)));
    }

    fn close_scene(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.stop_playback(cx);
        self.editor
            .update_if_changed(cx, TimelineEditor::close_scene);
        self.viewport = TimelineViewport::default();
        self.layer_scroll.set_offset(point(px(0.), px(0.)));
    }

    fn delete_empty_scene(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.stop_playback(cx);
        let deleted = self.editor.update_if_changed(cx, |editor| {
            let Some(scene_id) = editor.active_scene_id() else {
                return false;
            };
            if !editor.scene(scene_id).is_some_and(|scene| scene.is_empty()) {
                return false;
            }
            editor.delete_scene(scene_id)
        });
        if deleted {
            self.viewport = TimelineViewport::default();
            self.layer_scroll.set_offset(point(px(0.), px(0.)));
        }
    }

    pub(crate) fn remove_selected_item(&mut self, cx: &mut Context<Self>) {
        self.stop_playback(cx);
        self.editor
            .update_if_changed(cx, TimelineEditor::remove_selected_item);
    }

    fn transport_button(id: &'static str, icon: Icon, tooltip: &'static str) -> Button {
        Button::new(id)
            .icon(icon)
            .tooltip(tooltip)
            .small()
            .compact()
    }

    fn ruler(
        &self,
        colors: ThemeColor,
        viewport_width: f32,
        grid: TimelineGrid,
        cx: &mut Context<Self>,
    ) -> Div {
        let viewport = self.viewport;
        let (playhead_seconds, timecode) = {
            let editor = self.editor.read(cx);
            (editor.playhead_seconds(), editor.timecode())
        };
        let playhead_x = viewport.x_at_seconds(playhead_seconds);
        let ruler_ticks = grid.major_ticks.as_ref().clone();
        let playback_icon = if self.transport.read(cx).is_playing() {
            IconName::Pause
        } else {
            IconName::Play
        };
        let playback_tooltip = if self.transport.read(cx).is_playing() {
            "一時停止"
        } else {
            "再生"
        };

        div()
            .h(px(PANE_HEADER_HEIGHT))
            .flex_none()
            .flex()
            .border_b_1()
            .border_color(colors.border)
            .bg(colors.background)
            .child(
                div()
                    .w(px(LAYER_HEADER_WIDTH))
                    .h_full()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_1()
                    .px_1()
                    .border_r_1()
                    .border_color(colors.border)
                    .bg(colors.title_bar)
                    .on_scroll_wheel(cx.listener(Self::on_label_scroll))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                Self::transport_button(
                                    "timeline-previous",
                                    Icon::new(IconName::ChevronLeft),
                                    "前のフレーム",
                                )
                                .on_click(cx.listener(Self::previous_frame)),
                            )
                            .child(
                                Self::transport_button(
                                    "timeline-play",
                                    Icon::new(playback_icon),
                                    playback_tooltip,
                                )
                                .on_click(cx.listener(Self::toggle_playback_button)),
                            )
                            .child(
                                Self::transport_button(
                                    "timeline-next",
                                    Icon::new(IconName::ChevronRight),
                                    "次のフレーム",
                                )
                                .on_click(cx.listener(Self::next_frame)),
                            ),
                    )
                    .child(div().text_sm().text_color(colors.primary).child(timecode)),
            )
            .child(
                div()
                    .id("timeline-ruler-track")
                    .relative()
                    .flex_1()
                    .h_full()
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::begin_playhead_scrub))
                    .on_mouse_move(cx.listener(Self::update_playhead_scrub))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.finish_playhead_scrub(cx)),
                    )
                    .on_scroll_wheel(cx.listener(Self::on_track_scroll))
                    .child(
                        div()
                            .absolute()
                            .size_full()
                            .child(Self::grid_canvas(
                                grid,
                                colors.border.opacity(0.45),
                                colors.border.opacity(0.20),
                            ))
                            .children(ruler_ticks.into_iter().map(|(seconds, tick_x)| {
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .left(px(tick_x))
                                    .border_l_1()
                                    .border_color(colors.border)
                                    .flex()
                                    .items_center()
                                    .pl_1()
                                    .text_sm()
                                    .text_color(colors.muted_foreground)
                                    .child(Self::ruler_tick_label(seconds))
                            }))
                            .when(
                                playhead_x >= -4. && playhead_x <= viewport_width + 4.,
                                |this| {
                                    this.child(
                                        div()
                                            .absolute()
                                            .top_0()
                                            .bottom_0()
                                            .left(px(playhead_x))
                                            .w(px(2.))
                                            .bg(colors.primary),
                                    )
                                },
                            )
                            .when(
                                playhead_x >= -4. && playhead_x <= viewport_width + 4.,
                                |this| {
                                    this.child(
                                        div()
                                            .absolute()
                                            .top_0()
                                            .left(px(playhead_x - 4.))
                                            .size(px(8.))
                                            .rounded_b_sm()
                                            .bg(colors.primary),
                                    )
                                },
                            ),
                    ),
            )
    }

    fn layer_header(layer_number: usize, state: &LayerRenderState, cx: &mut Context<Self>) -> Div {
        let layer = LayerId::new(layer_number.saturating_sub(1) as u64);
        let hidden = state.hidden_layers.contains(&layer);
        let editor = state.editor.clone();
        div()
            .w(px(LAYER_HEADER_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .border_r_1()
            .border_color(state.colors.border)
            .on_scroll_wheel(cx.listener(Self::on_label_scroll))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .overflow_hidden()
                    .text_sm()
                    .text_color(if hidden {
                        state.colors.muted_foreground
                    } else {
                        state.colors.foreground
                    })
                    .child(format!("Layer {layer_number}")),
            )
            .child(
                Button::new(SharedString::from(format!(
                    "toggle-layer-visibility-{}",
                    layer.get()
                )))
                .small()
                .compact()
                .ghost()
                .icon(if hidden {
                    IconName::EyeOff
                } else {
                    IconName::Eye
                })
                .tooltip(if hidden {
                    "レイヤーを表示"
                } else {
                    "レイヤーを非表示"
                })
                .on_click(move |_, _, cx| {
                    editor.update(cx, |editor, cx| {
                        editor.toggle_layer_visibility(layer);
                        cx.notify();
                    });
                }),
            )
            .child(
                div()
                    .size(px(7.))
                    .rounded_full()
                    .bg(state.colors.muted_foreground),
            )
    }

    fn timeline_item(
        render_item: TimelineItemRenderData,
        state: &LayerRenderState,
        timeline_id: EntityId,
        clip_top: f32,
        clip_height: f32,
        visible_range: (f64, f64),
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let TimelineItemRenderData {
            item,
            label: item_label,
        } = render_item;
        let item_start = state.frame_rate.frame_to_seconds(item.start);
        let item_end = state.frame_rate.frame_to_seconds(item.end_exclusive());
        let (visible_start, visible_end) = visible_range;

        if item_end <= visible_start || item_start >= visible_end {
            return None;
        }

        let item_id = item.id;
        let scene_id = item.scene_id();
        let item_hidden = state.hidden_items.contains(&item_id);
        let item_left = state.viewport.x_at_seconds(item_start);
        let item_width = (state
            .frame_rate
            .frame_to_seconds(Frame::new(item.duration.get()))
            * state.viewport.pixels_per_second()) as f32;
        let is_selected = state.selected_item_ids.contains(&item_id);
        let mut animation_anchors = item
            .animations
            .iter()
            .chain(
                item.effects
                    .iter()
                    .flat_map(|effect| effect.animations.iter()),
            )
            .flat_map(|(_, animation)| animation.curves())
            .map(|(_, curve)| curve)
            .flat_map(|curve| curve.anchors().iter().map(|anchor| (*anchor)[0]))
            .collect::<Vec<_>>();
        animation_anchors.sort_by(f32::total_cmp);
        animation_anchors.dedup_by(|left, right| (*left - *right).abs() < 0.0001);

        let move_drag = MoveTimelineItem {
            timeline_id,
            item_id,
        };
        let left_drag = ResizeTimelineItem {
            timeline_id,
            origin: item.clone(),
            edge: ResizeEdge::Left,
        };
        let right_drag = ResizeTimelineItem {
            timeline_id,
            origin: item.clone(),
            edge: ResizeEdge::Right,
        };
        Some(
            div()
                .id(("timeline-item", item_id.get()))
                .absolute()
                .top(px(clip_top))
                .left(px(item_left))
                .h(px(clip_height))
                .w(px(item_width))
                .px_2()
                .flex()
                .items_center()
                .gap_1()
                .overflow_hidden()
                .cursor_pointer()
                .rounded_sm()
                .border_1()
                .border_color(if is_selected {
                    state.colors.primary
                } else {
                    state.colors.border
                })
                .bg(if is_selected {
                    state.colors.primary.opacity(0.24)
                } else if item_hidden {
                    state.colors.accent.opacity(0.35)
                } else {
                    state.colors.accent
                })
                .when(is_selected, |item| item.border_2())
                .text_sm()
                .text_color(if item_hidden {
                    state.colors.muted_foreground
                } else {
                    state.colors.accent_foreground
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event, window, cx| {
                        this.begin_item_interaction(item_id, event, window, cx);
                    }),
                )
                .on_click(cx.listener(move |this, event, _, cx| {
                    this.open_scene_item(item_id, event, cx);
                }))
                .on_drag(move_drag, |drag, _, _, cx| {
                    cx.stop_propagation();
                    cx.new(|_| drag.clone())
                })
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.finish_item_move(cx)),
                )
                .when(scene_id.is_none(), |this| {
                    this.child(
                        div()
                            .text_color(state.colors.primary)
                            .child(item.symbol().to_owned()),
                    )
                })
                .child(item_label)
                .child(
                    div()
                        .id(("timeline-item-left-handle", item_id.get()))
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left_0()
                        .w(px(6.))
                        .cursor_col_resize()
                        .bg(state.colors.primary.opacity(0.35))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event, _, cx| {
                                this.select_item(item_id, event, cx);
                            }),
                        )
                        .on_drag(left_drag, |drag, _, _, cx| {
                            cx.stop_propagation();
                            cx.new(|_| drag.clone())
                        }),
                )
                .child(
                    div()
                        .id(("timeline-item-right-handle", item_id.get()))
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .right_0()
                        .w(px(6.))
                        .cursor_col_resize()
                        .bg(state.colors.primary.opacity(0.35))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event, _, cx| {
                                this.select_item(item_id, event, cx);
                            }),
                        )
                        .on_drag(right_drag, |drag, _, _, cx| {
                            cx.stop_propagation();
                            cx.new(|_| drag.clone())
                        }),
                )
                .children(animation_anchors.into_iter().map(|progress| {
                    let margin_left = if progress <= f32::EPSILON {
                        0.
                    } else if progress >= 1. - f32::EPSILON {
                        -5.
                    } else {
                        -2.5
                    };
                    div()
                        .absolute()
                        .top(px(2.))
                        .left(relative(progress))
                        .ml(px(margin_left))
                        .size(px(5.))
                        .rounded_full()
                        .border_1()
                        .border_color(state.colors.background.opacity(0.7))
                        .bg(state
                            .colors
                            .primary
                            .opacity(if is_selected { 0.95 } else { 0.65 }))
                })),
        )
    }

    fn layer_row(
        layer_index: usize,
        state: LayerRenderState,
        items: Vec<TimelineItemRenderData>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let layer_number = layer_index + 1;
        let clip_top = (state.layer_height * 0.1).max(3.);
        let clip_height = (state.layer_height - clip_top * 2.).max(16.);
        let playhead_x = state.viewport.x_at_seconds(state.playhead_seconds);
        let visible_range = state.viewport.visible_time_range(state.viewport_width);
        let explorer_drop_x = state
            .explorer_drop_target
            .filter(|target| target.layer == LayerId::new(layer_index as u64))
            .map(|target| {
                state
                    .viewport
                    .x_at_seconds(state.frame_rate.frame_to_seconds(target.start))
            });
        let explorer_drop_highlight = state.colors.primary.opacity(0.08);
        let timeline_id = cx.entity_id();

        div()
            .id(("timeline-layer", layer_index))
            .h(px(state.layer_height))
            .w_full()
            .flex_none()
            .flex()
            .border_b_1()
            .border_color(state.colors.table_row_border)
            .bg(if layer_index.is_multiple_of(2) {
                state.colors.background
            } else {
                state.colors.table_even
            })
            .child(Self::layer_header(layer_number, &state, cx))
            .child(
                div()
                    .id(("timeline-track", layer_index))
                    .relative()
                    .flex_1()
                    .h_full()
                    .overflow_hidden()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event, window, cx| {
                            this.begin_primary_track_interaction(layer_index, event, window, cx);
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event, window, cx| {
                            this.begin_marquee_selection(layer_index, event, window, cx);
                        }),
                    )
                    .on_scroll_wheel(cx.listener(Self::on_track_scroll))
                    .on_drag_move(cx.listener(
                        move |this, event: &DragMoveEvent<ExplorerFileDrag>, _, cx| {
                            this.update_explorer_drop_target(layer_index, event, cx);
                        },
                    ))
                    .on_drag_hover::<ExplorerFileDrag>(cx.listener(move |this, hovered, _, cx| {
                        this.clear_explorer_drop_target(layer_index, *hovered, cx);
                    }))
                    .drag_over::<ExplorerFileDrag>(move |style, _, _, _| {
                        style.bg(explorer_drop_highlight)
                    })
                    .on_drop(
                        cx.listener(move |this, drag: &ExplorerFileDrag, window, cx| {
                            this.drop_explorer_items(layer_index, drag, window, cx);
                        }),
                    )
                    .child(
                        div()
                            .absolute()
                            .size_full()
                            .child(Self::grid_canvas(
                                state.grid.clone(),
                                state.colors.border.opacity(0.45),
                                state.colors.border.opacity(0.20),
                            ))
                            .children(items.into_iter().filter_map(|item| {
                                Self::timeline_item(
                                    item,
                                    &state,
                                    timeline_id,
                                    clip_top,
                                    clip_height,
                                    visible_range,
                                    cx,
                                )
                            }))
                            .when(
                                playhead_x >= -2. && playhead_x <= state.viewport_width + 2.,
                                |this| {
                                    this.child(
                                        div()
                                            .absolute()
                                            .top_0()
                                            .bottom_0()
                                            .left(px(playhead_x))
                                            .w(px(2.))
                                            .bg(state.colors.primary),
                                    )
                                },
                            )
                            .when_some(explorer_drop_x, |this, drop_x| {
                                this.child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .bottom_0()
                                        .left(px(drop_x - 1.))
                                        .w(px(2.))
                                        .bg(state.colors.primary),
                                )
                            }),
                    ),
            )
    }
}

impl Render for Timeline {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.transport.read(cx).is_playing() {
            self.transport
                .update(cx, |transport, cx| transport.advance(cx));
            window.request_animation_frame();
        }
        let layer_count = self.dynamic_layer_count(window, cx);
        let colors = cx.theme().colors;
        let viewport_width = Self::track_viewport_width(window);
        let layer_height = self.viewport.layer_height;
        let (
            frame_rate,
            playhead_seconds,
            selected_item_ids,
            hidden_layers,
            hidden_items,
            active_scene,
        ) = {
            let editor = self.editor.read(cx);
            (
                editor.frame_rate(),
                editor.playhead_seconds(),
                Rc::new(editor.selected_item_ids().collect()),
                Rc::new(editor.hidden_layer_ids().collect()),
                Rc::new(editor.hidden_item_ids().collect()),
                editor
                    .active_scene_id()
                    .and_then(|id| editor.scene(id))
                    .map(|scene| (scene.name.clone(), scene.is_empty())),
            )
        };
        let active_scene_name = active_scene.as_ref().map(|(name, _)| name.clone());
        let active_scene_is_empty = active_scene.is_some_and(|(_, is_empty)| is_empty);
        let scene_switcher_visible = active_scene_name.is_some();
        let grid = Self::timeline_grid(self.viewport, viewport_width, frame_rate);
        let ruler = self.ruler(colors, viewport_width, grid.clone(), cx);
        let row_state = LayerRenderState {
            editor: self.editor.clone(),
            colors,
            layer_height,
            viewport: self.viewport,
            viewport_width,
            frame_rate,
            playhead_seconds,
            selected_item_ids,
            hidden_layers,
            hidden_items,
            explorer_drop_target: self.explorer_drop_target,
            grid,
        };
        let timeline = cx.entity();
        let file_drop_error = self.file_drop_error.clone();

        div()
            .relative()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_mouse_move(cx.listener(|this, event, window, cx| {
                this.update_playhead_scrub(event, window, cx);
                this.update_marquee_selection(event, window, cx);
            }))
            .capture_any_mouse_up(cx.listener(|this, event: &MouseUpEvent, window, cx| {
                if event.button == MouseButton::Left {
                    this.finish_playhead_scrub(cx);
                    this.finish_item_move(cx);
                    this.finish_marquee_selection(event, window, cx);
                } else if event.button == MouseButton::Right {
                    this.finish_marquee_selection(event, window, cx);
                }
            }))
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<MoveTimelineItem>, window, cx| {
                    let drag = event.drag(cx).clone();
                    cx.set_active_drag_cursor_style(CursorStyle::ClosedHand, window);
                    this.move_item_from_pointer(
                        &drag,
                        f32::from(event.event.position.x),
                        f32::from(event.event.position.y),
                        event.event.modifiers.alt,
                        window,
                        cx,
                    );
                },
            ))
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<ResizeTimelineItem>, window, cx| {
                    let drag = event.drag(cx).clone();
                    this.resize_item_from_pointer(
                        &drag,
                        f32::from(event.event.position.x),
                        event.event.modifiers.alt,
                        window,
                        cx,
                    );
                },
            ))
            .bg(colors.background)
            .text_color(colors.foreground)
            .child(ruler)
            .child(
                uniform_list(
                    "timeline-layers",
                    layer_count,
                    move |visible_range, _window, cx| {
                        timeline.update(cx, |timeline, cx| {
                            visible_range
                                .map(|layer_index| {
                                    let items = {
                                        let editor = timeline.editor.read(cx);
                                        editor
                                            .items_on_layer(LayerId::new(layer_index as u64))
                                            .into_iter()
                                            .map(|item| {
                                                let label = editor
                                                    .item_label(item.id)
                                                    .unwrap_or_else(|| "不明なアイテム".to_owned());
                                                TimelineItemRenderData { item, label }
                                            })
                                            .collect()
                                    };
                                    Self::layer_row(layer_index, row_state.clone(), items, cx)
                                })
                                .collect::<Vec<_>>()
                        })
                    },
                )
                .track_scroll(&self.layer_scroll.0)
                .flex_1()
                .min_h_0()
                .w_full(),
            )
            .when_some(self.marquee_overlay(colors), |this, marquee| {
                this.child(marquee)
            })
            .when_some(
                self.context_menu_overlay(colors, cx.entity()),
                |this, menu| this.child(menu),
            )
            .when(active_scene_is_empty, |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap_3()
                                .px_5()
                                .py_4()
                                .rounded_md()
                                .border_1()
                                .border_color(colors.border)
                                .bg(colors.background.opacity(0.96))
                                .shadow_md()
                                .text_sm()
                                .child("このシーンは空です")
                                .child(
                                    Button::new("timeline-delete-empty-scene")
                                        .small()
                                        .danger()
                                        .icon(IconName::Delete)
                                        .label("シーンを削除")
                                        .tooltip("このシーンと、配置済みの全インスタンスを削除")
                                        .on_click(cx.listener(Self::delete_empty_scene)),
                                ),
                        ),
                )
            })
            .when_some(active_scene_name, |this, name| {
                this.child(
                    div()
                        .absolute()
                        .left(px(8.))
                        .bottom(px(8.))
                        .rounded_md()
                        .border_1()
                        .border_color(colors.border)
                        .bg(colors.background.opacity(0.96))
                        .shadow_md()
                        .child(
                            Button::new("timeline-close-scene")
                                .small()
                                .compact()
                                .ghost()
                                .icon(IconName::ChevronLeft)
                                .label(name)
                                .tooltip("親タイムラインへ戻る")
                                .on_click(cx.listener(Self::close_scene)),
                        ),
                )
            })
            .when_some(file_drop_error, |this, error| {
                this.child(
                    div()
                        .absolute()
                        .left(px(8.))
                        .bottom(px(if scene_switcher_visible { 48. } else { 8. }))
                        .max_w(px(520.))
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .border_1()
                        .border_color(colors.danger)
                        .bg(colors.background.opacity(0.96))
                        .text_sm()
                        .text_color(colors.danger)
                        .child(error),
                )
            })
    }
}
