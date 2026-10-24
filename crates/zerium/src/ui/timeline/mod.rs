//! Timeline pane wiring; rendering and interactions live with their feature modules.
mod clipboard;
mod interaction;
mod playback;
mod render;
mod ruler;
mod scene;
mod stretch;
mod viewport;

use render::timeline_item;
use ruler::{grid_canvas, timeline_grid};
use stretch::stretch_indicator;
use viewport::track_viewport_width;

use viewport::TimelineViewport;

use std::sync::Arc;

use gpui::{
    Context, Entity, FocusHandle, SharedString, SmoothScrollMode, Subscription,
    UniformListScrollHandle,
};

use crate::{
    engine::media::MediaReaderRegistry,
    project_session::{ProjectSession, ProjectSessionId},
};
use zerium_core::timeline::{LayerId, TimelineEditor};

use crate::ui::{
    animation_curve::AnimationSelection, notifications::UiNotifications,
    transport::TransportController,
};

use crate::ui::timeline::interaction::{
    context_menu::{ContextTarget, TimelineContextMenu},
    drop::FileDropTarget,
    item::ItemMoveOrigin,
    selection::MarqueeSelection,
};

const LAYER_HEADER_WIDTH: f32 = 200.;

fn layer_scroll_base(handle: &UniformListScrollHandle) -> gpui::ScrollHandle {
    handle.0.borrow().base_handle.clone()
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
    file_drop_target: Option<FileDropTarget>,
    file_drop_error: Option<SharedString>,
    item_move_origin: Option<ItemMoveOrigin>,
    marquee_selection: Option<MarqueeSelection>,
    context_menu: Option<TimelineContextMenu>,
    scrubbing_playhead: bool,
    animation_drag: Option<interaction::keyframe::AnimationStopDrag>,
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
                this.file_drop_error = None;
                this.reset_viewport();
                this.context_target = None;
                this.cursor_layer = None;
                this.file_drop_target = None;
                this.item_move_origin = None;
                this.marquee_selection = None;
                this.context_menu = None;
                this.animation_drag = None;
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
            file_drop_target: None,
            file_drop_error: None,
            item_move_origin: None,
            marquee_selection: None,
            context_menu: None,
            scrubbing_playhead: false,
            animation_drag: None,
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }
}
