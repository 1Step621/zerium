//! Direct editing of text-bearing items on one timeline layer.
mod editing;
mod render;
mod state;
mod target;

use ::ui::input::InputEvent;
use gpui::{App, Context, Entity, FocusHandle, ScrollHandle, Subscription, Window, prelude::*};
use rust_i18n::t;
use zerium_core::{
    property::PropertyValue,
    timeline::{Frame, ItemCreation, ItemId, LayerId, PropertyAddress, SceneId, TimelineEditor},
};

use crate::{
    app::{
        project::{ProjectSession, ProjectSessionId},
        transport::TransportController,
    },
    ui::{
        components::input::{InputStore, TextOptions, TextSync},
        workspace::notifications::UiNotifications,
    },
};
use target::Target;

/// A read projection; text input state is held separately by the shared input store.
struct Row {
    address: PropertyAddress,
    start: Frame,
    end: Frame,
    text: String,
    editable: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StampTarget {
    Start(ItemId),
    End(ItemId),
    Done(ItemId),
}

impl StampTarget {
    fn item(self) -> ItemId {
        match self {
            Self::Start(id) | Self::End(id) | Self::Done(id) => id,
        }
    }
}

pub(crate) struct TextPlacement {
    editor: Entity<TimelineEditor>,
    transport: Entity<TransportController>,
    session: Entity<ProjectSession>,
    notifications: Entity<UiNotifications>,
    session_id: ProjectSessionId,
    scene_id: SceneId,
    layer: Option<LayerId>,
    target: Option<Target>,
    inputs: InputStore<PropertyAddress>,
    stamp_target: Option<StampTarget>,
    error: Option<String>,
    focus: FocusHandle,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl TextPlacement {
    pub(crate) fn new(
        editor: Entity<TimelineEditor>,
        transport: Entity<TransportController>,
        session: Entity<ProjectSession>,
        notifications: Entity<UiNotifications>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![
            cx.observe(&editor, |this, _, cx| {
                this.sync_scope(cx);
                cx.notify();
            }),
            cx.observe(&session, |this, _, cx| {
                this.sync_scope(cx);
                cx.notify();
            }),
        ];
        let target = Target::available(editor.read(cx))
            .into_iter()
            .next()
            .map(|(target, _)| target);
        Self {
            session_id: session.read(cx).id(),
            scene_id: editor.read(cx).active_scene_id(),
            editor,
            transport,
            session,
            notifications,
            layer: None,
            target,
            inputs: InputStore::default(),
            stamp_target: None,
            error: None,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    pub(crate) fn open_layer(&mut self, layer: LayerId, cx: &mut Context<Self>) {
        self.sync_scope(cx);
        let changed = self.layer != Some(layer);
        self.layer = Some(layer);
        let editor = self.editor.read(cx);
        let items = editor.items_on_layer(layer);
        let current_matches = self.target.as_ref().is_some_and(|target| {
            target.schema(editor).is_some() && items.iter().any(|item| target.matches(item))
        });
        if !current_matches {
            let targets = Target::available(editor);
            let mut matching = targets
                .iter()
                .filter(|(target, _)| items.iter().any(|item| target.matches(item)));
            let preferred = matching
                .clone()
                .find(|(target, _)| {
                    items.iter().any(|item| {
                        target.matches(item) && item.properties.get(&target.property).is_some()
                    })
                })
                .or_else(|| matching.next());
            if let Some((target, _)) = preferred {
                self.target = Some(target.clone());
            }
        }
        if changed {
            self.inputs = InputStore::default();
            self.stamp_target = None;
            self.scroll.scroll_to_item(0);
        }
        self.error = None;
        cx.notify();
    }

    pub(crate) fn prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self
            .target
            .clone()
            .filter(|target| target.schema(self.editor.read(cx)).is_some())
        else {
            return;
        };
        let (session_id, scene_id, layer) = (self.session_id, self.scene_id, self.layer);
        let selection = rfd::AsyncFileDialog::new()
            .set_parent(window)
            .set_title(t!("text_placement.title").to_string())
            .add_filter(t!("text_placement.text_files").to_string(), &["txt"])
            .pick_file();
        cx.spawn_in(window, async move |sheet, cx| {
            let Some(file) = selection.await else {
                return;
            };
            let path = file.path().to_path_buf();
            let result = cx
                .background_spawn(async move {
                    std::fs::read_to_string(path).map_err(|error| error.to_string())
                })
                .await;
            sheet
                .update_in(cx, |this, _, cx| {
                    if this.session.read(cx).id() != session_id
                        || this.editor.read(cx).active_scene_id() != scene_id
                        || this.layer != layer
                        || this.target.as_ref() != Some(&target)
                    {
                        return;
                    }
                    if let Err(error) = result.and_then(|text| this.load_text(&text, &target, cx)) {
                        this.notifications.update(cx, |notifications, cx| {
                            notifications.push(
                                t!("text_placement.read_failed", error = error).to_string(),
                                cx,
                            )
                        });
                    }
                })
                .ok();
        })
        .detach();
    }
}
