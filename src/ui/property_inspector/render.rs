use super::control::{Control, ControlTree, EffectGroup, GroupKind};
use super::rows::RenderCtx;
use super::*;

pub(super) struct SelectionView {
    pub item: TimelineItem,
    pub item_label: String,
    pub selected_count: usize,
    pub aspect_ratio_lock: Option<AspectRatioLockState>,
    pub tree: ControlTree,
    pub scene_arguments: Vec<SceneArgumentOption>,
    pub file_inputs: Vec<(FileCapability, Option<MediaAsset>)>,
    pub available_effects: Vec<SearchPickerEntry<(String, String)>>,
    pub multiple: bool,
    pub editing_scene: bool,
    pub has_visual: bool,
    pub items_hidden: bool,
    pub item_visibility_mixed: bool,
    pub kind_label: String,
}

impl Render for PropertyInspector {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let selected = self.selected_view(cx);

        div()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<PropertyValueDrag>, window, cx| {
                    let drag = event.drag(cx).clone();
                    cx.set_active_drag_cursor_style(CursorStyle::ResizeLeftRight, window);
                    this.handle_value_drag(
                        &drag,
                        f32::from(event.event.position.x),
                        event.event.modifiers.shift,
                        window,
                        cx,
                    );
                },
            ))
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<SceneArgumentValueDrag>, window, cx| {
                    let drag = event.drag(cx).clone();
                    cx.set_active_drag_cursor_style(CursorStyle::ResizeLeftRight, window);
                    this.handle_scene_argument_value_drag(
                        &drag,
                        f32::from(event.event.position.x),
                        event.event.modifiers.shift,
                        window,
                        cx,
                    );
                },
            ))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.finish_value_drag(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.finish_value_drag(cx)),
            )
            .bg(colors.background)
            .text_color(colors.foreground)
            .when_some(selected, |this, selected| {
                this.child(self.selected_view_element(selected, cx))
            })
    }
}

impl PropertyInspector {
    fn render_context<'a>(
        &'a self,
        selection: &'a SelectionView,
        cx: &mut Context<Self>,
    ) -> RenderCtx<'a> {
        let active_scene_name_input = self.editor.read(cx).active_scene_id().and_then(|scene_id| {
            self.store
                .states
                .get(&ControlId::scene_name(scene_id))
                .and_then(state::ControlState::text)
                .map(|state| state.input.clone())
        });
        RenderCtx {
            colors: cx.theme().colors,
            editor: &self.editor,
            inspector: cx.entity(),
            focus_handle: &self.focus_handle,
            store: &self.store,
            font_names: &self.font_names,
            item_id: selection.item.id,
            active_scene_name_input,
        }
    }

    fn selected_view(&self, cx: &mut Context<Self>) -> Option<SelectionView> {
        let selected_items = self.editor.read(cx).selected_items();
        let item = selected_items.first()?.clone();
        let item_label = self
            .editor
            .read(cx)
            .item_label(item.id)
            .unwrap_or_else(|| "不明なアイテム".to_owned());
        let multiple = selected_items.len() > 1;
        let hidden_state = self.editor.read(cx).selected_items_hidden_state();
        let schema = Self::selected_schema(&item);
        let scene_arguments = self.active_scene_argument_options(cx);
        let editing_scene = self.editor.read(cx).active_scene_id().is_some() && !multiple;
        let effects = if multiple {
            Self::common_effects(&selected_items)
        } else {
            item.effects.clone()
        };
        let has_visual = schema
            .is_some_and(|schema| schema.visual().is_some() && (!multiple || !effects.is_empty()))
            || (item.scene_id().is_some() && !multiple);
        let file_inputs = (!multiple)
            .then_some(schema)
            .flatten()
            .map(|schema| {
                schema
                    .files()
                    .iter()
                    .map(|input| (input.clone(), item.media(input.id()).cloned()))
                    .collect()
            })
            .unwrap_or_default();
        let kind_label = if multiple {
            "複数".to_owned()
        } else if item.scene_id().is_some() {
            "シーン".to_owned()
        } else {
            schema
                .map(|schema| schema.label().to_owned())
                .unwrap_or_default()
        };
        let aspect_ratio_lock =
            Self::aspect_ratio_lock_state(&item, &selected_items, &scene_arguments, editing_scene);
        let available_effects = plugins()
            .effects()
            .map(|(plugin_id, effect)| {
                SearchPickerEntry::from_plugin_schema(
                    plugin_id,
                    effect,
                    (plugin_id.to_owned(), effect.id().to_owned()),
                )
            })
            .collect();

        Some(SelectionView {
            item,
            item_label,
            selected_count: selected_items.len(),
            aspect_ratio_lock,
            tree: self.store.tree.clone(),
            scene_arguments,
            file_inputs,
            available_effects,
            multiple,
            editing_scene,
            has_visual,
            items_hidden: hidden_state == Some(true),
            item_visibility_mixed: hidden_state.is_none() && multiple,
            kind_label,
        })
    }

    pub(super) fn active_scene_argument_options(
        &self,
        cx: &Context<Self>,
    ) -> Vec<SceneArgumentOption> {
        let editor = self.editor.read(cx);
        let Some(scene_id) = editor.active_scene_id() else {
            return Vec::new();
        };
        let Some(scene) = editor.scene(scene_id) else {
            return Vec::new();
        };

        scene
            .arguments
            .iter()
            .map(|argument| {
                let label = if argument.schema.label().is_empty() {
                    argument.schema.id().to_owned()
                } else {
                    argument.schema.label().to_owned()
                };
                let referenced_by_derived = scene.arguments.iter().any(|other| {
                    other.schema.id() != argument.schema.id()
                        && other.derived_expression_references(argument.schema.id())
                });
                SceneArgumentOption {
                    scene_id,
                    id: argument.schema.id().to_owned(),
                    label,
                    schema: argument.schema.parameter().clone(),
                    binding_count: argument.bindings.len(),
                    bindings: argument.bindings.clone(),
                    derived: argument.is_derived(),
                    referenced_by_derived,
                }
            })
            .collect()
    }

    fn selected_view_element(
        &self,
        view: SelectionView,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let render = self.render_context(&view, cx);
        let header = Self::selection_header(&view, &render);
        let scene_settings = view
            .editing_scene
            .then(|| self.scene_settings_element(&view.scene_arguments, &render));
        let controls = view
            .tree
            .roots
            .iter()
            .cloned()
            .filter_map(|control| self.control_root_element(control, &view, &render, cx))
            .collect::<Vec<_>>();
        let files = view
            .file_inputs
            .iter()
            .cloned()
            .map(|file| self.file_input_element(file, &render))
            .collect::<Vec<_>>();
        let effect_picker = if view.has_visual && !view.multiple {
            Some(Self::add_effect_picker(
                view.available_effects.clone(),
                render.inspector.clone(),
            ))
        } else {
            None
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .id("property-inspector-scroll")
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .gap_3()
                    .p_3()
                    .when_some(scene_settings, |this, section| this.child(section))
                    .when(view.editing_scene, |this| {
                        this.child(
                            div()
                                .w_full()
                                .h(px(1.))
                                .flex_none()
                                .bg(render.colors.border),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(render.colors.muted_foreground)
                                .child("アイテム設定"),
                        )
                    })
                    .child(Self::kind_row(view.kind_label.clone(), render.colors))
                    .children(controls)
                    .children(files)
                    .when_some(self.file_error.clone(), |this, error| {
                        this.child(
                            div()
                                .w_full()
                                .text_sm()
                                .text_color(render.colors.danger)
                                .child(error),
                        )
                    })
                    .when_some(effect_picker, |this, picker| {
                        this.child(
                            div()
                                .w_full()
                                .h(px(1.))
                                .flex_none()
                                .bg(render.colors.border),
                        )
                        .child(picker)
                    }),
            )
            .into_any_element()
    }

    fn control_root_element(
        &self,
        control: Control,
        view: &SelectionView,
        render: &RenderCtx<'_>,
        cx: &Context<Self>,
    ) -> Option<gpui::AnyElement> {
        match control {
            Control::Group {
                children,
                kind: GroupKind::Effect(effect),
                ..
            } => Some(self.effect_element(effect, children, view, render, cx)),
            control => Self::control_element(control, view.aspect_ratio_lock, render),
        }
    }

    fn control_element(
        control: Control,
        aspect: Option<AspectRatioLockState>,
        render: &RenderCtx,
    ) -> Option<gpui::AnyElement> {
        match control {
            Control::Group {
                label,
                children,
                kind,
                ..
            } => match kind {
                GroupKind::Plain => Some(Self::group_box(label, &children, None, aspect, render)),
                GroupKind::Tuple { size_key } => {
                    Some(Self::group_box(label, &children, size_key, aspect, render))
                }
                GroupKind::Array(array) => Some(Self::array_section(
                    &array,
                    &children,
                    render,
                    render.colors.border,
                    true,
                )),
                GroupKind::Effect(_) => None,
            },
            leaf => Self::scalar_full_row(&leaf, render),
        }
    }

    fn selection_header(view: &SelectionView, render: &RenderCtx<'_>) -> Div {
        let editor = render.editor.clone();
        pane_header(render.colors)
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(if view.multiple {
                        format!("{}個のアイテム", view.selected_count)
                    } else {
                        view.item_label.clone()
                    }),
            )
            .child(
                Button::new("toggle-selected-item-visibility")
                    .small()
                    .compact()
                    .ghost()
                    .icon(if view.items_hidden {
                        IconName::EyeOff
                    } else if view.item_visibility_mixed {
                        IconName::EyeClosed
                    } else {
                        IconName::Eye
                    })
                    .tooltip(if view.items_hidden {
                        "選択アイテムを表示"
                    } else if view.item_visibility_mixed {
                        "表示状態が混在しています。すべて非表示"
                    } else {
                        "選択アイテムを非表示"
                    })
                    .on_click(move |_, _, cx| {
                        editor.update(cx, |editor, cx| {
                            if editor.toggle_selected_items_visibility() {
                                cx.notify();
                            }
                        });
                    }),
            )
    }

    fn kind_row(label: String, colors: ThemeColor) -> Div {
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_3()
            .child(Self::parameter_label_column("種類"))
            .child(
                div()
                    .text_sm()
                    .text_color(colors.muted_foreground)
                    .child(label),
            )
    }

    fn file_input_element(
        &self,
        (input, media): (FileCapability, Option<MediaAsset>),
        render: &RenderCtx<'_>,
    ) -> gpui::AnyElement {
        let input_id = input.id().to_owned();
        let choose_input_id = input_id.clone();
        let inspector = render.inspector.clone();
        let button_label = if self.loading_file {
            "読み込み中…"
        } else if media.is_some() {
            "ファイルを変更"
        } else {
            "ファイルを選択"
        };
        let details = media.as_ref().map(Self::media_details);

        div()
            .w_full()
            .flex()
            .items_start()
            .gap_3()
            .child(Self::parameter_label_column(input.label().to_owned()))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        Button::new(SharedString::from(format!("select-item-file-{input_id}")))
                            .small()
                            .label(button_label)
                            .disabled(self.loading_file)
                            .on_click(move |event, window, cx| {
                                inspector.update(cx, |inspector, cx| {
                                    inspector.choose_file(
                                        choose_input_id.clone(),
                                        event,
                                        window,
                                        cx,
                                    );
                                });
                            }),
                    )
                    .when_some(details, |this, details| {
                        this.child(
                            div()
                                .whitespace_normal()
                                .text_sm()
                                .text_color(render.colors.muted_foreground)
                                .child(details),
                        )
                    }),
            )
            .into_any_element()
    }

    fn media_details(media: &MediaAsset) -> String {
        let duration = media.duration.as_secs_f64();
        let minutes = (duration / 60.).floor() as u64;
        let seconds = duration - minutes as f64 * 60.;
        let format = match &media.kind {
            MediaKind::Video {
                width,
                height,
                frame_rate,
                has_audio,
                ..
            } => format!(
                "{width} × {height}・{:.3} fps{}",
                frame_rate.frames_per_second(),
                if *has_audio { "・音声あり" } else { "" }
            ),
            MediaKind::Audio {
                channels,
                sample_rate,
            } => match (channels, sample_rate) {
                (Some(channels), Some(sample_rate)) => format!("{channels} ch・{sample_rate} Hz"),
                (Some(channels), None) => format!("{channels} ch"),
                (None, Some(sample_rate)) => format!("{sample_rate} Hz"),
                (None, None) => "音声ストリーム".to_owned(),
            },
            MediaKind::Image { width, height } => format!("{width} × {height}・画像"),
        };
        format!(
            "{minutes:02}:{seconds:06.3}・{format}\n{}",
            media.path.display()
        )
    }

    fn effect_element(
        &self,
        effect: EffectGroup,
        controls: Vec<Control>,
        view: &SelectionView,
        render: &RenderCtx<'_>,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        let effect_id = effect.id;
        let hidden = effect.hidden;
        let (can_move_up, can_move_down) = {
            let editor = render.editor.read(cx);
            (
                editor.can_move_selected_effect(effect_id, -1),
                editor.can_move_selected_effect(effect_id, 1),
            )
        };
        let controls = controls
            .into_iter()
            .filter_map(|control| Self::control_element(control, None, render))
            .collect::<Vec<_>>();

        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .pb_3()
            .border_b_1()
            .border_color(render.colors.border)
            .child(Self::effect_header(
                effect.label,
                effect_id,
                hidden,
                can_move_up,
                can_move_down,
                view.multiple,
                render,
            ))
            .children(controls)
            .into_any_element()
    }

    fn effect_header(
        label: String,
        effect_id: EffectInstanceId,
        hidden: bool,
        can_move_up: bool,
        can_move_down: bool,
        multiple: bool,
        render: &RenderCtx<'_>,
    ) -> Div {
        let visibility_editor = render.editor.clone();
        let move_up_editor = render.editor.clone();
        let move_down_editor = render.editor.clone();
        let remove_editor = render.editor.clone();

        div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_sm()
                    .text_color(if hidden {
                        render.colors.muted_foreground
                    } else {
                        render.colors.foreground
                    })
                    .child(label),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        Button::new(SharedString::from(format!(
                            "toggle-effect-visibility-{}",
                            effect_id.get()
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
                            "エフェクトを有効化"
                        } else {
                            "エフェクトを一時的に無効化"
                        })
                        .on_click(move |_, _, cx| {
                            visibility_editor.update(cx, |editor, cx| {
                                if editor.toggle_selected_effect_visibility(effect_id) {
                                    cx.notify();
                                }
                            });
                        }),
                    )
                    .child(
                        Button::new(SharedString::from(format!(
                            "move-effect-up-{}",
                            effect_id.get()
                        )))
                        .small()
                        .compact()
                        .ghost()
                        .icon(IconName::ChevronUp)
                        .tooltip("上へ移動")
                        .disabled(!can_move_up)
                        .on_click(move |_, _, cx| {
                            move_up_editor.update(cx, |editor, cx| {
                                if editor.move_selected_effect(effect_id, -1) {
                                    cx.notify();
                                }
                            });
                        }),
                    )
                    .child(
                        Button::new(SharedString::from(format!(
                            "move-effect-down-{}",
                            effect_id.get()
                        )))
                        .small()
                        .compact()
                        .ghost()
                        .icon(IconName::ChevronDown)
                        .tooltip("下へ移動")
                        .disabled(!can_move_down)
                        .on_click(move |_, _, cx| {
                            move_down_editor.update(cx, |editor, cx| {
                                if editor.move_selected_effect(effect_id, 1) {
                                    cx.notify();
                                }
                            });
                        }),
                    )
                    .when(!multiple, |this| {
                        this.child(
                            Button::new(SharedString::from(format!(
                                "remove-effect-{}",
                                effect_id.get()
                            )))
                            .small()
                            .compact()
                            .label("削除")
                            .on_click(move |_, _, cx| {
                                remove_editor.update(cx, |editor, cx| {
                                    if editor.remove_selected_effect(effect_id) {
                                        cx.notify();
                                    }
                                });
                            }),
                        )
                    }),
            )
    }

    fn add_effect_picker(
        entries: Vec<SearchPickerEntry<(String, String)>>,
        inspector: Entity<Self>,
    ) -> gpui::AnyElement {
        Popover::new("add-effect-picker")
            .trigger(
                Button::new("add-effect")
                    .small()
                    .label("エフェクトを追加")
                    .dropdown_caret(true),
            )
            .content(move |window, cx| {
                let inspector = inspector.clone();
                let entries = entries.clone();
                cx.new(|cx| {
                    SearchPicker::new(
                        entries,
                        "エフェクトを検索",
                        move |(plugin_id, effect_id), _, cx| {
                            inspector.update(cx, |inspector, cx| {
                                let result = inspector.editor.update(cx, |editor, cx| {
                                    let result = editor.add_selected_effect(&plugin_id, &effect_id);
                                    if result.is_ok() {
                                        cx.notify();
                                    }
                                    result
                                });
                                if let Err(error) = result {
                                    inspector.notifications.update(cx, |notifications, cx| {
                                        notifications.push(
                                            format!("エフェクトを追加できません: {error}"),
                                            cx,
                                        );
                                    });
                                }
                            });
                        },
                        window,
                        cx,
                    )
                })
            })
            .into_any_element()
    }
}
