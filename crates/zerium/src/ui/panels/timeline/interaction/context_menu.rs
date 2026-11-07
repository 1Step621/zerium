use rust_i18n::t;

use ::ui::{IconName, ThemeColor, menu::PopupMenu};
use gpui::{
    Context, Corner, DismissEvent, Entity, Focusable as _, Pixels, Window, anchored, deferred, div,
    prelude::*, px,
};

use zerium_core::timeline::{
    Frame, ItemId, LayerId, PropertyAddress, SceneId, TimelineEditError, TimelineTime,
};

use crate::{
    app::actions::SplitSelectedItems,
    ui::{
        TimelineEditorEntityExt as _,
        components::search_picker::{SearchPicker, SearchPickerEntry},
        panels::timeline::{Timeline, TimelineRow, layer_scroll_base},
    },
};

impl Timeline {
    pub(in crate::ui::panels::timeline) fn open_animation_context_menu(
        &mut self,
        address: &PropertyAddress,
        stop: Option<usize>,
        position: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let time = TimelineTime::from_frame(self.pointer_frame(f32::from(position.x), window, cx));
        let editor = self.editor.read(cx);
        if stop.is_none()
            && editor
                .item(address.item_id)
                .zip(editor.animation_track(address))
                .is_none_or(|(item, track)| {
                    !item
                        .animation_clock(track)
                        .pattern_time_range()
                        .contains(&time)
                })
        {
            return;
        }
        self.edit_stops(cx);
        let enabled = if let Some(stop) = stop {
            let removable = self
                .editor
                .read(cx)
                .animation_track(address)
                .is_some_and(|track| stop > 0 && stop + 1 < track.stops().len());
            if removable && !self.edit_stops(cx).keys.contains(&(address.clone(), stop)) {
                self.select_stop(address.clone(), stop, false, cx);
            }
            removable
        } else {
            self.editor
                .read(cx)
                .animation_stop_insertion_frame(address, time)
                .is_some()
        };
        let context = self.editor.read(cx).edit_context();
        let target = address.clone();
        let timeline = cx.entity();
        let action_context = self.focus_handle.clone();
        let menu = PopupMenu::build(window, cx, move |menu, window, cx| {
            let menu = menu.action_context(action_context);
            if stop.is_some() {
                menu.menu_handler_with_icon_and_disabled(
                    t!("curve.delete_stop").to_string(),
                    IconName::Delete,
                    !enabled,
                    move |_, cx| {
                        timeline.update(cx, |this, cx| {
                            if context.is_current(this.editor.read(cx)) {
                                this.delete_selected_stops(cx);
                            }
                        });
                    },
                )
            } else {
                crate::ui::components::animation::animation_stop_add_menu(
                    menu,
                    enabled,
                    window,
                    cx,
                    move |source, cx| {
                        timeline.update(cx, |this, cx| {
                            if context.is_current(this.editor.read(cx)) {
                                this.add_animation_stop(&target, time, source, cx);
                            }
                        });
                    },
                )
            }
        });
        let stop_insertion = stop
            .is_none()
            .then(|| (address.clone(), time.nearest_frame()));
        self.show_context_menu(menu, position, stop_insertion, window, cx);
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
        if let Some(item_id) = target.item {
            if !self.editor.read(cx).is_item_selected(item_id) {
                self.editor
                    .update_if_changed(cx, |editor| editor.select(item_id));
            }
            self.animation_selection.update(cx, |selection, cx| {
                selection.select_item(item_id, &self.editor, cx);
            });
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
            self.snap_frame(raw_start, &[], cx)
        };
        let item = if include_item {
            self.editor
                .read(cx)
                .items_on_layer(layer)
                .into_iter()
                .rev()
                .find(|item| {
                    item.start() <= raw_start
                        && raw_start < item.end_exclusive(self.editor.read(cx).frame_rate())
                })
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
            self.layer_at_content_y(content_y)
        } else {
            let selected_layer = self.editor.read(cx).selected_items_layer();
            self.cursor_layer
                .or(selected_layer)
                .unwrap_or_else(|| LayerId::new(0))
        }
    }

    fn item_picker_entries(&self, cx: &Context<Self>) -> Vec<SearchPickerEntry<ItemPickerTarget>> {
        let mut entries = self
            .editor
            .read(cx)
            .plugin_registry()
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
                SearchPickerEntry::new(
                    t!("context.paste").to_string(),
                    t!("context.clipboard").to_string(),
                    ItemPickerTarget::Paste,
                )
                .icon(IconName::PasteClipboard)
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
                        t!("context.scene").to_string(),
                        ItemPickerTarget::Scene(scene.id),
                    )
                    .icon(IconName::Frame)
                    .search_terms(["scene"])
                }),
        );
        entries
    }

    pub(in crate::ui::panels::timeline) fn open_item_picker(
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
                t!("context.search_item").to_string(),
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
            stop_insertion: None,
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
        let target = if layer_scroll_base(&self.layer_scroll)
            .bounds()
            .contains(&position)
        {
            self.context_target_at(layer, position.x, false, window, cx)
        } else {
            ContextTarget {
                layer,
                start: self.editor.read(cx).playhead(),
                item: None,
            }
        };
        self.cursor_layer = Some(layer);
        self.context_target = Some(target);
        self.open_item_picker(position, window, cx);
    }

    pub(in crate::ui::panels::timeline) fn open_context_menu(
        &mut self,
        layer_index: usize,
        pointer_x: Pixels,
        position: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(TimelineRow::Property(row)) = self.row_at_position(position) {
            let address = row.address.clone();
            self.open_animation_context_menu(&address, None, position, window, cx);
            return;
        }
        self.prepare_context_target(layer_index, pointer_x, window, cx);
        let target_item = self.context_target.and_then(|target| target.item);
        let can_group = self.editor.read(cx).selected_item_ids().count() >= 1;
        let can_paste = self.can_paste_items(cx);
        let can_split = self.can_split_selected_items(cx);
        let paste_target = self
            .context_target
            .map(|target| (target.layer, target.start));
        let timeline = cx.entity();
        let action_context = self.focus_handle.clone();
        let menu = if let Some(item_id) = target_item {
            PopupMenu::build(window, cx, move |menu, _, _cx| {
                let copy_timeline = timeline.clone();
                let cut_timeline = timeline.clone();
                let paste_timeline = timeline.clone();
                menu.menu_handler_with_icon(
                    t!("context.copy").to_string(),
                    IconName::Copy,
                    move |_, cx| {
                        copy_timeline.update(cx, |timeline, cx| {
                            timeline.copy_selected_items(cx);
                        });
                    },
                )
                .menu_handler_with_icon(
                    t!("context.cut").to_string(),
                    IconName::Cut,
                    move |_, cx| {
                        cut_timeline.update(cx, |timeline, cx| {
                            timeline.cut_selected_items(cx);
                        });
                    },
                )
                .when(can_paste, |menu| {
                    menu.menu_handler_with_icon(
                        t!("context.paste").to_string(),
                        IconName::PasteClipboard,
                        move |_, cx| {
                            paste_timeline.update(cx, |timeline, cx| {
                                timeline.paste_items_at(paste_target, cx);
                            });
                        },
                    )
                })
                .separator()
                .menu_with_icon_and_disabled(
                    t!("menu.split_items").to_string(),
                    IconName::VerticalSplit,
                    Box::new(SplitSelectedItems),
                    !can_split,
                )
                .separator()
                .when(can_group, |menu| {
                    let group_timeline = timeline.clone();
                    menu.menu_handler_with_icon(
                        t!("context.group_scene").to_string(),
                        IconName::Group,
                        move |_, cx| {
                            group_timeline.update(cx, |timeline, cx| {
                                timeline.group_selected_as_scene(cx);
                            });
                        },
                    )
                    .separator()
                })
                .menu_handler_with_icon(
                    t!("common.delete").to_string(),
                    IconName::Delete,
                    move |_, cx| {
                        timeline.update(cx, |timeline, cx| {
                            timeline.remove_item(item_id, cx);
                        });
                    },
                )
                .action_context(action_context)
            })
        } else {
            self.open_item_picker(position, window, cx);
            return;
        };
        self.show_context_menu(menu, position, None, window, cx);
    }

    fn show_context_menu(
        &mut self,
        menu: Entity<PopupMenu>,
        position: gpui::Point<Pixels>,
        stop_insertion: Option<(PropertyAddress, Frame)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe(&menu, |this, _, _: &DismissEvent, cx| {
            this.context_menu = None;
            cx.notify();
        })
        .detach();
        menu.read(cx).focus_handle(cx).focus(window, cx);
        let content = TimelineContextMenuContent::Commands(menu);
        self.context_menu = Some(TimelineContextMenu {
            content,
            position,
            stop_insertion,
        });
        cx.notify();
    }

    pub(in crate::ui::panels::timeline) fn context_menu_overlay(
        &self,
        colors: ThemeColor,
        timeline: Entity<Self>,
    ) -> Option<impl IntoElement> {
        let context_menu = self.context_menu.as_ref()?;
        let (content, dismiss_on_outside) = match &context_menu.content {
            TimelineContextMenuContent::Commands(menu) => (menu.clone().into_any_element(), false),
            TimelineContextMenuContent::ItemPicker(picker) => {
                (picker.clone().into_any_element(), true)
            }
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
                            .when(dismiss_on_outside, |overlay| {
                                overlay.on_mouse_down_out(move |_, _, cx| {
                                    timeline.update(cx, |timeline, cx| {
                                        timeline.context_menu = None;
                                        cx.notify();
                                    });
                                })
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
            let target = crate::app::defaults::DefaultsTarget::Item {
                plugin_id: plugin_id.to_owned(),
                item_id: item_id.to_owned(),
            };
            let properties = editor
                .plugin_registry()
                .item(plugin_id, item_id)
                .map(|schema| crate::app::defaults::properties(cx, &target, schema.properties()))
                .unwrap_or_default();
            let item = editor.add_item(layer, start, plugin_id, item_id, properties);
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
                        notifications
                            .push(t!("context.add_item_failed", error = error).to_string(), cx);
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
                        notifications.push(
                            t!("context.add_scene_failed", error = error).to_string(),
                            cx,
                        );
                    });
                }
            }
            ItemPickerTarget::Paste => {
                self.paste_items_at(Some((target.layer, target.start)), cx);
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(in crate::ui::panels::timeline) struct ContextTarget {
    pub(in crate::ui::panels::timeline) layer: LayerId,
    pub(in crate::ui::panels::timeline) start: Frame,
    pub(in crate::ui::panels::timeline) item: Option<ItemId>,
}

#[derive(Clone)]
pub(in crate::ui::panels::timeline) struct TimelineContextMenu {
    pub(in crate::ui::panels::timeline) content: TimelineContextMenuContent,
    pub(in crate::ui::panels::timeline) position: gpui::Point<Pixels>,
    pub(in crate::ui::panels::timeline) stop_insertion: Option<(PropertyAddress, Frame)>,
}

#[derive(Clone)]
pub(in crate::ui::panels::timeline) enum TimelineContextMenuContent {
    Commands(Entity<PopupMenu>),
    ItemPicker(Entity<SearchPicker<ItemPickerTarget>>),
}

#[derive(Clone)]
pub(in crate::ui::panels::timeline) enum ItemPickerTarget {
    Plugin { plugin_id: String, item_id: String },
    Scene(SceneId),
    Paste,
}
