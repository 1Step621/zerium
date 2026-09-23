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
    app::project_session::{ProjectActivity, ProjectSession, ProjectSessionId},
    domain::timeline::{
        Frame, FrameDuration, FrameRate, ItemId, LayerId, ResizeEdge, SceneId, TimelineEditError,
        TimelineEditor, TimelineItem, TimelineTime,
    },
    engine::media::MediaReaderRegistry,
};

use super::{
    TimelineEditorEntityExt as _,
    animation_curve::{AnimationSelection, AnimationTarget},
    explorer::ExplorerFileDrag,
    pane::PANE_HEADER_HEIGHT,
    search_picker::{SearchPicker, SearchPickerEntry},
    session::UiNotifications,
    time_grid,
    transport::{ScrubSource, TransportController},
};

mod clipboard;
mod interaction;
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
