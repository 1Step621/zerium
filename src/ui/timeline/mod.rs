use std::{collections::HashSet, rc::Rc, sync::Arc};

use ::ui::{
    ActiveTheme as _, Colorize as _, Icon, IconName, Sizable as _, ThemeColor,
    button::{Button, ButtonVariants as _},
    menu::{PopupMenu, PopupMenuItem, popup_menu::PopupMenuExt as _},
};
use gpui::{
    App, Bounds, ClickEvent, Context, Corner, CursorStyle, DismissEvent, Div, DragMoveEvent, Empty,
    Entity, EntityId, FocusHandle, Focusable as _, Hsla, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels, Render, ScrollWheelEvent, SharedString,
    SmoothScrollMode, Stateful, Subscription, UniformListScrollHandle, Window, anchored, canvas,
    deferred, div, point, prelude::*, px, relative, size, uniform_list,
};

use crate::{
    domain::timeline::{
        Frame, FrameDuration, FrameRate, ItemId, LayerId, ResizeEdge, SceneId, TimelineEditError,
        TimelineEditor, TimelineItem, TimelineTime,
    },
    engine::media::MediaReaderRegistry,
    plugin::plugins,
};

use super::{
    TimelineEditorEntityExt as _,
    animation_curve::{AnimationSelection, AnimationTarget},
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
mod render;
mod viewport;

use viewport::TimelineViewport;

const LAYER_HEADER_WIDTH: f32 = 200.;
const SCENE_SWITCHER_LABEL_WIDTH: usize = 12;
const ZOOM_STEP: f32 = 1.05;
const ANIMATION_STOP_SNAP_DISTANCE: f32 = 8.;
const ANIMATION_STOP_POSITION_EPSILON: f32 = 0.0001;

fn layer_scroll_base(handle: &UniformListScrollHandle) -> gpui::ScrollHandle {
    handle.0.borrow().base_handle.clone()
}

#[derive(Clone)]
struct ResizeTimelineItem {
    timeline_id: EntityId,
    origins: Rc<[TimelineItem]>,
    anchor_id: ItemId,
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

#[derive(Clone)]
struct MoveAnimationStop {
    timeline_id: EntityId,
    target: AnimationTarget,
    stop: usize,
    snap_frame: Frame,
    follow_focus: Option<usize>,
}

impl Render for MoveAnimationStop {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
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

#[derive(Clone, Copy)]
struct RenderResultHighlight {
    top_layer: LayerId,
    bottom_layer: LayerId,
    start: Frame,
    end: Frame,
}

#[derive(Clone)]
struct LayerRenderState {
    editor: Entity<TimelineEditor>,
    transport: Entity<TransportController>,
    animation_target: Option<AnimationTarget>,
    focused_animation_segment: Option<usize>,
    colors: ThemeColor,
    layer_height: f32,
    viewport: TimelineViewport,
    viewport_width: f32,
    frame_rate: FrameRate,
    playhead_seconds: f64,
    selected_item_ids: Rc<HashSet<ItemId>>,
    hidden_layers: Rc<HashSet<LayerId>>,
    hidden_items: Rc<HashSet<ItemId>>,
    render_result_highlights: Rc<Vec<RenderResultHighlight>>,
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
    animation_selection: Entity<AnimationSelection>,
    session: Entity<ProjectSession>,
    session_id: ProjectSessionId,
    notifications: Entity<UiNotifications>,
    media_readers: Arc<MediaReaderRegistry>,
    layer_scroll: UniformListScrollHandle,
    viewport: TimelineViewport,
    context_target: Option<ContextTarget>,
    cursor_layer: Option<LayerId>,
    explorer_drop_target: Option<ExplorerDropTarget>,
    file_drop_error: Option<SharedString>,
    item_move_origin: Option<ItemMoveOrigin>,
    marquee_selection: Option<MarqueeSelection>,
    context_menu: Option<TimelineContextMenu>,
    scrubbing_playhead: bool,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl Timeline {
    pub(crate) fn new(
        editor: Entity<TimelineEditor>,
        transport: Entity<TransportController>,
        animation_selection: Entity<AnimationSelection>,
        session: Entity<ProjectSession>,
        notifications: Entity<UiNotifications>,
        media_readers: Arc<MediaReaderRegistry>,
        cx: &mut Context<Self>,
    ) -> Self {
        let session_id = session.read(cx).id();
        let layer_scroll = UniformListScrollHandle::new();
        layer_scroll
            .0
            .borrow_mut()
            .smooth_scroll
            .set_mode(SmoothScrollMode::Disabled);
        let subscriptions = vec![
            cx.observe(&editor, |_, _, cx| cx.notify()),
            cx.observe(&transport, |_, _, cx| cx.notify()),
            cx.observe(&animation_selection, |_, _, cx| cx.notify()),
            cx.observe(&session, |this, _, cx| {
                let session_id = this.session.read(cx).id();
                if session_id == this.session_id {
                    return;
                }
                this.session_id = session_id;
                this.cancel_async_work();
                this.reset_viewport();
                this.context_target = None;
                this.cursor_layer = None;
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
            animation_selection,
            session,
            session_id,
            notifications,
            media_readers,
            layer_scroll,
            viewport: TimelineViewport::default(),
            context_target: None,
            cursor_layer: None,
            explorer_drop_target: None,
            file_drop_error: None,
            item_move_origin: None,
            marquee_selection: None,
            context_menu: None,
            scrubbing_playhead: false,
            focus_handle: cx.focus_handle(),
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
        let scroll = layer_scroll_base(&self.layer_scroll);
        let measured_height = f32::from(scroll.bounds().size.height);
        let viewport_height = if measured_height > 0. {
            measured_height
        } else {
            f32::from(window.viewport_size().height)
        };
        let scroll_top = (-f32::from(scroll.offset().y)).max(0.);
        let viewport_end = ((scroll_top + viewport_height) / self.viewport.layer_height)
            .ceil()
            .max(1.) as usize;
        model::virtual_layer_count(highest_occupied_layer, viewport_end)
    }

    fn reset_viewport(&mut self) {
        self.viewport = TimelineViewport::default();
        layer_scroll_base(&self.layer_scroll).set_offset(point(px(0.), px(0.)));
    }

    fn switch_scene(&mut self, scene_id: Option<SceneId>, cx: &mut Context<Self>) {
        if self.editor.read(cx).active_scene_id() == scene_id {
            return;
        }
        self.stop_playback(cx);
        while self.editor.read(cx).active_scene_id().is_some() {
            self.editor
                .update_if_changed(cx, TimelineEditor::close_scene);
        }
        if let Some(scene_id) = scene_id {
            self.editor
                .update_if_changed(cx, |editor| editor.open_scene(scene_id));
        }
        self.reset_viewport();
    }

    fn cancel_async_work(&mut self) {
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

        let old_offset = layer_scroll_base(&self.layer_scroll).offset();
        layer_scroll_base(&self.layer_scroll)
            .set_offset(point(old_offset.x, old_offset.y * scroll_ratio));
        cx.notify();
    }

    fn scroll_layers(&self, event: &ScrollWheelEvent, window: &Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(window.line_height());
        let scroll = layer_scroll_base(&self.layer_scroll);
        let old_offset = scroll.offset();
        let new_y = (old_offset.y + delta.y).min(px(0.));
        if new_y != old_offset.y {
            scroll.set_offset(point(old_offset.x, new_y));
            cx.notify();
        }
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
            self.scroll_layers(event, window, cx);
        } else {
            let delta = Self::dominant_scroll_delta(event, window);
            if self.viewport.scroll_horizontal(delta) {
                cx.notify();
            }
        }

        window.prevent_default();
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
            self.scroll_layers(event, window, cx);
        }

        window.prevent_default();
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

    fn begin_primary_track_interaction(
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

    fn update_marquee_selection(
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
        let list_bounds = layer_scroll_base(&self.layer_scroll).bounds();
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
            pointer = self.snap_frame(pointer, None, &drag.origins, cx);
        }
        self.editor.update_if_changed(cx, |editor| {
            editor.resize_items(drag.origins.as_ref(), drag.anchor_id, drag.edge, pointer)
        });
    }

    fn prepare_item_move(
        &mut self,
        item_id: ItemId,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        self.cursor_layer = self.editor.read(cx).item_layer(item_id);
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
        cx.stop_propagation();
    }

    fn begin_item_interaction(
        &mut self,
        item_id: ItemId,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        self.cursor_layer = self.editor.read(cx).item_layer(item_id);
        if event.modifiers.shift {
            self.item_move_origin = None;
            self.editor.update(cx, |editor, cx| {
                editor.finish_history_group();
                if editor.toggle_item_selection(item_id) {
                    cx.notify();
                }
            });
            cx.stop_propagation();
            return;
        }
        self.prepare_item_move(item_id, event, cx);
    }

    fn select_item(&mut self, item_id: ItemId, event: &MouseDownEvent, cx: &mut Context<Self>) {
        self.cursor_layer = self.editor.read(cx).item_layer(item_id);
        self.editor.update(cx, |editor, cx| {
            editor.finish_history_group();
            let changed = if event.modifiers.shift {
                editor.toggle_item_selection(item_id)
            } else if editor.is_item_selected(item_id) {
                false
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
        excluded_items: &[TimelineItem],
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
            .filter(|(item_id, _, _)| !excluded_items.iter().any(|item| item.id == *item_id))
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

    fn move_animation_stop_from_pointer(
        &mut self,
        drag: &MoveAnimationStop,
        pointer_x: f32,
        snap_disabled: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if drag.timeline_id != cx.entity_id()
            || self.animation_selection.read(cx).target() != Some(&drag.target)
        {
            return;
        }

        let mut frame = self.pointer_frame(pointer_x, window, cx);
        let progress = {
            let editor = self.editor.read(cx);
            let Some(item) = editor
                .selected_item()
                .filter(|item| item.id == drag.target.item_id)
            else {
                return;
            };
            let Some(track) = item.animation_track(
                drag.target.effect_id,
                &drag.target.property_id,
                drag.target.element_id,
                drag.target.scalar_index,
            ) else {
                return;
            };
            let Some(previous) = drag
                .stop
                .checked_sub(1)
                .and_then(|index| track.stops().get(index))
            else {
                return;
            };
            let Some(next) = track.stops().get(drag.stop + 1) else {
                return;
            };
            let frame_at = |position| {
                Frame::new(item.animation_timeline_frame(position).round().max(0.) as u64)
            };
            let minimum = Frame::new(frame_at(previous.position()).get().saturating_add(1));
            let maximum = Frame::new(frame_at(next.position()).get().saturating_sub(1));
            if minimum > maximum {
                return;
            }
            frame = frame.clamp(minimum, maximum);
            if !snap_disabled {
                let playhead_x = LAYER_HEADER_WIDTH
                    + self
                        .viewport
                        .x_at_seconds(editor.frame_rate().frame_to_seconds(drag.snap_frame));
                if (minimum..=maximum).contains(&drag.snap_frame)
                    && (pointer_x - playhead_x).abs() <= ANIMATION_STOP_SNAP_DISTANCE
                {
                    frame = drag.snap_frame;
                }
            }
            item.animation_progress_at_time(TimelineTime::from_frame(frame))
        };

        let changed = self.editor.update(cx, |editor, cx| {
            let changed = editor.move_selected_animation_stop(
                drag.target.effect_id,
                drag.target.property_id.clone(),
                drag.target.element_id,
                drag.target.scalar_index,
                drag.stop,
                progress,
            );
            if changed {
                cx.notify();
            }
            changed
        });
        if changed && let Some(segment) = drag.follow_focus {
            self.animation_selection.read(cx).focus_segment(segment);
            self.transport
                .update(cx, |transport, cx| transport.set_playhead(frame, cx));
        }
    }

    fn finish_item_move(&mut self, cx: &mut Context<Self>) {
        self.item_move_origin.take();
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
        let layer = LayerId::new(layer_index as u64);
        let target = self.context_target_at(layer, pointer_x, true, window, cx);
        self.cursor_layer = Some(layer);
        self.context_target = Some(target);
        if let Some(item_id) = target.item
            && !self.editor.read(cx).is_item_selected(item_id)
        {
            self.editor
                .update_if_changed(cx, |editor| editor.select(item_id));
        }
    }

    fn context_target_at(
        &self,
        layer: LayerId,
        pointer_x: Pixels,
        include_item: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> ContextTarget {
        let raw_start = self.pointer_frame(f32::from(pointer_x), window, cx);
        let start = if window.modifiers().alt {
            raw_start
        } else {
            self.snap_frame(raw_start, None, &[], cx)
        };
        let item = if include_item {
            self.editor
                .read(cx)
                .items_on_layer(layer)
                .into_iter()
                .rev()
                .find(|item| item.start <= raw_start && raw_start < item.end_exclusive())
                .map(|item| item.id)
        } else {
            None
        };
        ContextTarget { layer, start, item }
    }

    fn layer_at_cursor(&self, position: gpui::Point<Pixels>, cx: &Context<Self>) -> LayerId {
        let scroll = layer_scroll_base(&self.layer_scroll);
        if scroll.bounds().contains(&position) {
            let content_y = f32::from(position.y - scroll.bounds().origin.y - scroll.offset().y);
            LayerId::new((content_y / self.viewport.layer_height).floor().max(0.) as u64)
        } else {
            let selected_layer = {
                let editor = self.editor.read(cx);
                editor
                    .selected_item()
                    .and_then(|item| editor.item_layer(item.id))
            };
            self.cursor_layer
                .or(selected_layer)
                .unwrap_or_else(|| LayerId::new(0))
        }
    }

    fn item_picker_entries(&self, cx: &Context<Self>) -> Vec<SearchPickerEntry<ItemPickerTarget>> {
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
        if self.can_paste_items(cx) {
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
        entries
    }

    fn open_item_picker(
        &mut self,
        position: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = self.item_picker_entries(cx);
        let timeline = cx.entity();
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
        self.context_menu = Some(TimelineContextMenu {
            content: TimelineContextMenuContent::ItemPicker(picker),
            position,
        });
        cx.notify();
    }

    pub(crate) fn open_item_picker_at_cursor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let position = window.mouse_position();
        let layer = self.layer_at_cursor(position, cx);
        let target = self.context_target_at(layer, position.x, false, window, cx);
        self.cursor_layer = Some(layer);
        self.context_target = Some(target);
        self.open_item_picker(position, window, cx);
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
        let action_context = self.focus_handle.clone();
        let content = if let Some(item_id) = target_item {
            let menu = PopupMenu::build(window, cx, move |menu, _, _cx| {
                let copy_timeline = timeline.clone();
                let cut_timeline = timeline.clone();
                let paste_timeline = timeline.clone();
                menu.item(PopupMenuItem::new("コピー").on_click(move |_, _, cx| {
                    copy_timeline.update(cx, |timeline, cx| {
                        timeline.copy_selected_items(cx);
                    });
                }))
                .item(PopupMenuItem::new("切り取り").on_click(move |_, _, cx| {
                    cut_timeline.update(cx, |timeline, cx| {
                        timeline.cut_selected_items(cx);
                    });
                }))
                .when(can_paste, |menu| {
                    menu.item(PopupMenuItem::new("貼り付け").on_click(move |_, _, cx| {
                        paste_timeline.update(cx, |timeline, cx| {
                            timeline.paste_items_at(paste_target, cx);
                        });
                    }))
                })
                .separator()
                .when(can_group, |menu| {
                    let group_timeline = timeline.clone();
                    menu.item(
                        PopupMenuItem::new("シーンにまとめる").on_click(move |_, _, cx| {
                            group_timeline.update(cx, |timeline, cx| {
                                timeline.group_selected_as_scene(cx);
                            });
                        }),
                    )
                    .separator()
                })
                .item(PopupMenuItem::new("削除").on_click(move |_, _, cx| {
                    timeline.update(cx, |timeline, cx| {
                        timeline.remove_item(item_id, cx);
                    });
                }))
                .action_context(action_context)
            });
            cx.subscribe(&menu, |this, _, _: &DismissEvent, cx| {
                this.context_menu = None;
                cx.notify();
            })
            .detach();
            menu.read(cx).focus_handle(cx).focus(window, cx);
            TimelineContextMenuContent::Commands(menu)
        } else {
            self.open_item_picker(position, window, cx);
            return;
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
            start = self.snap_frame(start, None, &[], cx);
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
                .unwrap_or_else(|| self.snap_frame(raw_pointer, None, &[], cx))
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
        cx.spawn(async move |timeline, cx| {
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
        })
        .detach();
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
        self.reset_viewport();
    }

    fn clear_selection_on_double_click(
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
            self.reset_viewport();
        }
    }

    pub(crate) fn remove_selected_item(&mut self, cx: &mut Context<Self>) {
        self.stop_playback(cx);
        self.editor
            .update_if_changed(cx, TimelineEditor::remove_selected_item);
    }
}
