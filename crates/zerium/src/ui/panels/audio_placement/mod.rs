//! Start-time placement of a layer's audio files.
mod import;
mod render;

use std::path::PathBuf;

use gpui::{App, Context, Entity, FocusHandle, ScrollHandle, Subscription, Window, prelude::*};
use zerium_core::{
    media::{MediaInputReference, MediaSource},
    plugin::ItemSchema,
    timeline::{
        Frame, FrameDuration, ItemId, ItemInterval, LayerId, SceneId, TimelineEditError,
        TimelineEditor,
    },
};

use crate::app::{
    project::{ProjectOperation, ProjectSession, ProjectSessionId},
    transport::TransportController,
};

struct Row {
    id: ItemId,
    start: Frame,
    end: Frame,
    path: PathBuf,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StampTarget {
    Start(ItemId),
    Done(ItemId),
}

impl StampTarget {
    fn item(self) -> ItemId {
        match self {
            Self::Start(id) | Self::Done(id) => id,
        }
    }
}

pub(crate) struct AudioPlacement {
    editor: Entity<TimelineEditor>,
    transport: Entity<TransportController>,
    session: Entity<ProjectSession>,
    session_id: ProjectSessionId,
    scene_id: SceneId,
    layer: Option<LayerId>,
    stamp_target: Option<StampTarget>,
    import_task: Option<(ProjectOperation, gpui::Task<()>)>,
    error: Option<String>,
    focus: FocusHandle,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl AudioPlacement {
    pub(crate) fn new(
        editor: Entity<TimelineEditor>,
        transport: Entity<TransportController>,
        session: Entity<ProjectSession>,
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
        Self {
            session_id: session.read(cx).id(),
            scene_id: editor.read(cx).active_scene_id(),
            editor,
            transport,
            session,
            layer: None,
            stamp_target: None,
            import_task: None,
            error: None,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    fn finish_import(&mut self, cx: &mut Context<Self>) {
        if let Some((operation, _)) = self.import_task.take() {
            self.session.update(cx, |session, cx| {
                if session.finish(operation) {
                    cx.notify();
                }
            });
        }
    }

    fn sync_scope(&mut self, cx: &mut Context<Self>) {
        let session_id = self.session.read(cx).id();
        let scene_id = self.editor.read(cx).active_scene_id();
        if (self.session_id, self.scene_id) != (session_id, scene_id) {
            self.session_id = session_id;
            self.scene_id = scene_id;
            self.layer = None;
            self.stamp_target = None;
            self.finish_import(cx);
            self.error = None;
        }
    }

    pub(crate) fn open_layer(&mut self, layer: LayerId, cx: &mut Context<Self>) {
        self.sync_scope(cx);
        if self.layer != Some(layer) {
            self.finish_import(cx);
            self.layer = Some(layer);
            self.stamp_target = None;
            self.scroll.scroll_to_item(0);
        }
        self.error = None;
        cx.notify();
    }

    fn rows(&self, cx: &App) -> Vec<Row> {
        let Some(layer) = self.layer else {
            return Vec::new();
        };
        let editor = self.editor.read(cx);
        let mut rows = editor
            .items_on_layer(layer)
            .into_iter()
            .filter_map(|item| {
                let schema = item.schema()?;
                let source = audio_source(schema)?;
                let address = zerium_core::timeline::PropertyAddress {
                    item_id: item.id,
                    effect_id: None,
                    property_id: source.file.to_owned(),
                    element_id: None,
                    scalar_index: None,
                };
                let value = editor.property_value(&address)?;
                Some(Row {
                    id: item.id,
                    start: item.start(),
                    end: item.end_exclusive(editor.frame_rate()),
                    path: value.file()?.to_owned(),
                })
            })
            .collect::<Vec<_>>();
        rows.sort_by_key(|row| (row.start, row.id.get()));
        rows
    }

    fn current_stamp(&self, rows: &[Row], cx: &App) -> Option<StampTarget> {
        let selected = rows
            .iter()
            .find(|row| self.editor.read(cx).is_item_selected(row.id))
            .or_else(|| rows.first())?;
        match self.stamp_target {
            Some(target) if target.item() == selected.id => Some(target),
            _ => Some(StampTarget::Start(selected.id)),
        }
    }

    pub(crate) fn can_stamp(&self, cx: &App) -> bool {
        matches!(
            self.current_stamp(&self.rows(cx), cx),
            Some(StampTarget::Start(_))
        )
    }

    fn select_row(&mut self, id: ItemId, cx: &mut Context<Self>) {
        let Some(row) = self.rows(cx).into_iter().find(|row| row.id == id) else {
            return;
        };
        self.stamp_target = Some(StampTarget::Start(id));
        self.editor.update(cx, |editor, cx| {
            if editor.select(id) {
                cx.notify();
            }
        });
        self.transport
            .update(cx, |transport, cx| transport.set_playhead(row.start, cx));
        self.error = None;
        cx.notify();
    }

    pub(crate) fn stamp(&mut self, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        let Some(StampTarget::Start(id)) = self.current_stamp(&rows, cx) else {
            return;
        };
        let index = rows.iter().position(|row| row.id == id).expect("stamp row");
        let result = self.stamp_start(&rows[index..], self.editor.read(cx).playhead(), cx);
        match result {
            Ok(()) => {
                let next = rows
                    .get(index + 1)
                    .map_or(StampTarget::Done(id), |row| StampTarget::Start(row.id));
                self.stamp_target = Some(next);
                self.editor.update(cx, |editor, cx| {
                    if editor.select(next.item()) {
                        cx.notify();
                    }
                });
                self.scroll.scroll_to_item((index + 1).min(rows.len() - 1));
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
        cx.notify();
    }

    fn stamp_start(
        &self,
        rows: &[Row],
        frame: Frame,
        cx: &mut Context<Self>,
    ) -> Result<(), TimelineEditError> {
        let mut end = frame;
        let mut updates = Vec::new();
        for (index, row) in rows.iter().enumerate() {
            if index > 0 && row.start >= end {
                break;
            }
            let start = end;
            let duration = FrameDuration::new(row.end.get() - row.start.get())
                .ok_or(TimelineEditError::PlacementUnavailable)?;
            end = Frame::new(
                start
                    .get()
                    .checked_add(duration.get())
                    .ok_or(TimelineEditError::PlacementUnavailable)?,
            );
            updates.push(ItemInterval {
                item_id: row.id,
                start,
                duration,
            });
        }
        self.editor.update(cx, |editor, cx| {
            editor.set_item_intervals(updates)?;
            cx.notify();
            Ok(())
        })
    }

    fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_import(cx);
        let ids = self.rows(cx).iter().map(|row| row.id).collect::<Vec<_>>();
        self.transport
            .update(cx, |transport, cx| transport.stop_in_place(cx));
        self.editor.update(cx, |editor, cx| {
            if editor.remove_items(ids) {
                cx.notify();
            }
        });
        self.stamp_target = None;
        self.error = None;
        self.scroll.scroll_to_item(0);
        self.focus.focus(window, cx);
        cx.notify();
    }
}

fn audio_source(schema: &ItemSchema) -> Option<MediaSource<'_>> {
    if schema.render().is_some() {
        return None;
    }
    let playback = schema.playback()?;
    if let MediaInputReference::Audio(_) = &playback.source {
        schema
            .media_sources()
            .find(|source| source.input == playback.source)
    } else {
        None
    }
}
