//! Timeline pane wiring; rendering and interactions live with their feature modules.
mod interaction;
mod render;
mod viewport;

use self::{
    interaction::{
        context_menu::{ContextTarget, TimelineContextMenu},
        drop::FileDropTarget,
        item::ItemMoveOrigin,
        selection::MarqueeSelection,
        stretch::stretch_indicator,
    },
    render::{
        ruler::{grid_canvas, timeline_grid},
        timeline_item,
    },
    viewport::{TimelineViewport, track_viewport_width},
};

use std::sync::Arc;

use gpui::{
    Context, Entity, FocusHandle, SharedString, SmoothScrollMode, Subscription,
    UniformListScrollHandle,
};

use crate::{
    app::{
        project::{ProjectSession, ProjectSessionId},
        transport::TransportController,
    },
    engine::input::MediaReaderRegistry,
    ui::workspace::notifications::UiNotifications,
};
use zerium_core::timeline::{LayerId, TimelineEditor};

use super::animation_curve::AnimationSelection;

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
        let mut document = editor.read(cx).active_scene_id();
        let layer_scroll = UniformListScrollHandle::new();
        layer_scroll
            .0
            .borrow_mut()
            .smooth_scroll
            .set_mode(SmoothScrollMode::Disabled);
        let subscriptions = vec![
            cx.observe(&editor, move |this, editor, cx| {
                let next = editor.read(cx).active_scene_id();
                if document != next {
                    document = next;
                    this.reset_document_state(cx);
                }
                cx.notify();
            }),
            cx.observe(&transport, |_, _, cx| cx.notify()),
            cx.observe(&animation_selection, |_, _, cx| cx.notify()),
            cx.observe(&session, |this, _, cx| {
                let session_id = this.session.read(cx).id();
                if session_id == this.session_id {
                    return;
                }
                this.session_id = session_id;
                this.reset_document_state(cx);
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
            animation_drag: None,
            focus_handle: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    fn reset_document_state(&mut self, cx: &mut Context<Self>) {
        self.finish_playhead_scrub(cx);
        self.reset_viewport();
        self.file_drop_error = None;
        self.context_target = None;
        self.cursor_layer = None;
        self.file_drop_target = None;
        self.item_move_origin = None;
        self.marquee_selection = None;
        self.context_menu = None;
        self.animation_drag = None;
    }
}
