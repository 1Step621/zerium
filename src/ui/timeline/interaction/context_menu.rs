use super::*;

impl Timeline {
    pub(super) fn prepare_context_target(
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

    pub(super) fn context_target_at(
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

    pub(super) fn layer_at_cursor(
        &self,
        position: gpui::Point<Pixels>,
        cx: &Context<Self>,
    ) -> LayerId {
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

    pub(super) fn item_picker_entries(
        &self,
        cx: &Context<Self>,
    ) -> Vec<SearchPickerEntry<ItemPickerTarget>> {
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

    pub(super) fn open_item_picker(
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

    pub(super) fn open_context_menu(
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

    pub(in crate::ui::timeline) fn context_menu_overlay(
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

    pub(super) fn add_item_at(
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

    pub(super) fn add_picker_item(&mut self, item: ItemPickerTarget, cx: &mut Context<Self>) {
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
}
