use rust_i18n::t;
use zerium_core::timeline::BlendMode;

use super::control::{Control, EffectGroup, GroupKind};
use super::rows::RenderCtx;
use super::*;

impl Render for PropertyInspector {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let item = self
            .inspector_item_id(cx)
            .and_then(|id| self.editor.read(cx).item(id))
            .cloned();

        div()
            .relative()
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_drag_move(cx.listener(Self::handle_number_drag))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.finish_number_drag(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.finish_number_drag(cx)),
            )
            .bg(colors.background)
            .text_color(colors.foreground)
            .when_some(item, |this, item| this.child(self.item_element(&item, cx)))
            .when_some(self.effect_picker.clone(), |this, picker| {
                this.child(
                    div()
                        .absolute()
                        .top(px(32.))
                        .right(px(8.))
                        .bg(colors.popover)
                        .border_1()
                        .border_color(colors.border)
                        .rounded_md()
                        .shadow_md()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.effect_picker = None;
                            cx.notify();
                        }))
                        .child(picker),
                )
            })
    }
}

impl PropertyInspector {
    fn render_context<'a>(&'a self, item: &TimelineItem, cx: &mut Context<Self>) -> RenderCtx<'a> {
        RenderCtx {
            colors: cx.theme().colors,
            editor: &self.editor,
            animation_address: self
                .animation_selection
                .read(cx)
                .address()
                .filter(|address| address.item_id == item.id)
                .cloned(),
            inspector: cx.entity(),
            store: &self.store,
            font_names: &self.font_names,
            selecting_file: self.file_input.read(cx).is_selecting(),
            file_input: self.file_input.clone(),
            scene_overrides: item
                .scene_id()
                .map(|_| {
                    item.properties
                        .iter()
                        .map(|(id, _)| id.to_owned())
                        .collect()
                })
                .unwrap_or_default(),
            item_id: item.id,
        }
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
                SceneArgumentOption {
                    id: argument.schema.id().to_owned(),
                    label,
                    schema: argument.schema.clone(),
                    bindings: argument.bindings.clone(),
                }
            })
            .collect()
    }

    fn item_element(&self, item: &TimelineItem, cx: &mut Context<Self>) -> gpui::AnyElement {
        let selector = self.target_selector(cx);
        let render = self.render_context(item, cx);
        let header = Self::item_header(item, &render, selector, cx);
        let has_visual = self.can_add_effect(cx);
        let mut controls = Vec::new();
        let mut effect_controls = Vec::new();
        for control in self.store.tree.roots.iter().cloned() {
            match control {
                Control::Group {
                    children,
                    kind: GroupKind::Effect(effect),
                    ..
                } => effect_controls.push(self.effect_element(effect, children, &render, cx)),
                control => {
                    if let Some(control) = Self::control_element(control, &render) {
                        controls.push(control);
                    }
                }
            }
        }
        let effects = (!effect_controls.is_empty() || has_visual).then(|| {
            div()
                .id("effect-stack")
                .relative()
                .w_full()
                .flex()
                .flex_col()
                .gap_3()
                .when(effect_controls.is_empty(), |this| {
                    let inspector = render.inspector.clone();
                    this.context_menu(move |menu, _, cx| {
                        inspector.update(cx, |this, cx| this.effect_menu(menu, None, cx))
                    })
                })
                .child(
                    div()
                        .w_full()
                        .h(px(1.))
                        .flex_none()
                        .bg(render.colors.border),
                )
                .children(effect_controls)
                .when(has_visual, |this| {
                    this.child(Self::add_effect_picker(
                        self.editor.clone(),
                        render.inspector.clone(),
                    ))
                })
        });
        let item_editor = render.editor.clone();
        let item_controls = Self::property_grid()
            .gap_y_3()
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                Self::activate_edit_target(&item_editor, None, cx);
            })
            .children(
                controls
                    .into_iter()
                    .flat_map(super::rows::PropertyRow::into_cells),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .id("property-inspector-scroll")
                    .track_scroll(&self.scroll_handle)
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .gap_3()
                    .p_3()
                    .child(item_controls)
                    .when_some(effects, |this, effects| this.child(effects)),
            )
            .into_any_element()
    }

    pub(crate) fn can_add_effect(&self, cx: &App) -> bool {
        let editor = self.editor.read(cx);
        self.inspector_item_id(cx)
            .and_then(|id| editor.item(id))
            .is_some_and(|item| {
                item.scene_id().is_some()
                    || item
                        .schema()
                        .is_some_and(|schema| schema.render().is_some())
            })
    }

    pub(crate) fn open_effect_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(picker) = self.effect_picker.clone() {
            picker.focus_handle(cx).focus(window, cx);
            return;
        }
        if !self.can_add_effect(cx) {
            return;
        }
        let picker = Self::effect_search_picker(&self.editor, cx.entity(), window, cx);
        cx.subscribe(&picker, |this, _, _: &DismissEvent, cx| {
            this.effect_picker = None;
            cx.notify();
        })
        .detach();
        picker.focus_handle(cx).focus(window, cx);
        self.effect_picker = Some(picker);
        cx.notify();
    }

    fn effect_search_picker(
        editor: &Entity<TimelineEditor>,
        inspector: Entity<Self>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<SearchPicker<EffectPickerTarget>> {
        let entries = editor
            .read(cx)
            .plugin_registry()
            .effects()
            .map(|(plugin_id, effect)| {
                SearchPickerEntry::from_plugin_schema(
                    plugin_id,
                    effect,
                    (plugin_id.to_owned(), effect.id().to_owned()),
                )
            })
            .collect();
        cx.new(|cx| {
            SearchPicker::new(
                entries,
                t!("inspector.search_effects").to_string(),
                move |target, _, cx| Self::add_effect(&inspector, target, cx),
                window,
                cx,
            )
        })
    }

    fn add_effect(inspector: &Entity<Self>, target: EffectPickerTarget, cx: &mut App) {
        let (plugin_id, effect_id) = target;
        inspector.update(cx, |inspector, cx| {
            let Some(item_id) = inspector.inspector_item_id(cx) else {
                return;
            };
            let result = inspector.editor.update(cx, |editor, cx| {
                let result = editor.add_item_effect(item_id, &plugin_id, &effect_id);
                if let Ok(instance_id) = result.as_ref() {
                    editor.set_active_edit_effect(Some(*instance_id));
                    cx.notify();
                }
                result
            });
            if let Err(error) = result {
                inspector.notifications.update(cx, |notifications, cx| {
                    notifications.push(
                        t!("inspector.add_effect_failed", error = error).to_string(),
                        cx,
                    );
                });
            }
        });
    }

    fn control_element(control: Control, render: &RenderCtx) -> Option<super::rows::PropertyRow> {
        match control {
            Control::Group {
                id,
                label,
                children,
                kind,
            } => match kind {
                GroupKind::Plain(extensions) => {
                    Some(Self::group_box(id, label, &children, &extensions, render))
                }
                GroupKind::Elements(group) => Some(Self::elements_section(
                    &group,
                    &children,
                    render,
                    render.colors.border,
                    group.property.is_editable(None),
                )),
                GroupKind::Effect(_) => None,
            },
            leaf => Self::scalar_full_row(&leaf, render),
        }
    }

    fn item_header(
        item: &TimelineItem,
        render: &RenderCtx<'_>,
        selector: gpui::AnyElement,
        cx: &App,
    ) -> Div {
        let editor = render.editor.clone();
        let blend_editor = render.editor.clone();
        let item_id = item.id;
        let blend_mode = item.blend_mode;
        let item_hidden = render.editor.read(cx).is_item_hidden(item_id);
        pane_header(render.colors)
            .child(div().min_w_0().flex_1().overflow_hidden().child(selector))
            .child(
                Button::new("selected-item-blend-mode")
                    .small()
                    .compact()
                    .ghost()
                    .dropdown_caret(true)
                    .label(t!(format!("blend_mode.{}", blend_mode.id())).to_string())
                    .tooltip(t!("inspector.blend_mode").to_string())
                    .popup_menu(move |menu, _, _| {
                        BlendMode::ALL.into_iter().fold(menu, |menu, mode| {
                            let editor = blend_editor.clone();
                            menu.item(
                                PopupMenuItem::new(
                                    t!(format!("blend_mode.{}", mode.id())).to_string(),
                                )
                                .checked(blend_mode == mode)
                                .on_click(move |_, _, cx| {
                                    editor.update(cx, |editor, cx| {
                                        if editor.set_item_blend_mode(item_id, mode) {
                                            cx.notify();
                                        }
                                    });
                                }),
                            )
                        })
                    }),
            )
            .child(
                Button::new("toggle-selected-item-visibility")
                    .small()
                    .compact()
                    .ghost()
                    .icon(if item_hidden {
                        IconName::EyeOff
                    } else {
                        IconName::Eye
                    })
                    .tooltip(if item_hidden {
                        t!("inspector.show_item").to_string()
                    } else {
                        t!("inspector.hide_item").to_string()
                    })
                    .on_click(move |_, _, cx| {
                        editor.update(cx, |editor, cx| {
                            if editor.toggle_item_visibility(item_id) {
                                cx.notify();
                            }
                        });
                    }),
            )
    }

    fn activate_edit_target(
        editor: &Entity<TimelineEditor>,
        effect_id: Option<EffectInstanceId>,
        cx: &mut App,
    ) {
        editor.update(cx, |editor, cx| {
            if editor.set_active_edit_effect(effect_id) {
                cx.notify();
            }
        });
    }

    fn effect_element(
        &self,
        effect: EffectGroup,
        controls: Vec<Control>,
        render: &RenderCtx<'_>,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        let effect_id = effect.id;
        let focused = render.editor.read(cx).active_edit_effect() == Some(effect_id);
        let effect_editor = render.editor.clone();
        let (can_move_up, can_move_down) = {
            let editor = render.editor.read(cx);
            (
                editor.can_move_effect(render.item_id, effect_id, -1),
                editor.can_move_effect(render.item_id, effect_id, 1),
            )
        };
        let controls = controls
            .into_iter()
            .filter_map(|control| Self::control_element(control, render))
            .collect::<Vec<_>>();

        let inspector = render.inspector.clone();
        div()
            .id(SharedString::from(format!(
                "effect-card-{}",
                effect_id.get()
            )))
            .relative()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .pb_3()
            .context_menu(move |menu, _, cx| {
                inspector.update(cx, |this, cx| this.effect_menu(menu, Some(effect_id), cx))
            })
            .border_b_1()
            .border_color(render.colors.border)
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                Self::activate_edit_target(&effect_editor, Some(effect_id), cx);
            })
            .child(Self::effect_header(
                &effect,
                can_move_up,
                can_move_down,
                focused,
                render,
            ))
            .child(
                Self::property_grid().gap_y_2().children(
                    controls
                        .into_iter()
                        .flat_map(super::rows::PropertyRow::into_cells),
                ),
            )
            .into_any_element()
    }

    fn effect_header(
        effect: &EffectGroup,
        can_move_up: bool,
        can_move_down: bool,
        focused: bool,
        render: &RenderCtx<'_>,
    ) -> Div {
        let effect_id = effect.id;
        let item_id = render.item_id;
        let hidden = effect.hidden;
        let label = effect.label.clone();
        let title_editor = render.editor.clone();
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
                    .id(SharedString::from(format!(
                        "effect-editor-title-{}",
                        effect_id.get()
                    )))
                    .text_sm()
                    .text_color(if focused {
                        render.colors.primary
                    } else if hidden {
                        render.colors.muted_foreground
                    } else {
                        render.colors.foreground
                    })
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        Self::activate_edit_target(&title_editor, Some(effect_id), cx);
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
                            t!("inspector.enable_effect")
                        } else {
                            t!("inspector.disable_effect")
                        })
                        .on_click(move |_, _, cx| {
                            visibility_editor.update(cx, |editor, cx| {
                                if editor.toggle_effect_visibility(item_id, effect_id) {
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
                        .tooltip(t!("common.move_up").to_string())
                        .disabled(!can_move_up)
                        .on_click(move |_, _, cx| {
                            move_up_editor.update(cx, |editor, cx| {
                                if editor.move_effect(item_id, effect_id, -1) {
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
                        .tooltip(t!("common.move_down").to_string())
                        .disabled(!can_move_down)
                        .on_click(move |_, _, cx| {
                            move_down_editor.update(cx, |editor, cx| {
                                if editor.move_effect(item_id, effect_id, 1) {
                                    cx.notify();
                                }
                            });
                        }),
                    )
                    .child(
                        Button::new(SharedString::from(format!(
                            "remove-effect-{}",
                            effect_id.get()
                        )))
                        .small()
                        .compact()
                        .ghost()
                        .icon(IconName::Delete)
                        .tooltip(t!("common.delete").to_string())
                        .on_click(move |_, _, cx| {
                            remove_editor.update(cx, |editor, cx| {
                                if editor.remove_item_effect(item_id, effect_id) {
                                    cx.notify();
                                }
                            });
                        }),
                    ),
            )
    }

    fn add_effect_picker(
        editor: Entity<TimelineEditor>,
        inspector: Entity<Self>,
    ) -> gpui::AnyElement {
        Popover::new("add-effect-picker")
            .trigger(
                Button::new("add-effect")
                    .small()
                    .label(t!("inspector.add_effect").to_string())
                    .dropdown_caret(true),
            )
            .content(move |window, cx| {
                Self::effect_search_picker(&editor, inspector.clone(), window, cx)
            })
            .into_any_element()
    }
}
