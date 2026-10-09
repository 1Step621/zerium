use rust_i18n::t;
use zerium_core::timeline::BlendMode;

use super::control::{Control, ControlTree, EffectGroup, GroupKind};
use super::rows::RenderCtx;
use super::*;

pub(super) struct SelectionView {
    pub item: TimelineItem,
    pub tree: ControlTree,
    pub available_effects: Vec<SearchPickerEntry<EffectPickerTarget>>,
    pub multiple: bool,
    pub has_visual: bool,
    pub items_hidden: bool,
    pub item_visibility_mixed: bool,
    pub blend_mode: Option<BlendMode>,
}

impl Render for PropertyInspector {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let selected = self.selected_view(cx);

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
            .when_some(selected, |this, selected| {
                this.child(self.selected_view_element(selected, cx))
            })
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
    fn render_context<'a>(
        &'a self,
        selection: &'a SelectionView,
        cx: &mut Context<Self>,
    ) -> RenderCtx<'a> {
        RenderCtx {
            colors: cx.theme().colors,
            editor: &self.editor,
            animation_address: self
                .animation_selection
                .read(cx)
                .address()
                .and_then(|address| {
                    self.editor
                        .read(cx)
                        .corresponding_property_address(address, selection.item.id)
                }),
            inspector: cx.entity(),
            store: &self.store,
            font_names: &self.font_names,
            selecting_file: self.file_input.read(cx).is_selecting(),
            file_input: self.file_input.clone(),
            scene_overrides: self
                .editor
                .read(cx)
                .source_items_in_scope(self.scope)
                .filter(|item| item.scene_id().is_some())
                .flat_map(|item| {
                    item.properties
                        .iter()
                        .map(|(id, _)| id.to_owned())
                        .collect::<Vec<_>>()
                })
                .collect(),
            item_id: selection.item.id,
            scope: self.scope,
            multiple: selection.multiple,
        }
    }

    fn selected_view(&self, cx: &mut Context<Self>) -> Option<SelectionView> {
        let selected_items = {
            let editor = self.editor.read(cx);
            let time = zerium_core::timeline::TimelineTime::from_frame(editor.playhead());
            editor.evaluated_items_in_scope(self.scope, time)
        };
        let item = selected_items.first()?.clone();
        let multiple = selected_items.len() > 1;
        let blend_mode = selected_items
            .iter()
            .all(|selected| selected.blend_mode == item.blend_mode)
            .then_some(item.blend_mode);
        let hidden_state = self.editor.read(cx).items_hidden_state(self.scope);
        let has_visual = item.scene_id().is_some()
            || item
                .schema()
                .is_some_and(|schema| schema.render().is_some());
        let available_effects = {
            let editor = self.editor.read(cx);
            editor
                .plugin_registry()
                .effects()
                .map(|(plugin_id, effect)| {
                    SearchPickerEntry::from_plugin_schema(
                        plugin_id,
                        effect,
                        (plugin_id.to_owned(), effect.id().to_owned()),
                    )
                })
                .collect()
        };
        Some(SelectionView {
            item,
            tree: self.store.tree.clone(),
            available_effects,
            multiple,
            has_visual,
            items_hidden: hidden_state == Some(true),
            item_visibility_mixed: hidden_state.is_none() && multiple,
            blend_mode,
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
                SceneArgumentOption {
                    id: argument.schema.id().to_owned(),
                    label,
                    schema: argument.schema.clone(),
                    bindings: argument.bindings.clone(),
                }
            })
            .collect()
    }

    fn selected_view_element(
        &self,
        view: SelectionView,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let selector = self.target_selector(cx);
        let render = self.render_context(&view, cx);
        let header = Self::selection_header(&view, &render, selector);
        let mut controls = Vec::new();
        let mut effect_controls = Vec::new();
        for control in view.tree.roots.iter().cloned() {
            match control {
                Control::Group {
                    children,
                    kind: GroupKind::Effect(effect),
                    ..
                } => {
                    effect_controls.push(self.effect_element(effect, children, &view, &render, cx))
                }
                control => {
                    if let Some(control) = Self::control_element(control, &render) {
                        controls.push(control);
                    }
                }
            }
        }
        let effects =
            (!effect_controls.is_empty() || view.has_visual && !view.multiple).then(|| {
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
                    .when(view.has_visual && !view.multiple, |this| {
                        this.child(Self::add_effect_picker(
                            view.available_effects.clone(),
                            render.inspector.clone(),
                        ))
                    })
            });
        let item_editor = render.editor.clone();
        let item_controls = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_3()
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                Self::activate_edit_target(&item_editor, None, cx);
            })
            .children(controls);
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

    pub(crate) fn open_effect_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(picker) = self.effect_picker.clone() {
            picker.focus_handle(cx).focus(window, cx);
            return;
        }
        let Some(view) = self.selected_view(cx) else {
            return;
        };
        if view.multiple || !view.has_visual {
            return;
        }
        let picker = Self::effect_search_picker(view.available_effects, cx.entity(), window, cx);
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
        entries: Vec<SearchPickerEntry<EffectPickerTarget>>,
        inspector: Entity<Self>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<SearchPicker<EffectPickerTarget>> {
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
            let Some(view) = inspector.selected_view(cx).filter(|view| !view.multiple) else {
                return;
            };
            let result = inspector.editor.update(cx, |editor, cx| {
                let result = editor.add_item_effect(view.item.id, &plugin_id, &effect_id);
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

    fn control_element(control: Control, render: &RenderCtx) -> Option<gpui::AnyElement> {
        match control {
            Control::Group {
                label,
                children,
                kind,
                ..
            } => match kind {
                GroupKind::Plain(extensions) => {
                    Some(Self::group_box(label, &children, &extensions, render))
                }
                GroupKind::Elements(group) => Some(Self::elements_section(
                    &group,
                    &children,
                    render,
                    render.colors.border,
                    !render.multiple && group.property.is_editable(None),
                )),
                GroupKind::Effect(_) => None,
            },
            leaf => Self::scalar_full_row(&leaf, render),
        }
    }

    fn selection_header(
        view: &SelectionView,
        render: &RenderCtx<'_>,
        selector: gpui::AnyElement,
    ) -> Div {
        let editor = render.editor.clone();
        let blend_editor = render.editor.clone();
        let scope = render.scope;
        let blend_mode = view.blend_mode;
        pane_header(render.colors)
            .child(div().min_w_0().flex_1().overflow_hidden().child(selector))
            .child(
                Button::new("selected-item-blend-mode")
                    .small()
                    .compact()
                    .ghost()
                    .dropdown_caret(true)
                    .label(blend_mode.map_or_else(
                        || t!("inspector.mixed_blend_mode").to_string(),
                        |mode| t!(format!("blend_mode.{}", mode.id())).to_string(),
                    ))
                    .tooltip(t!("inspector.blend_mode").to_string())
                    .popup_menu(move |menu, _, _| {
                        BlendMode::ALL.into_iter().fold(menu, |menu, mode| {
                            let editor = blend_editor.clone();
                            menu.item(
                                PopupMenuItem::new(
                                    t!(format!("blend_mode.{}", mode.id())).to_string(),
                                )
                                .checked(blend_mode == Some(mode))
                                .on_click(move |_, _, cx| {
                                    editor.update(cx, |editor, cx| {
                                        if editor.set_items_blend_mode(scope, mode) {
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
                    .icon(if view.items_hidden {
                        IconName::EyeOff
                    } else if view.item_visibility_mixed {
                        IconName::EyeClosed
                    } else {
                        IconName::Eye
                    })
                    .tooltip(if view.items_hidden {
                        t!("inspector.show_selected_items").to_string()
                    } else if view.item_visibility_mixed {
                        t!("inspector.hide_selected_items_mixed").to_string()
                    } else {
                        t!("inspector.hide_selected_items").to_string()
                    })
                    .on_click(move |_, _, cx| {
                        editor.update(cx, |editor, cx| {
                            if editor.toggle_items_visibility(scope) {
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
        view: &SelectionView,
        render: &RenderCtx<'_>,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        let effect_id = effect.id;
        let focused = render.editor.read(cx).active_edit_effect() == Some(effect_id);
        let effect_editor = render.editor.clone();
        let (can_move_up, can_move_down) = {
            let editor = render.editor.read(cx);
            (
                editor.can_move_effect(render.scope, effect_id, -1),
                editor.can_move_effect(render.scope, effect_id, 1),
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
            .when(!view.multiple, |this| {
                this.context_menu(move |menu, _, cx| {
                    inspector.update(cx, |this, cx| this.effect_menu(menu, Some(effect_id), cx))
                })
            })
            .border_b_1()
            .border_color(render.colors.border)
            .when(!view.multiple, |this| {
                this.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    Self::activate_edit_target(&effect_editor, Some(effect_id), cx);
                })
            })
            .child(Self::effect_header(
                &effect,
                can_move_up,
                can_move_down,
                view.multiple,
                focused,
                render,
            ))
            .children(controls)
            .into_any_element()
    }

    fn effect_header(
        effect: &EffectGroup,
        can_move_up: bool,
        can_move_down: bool,
        multiple: bool,
        focused: bool,
        render: &RenderCtx<'_>,
    ) -> Div {
        let effect_id = effect.id;
        let scope = render.scope;
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
                    .when(!multiple, |this| {
                        this.cursor_pointer().on_click(move |_, _, cx| {
                            Self::activate_edit_target(&title_editor, Some(effect_id), cx);
                        })
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
                                if editor.toggle_effect_visibility(scope, effect_id) {
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
                                if editor.move_effect(scope, effect_id, -1) {
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
                                if editor.move_effect(scope, effect_id, 1) {
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
                        )
                    }),
            )
    }

    fn add_effect_picker(
        entries: Vec<SearchPickerEntry<EffectPickerTarget>>,
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
                Self::effect_search_picker(entries.clone(), inspector.clone(), window, cx)
            })
            .into_any_element()
    }
}
