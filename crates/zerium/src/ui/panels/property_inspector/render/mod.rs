pub(super) mod animation;
pub(super) mod binding;
use super::{
    control::{Control, EffectControl},
    rows::{RenderCtx, elements_section, group_box, property_grid, scalar_full_row},
    *,
};
use crate::ui::components::pane::{settings_section, settings_section_header};
use rust_i18n::t;
use zerium_core::timeline::BlendMode;

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
            .capture_any_mouse_down(cx.listener(|this, _, _, _| {
                this.finish_animation_value_edit();
            }))
            .on_mouse_down_out(cx.listener(|this, _, _, _| {
                this.finish_animation_value_edit();
            }))
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

    fn item_element(&self, item: &TimelineItem, cx: &mut Context<Self>) -> gpui::AnyElement {
        let selector = self.target_selector(cx);
        let render = self.render_context(item, cx);
        let collapsed = self
            .collapsed_sections
            .contains(&PropertySection::Item(item.id));
        let defaults_inspector = render.inspector.clone();
        let header = item_header(item, collapsed, &render, selector)
            .id("item-properties-header")
            .when(item.schema().is_some(), |header| {
                header.context_menu(move |menu, _, cx| {
                    defaults_inspector.update(cx, |this, cx| this.defaults_menu(menu, None, cx))
                })
            });
        let has_visual = self.can_add_effect(cx);
        let mut controls = Vec::new();
        let mut effect_controls = Vec::new();
        for control in &self.store.tree.roots {
            match control {
                Control::Effect(effect) => {
                    let collapsed = self
                        .collapsed_sections
                        .contains(&PropertySection::Effect(item.id, effect.id));
                    effect_controls.push(effect_element(effect, collapsed, &render, cx))
                }
                control if !collapsed => {
                    if let Some(control) = control_element(control.clone(), &render) {
                        controls.push(control);
                    }
                }
                _ => {}
            }
        }
        let effects = (!effect_controls.is_empty() || has_visual).then(|| {
            div()
                .id("effect-stack")
                .relative()
                .w_full()
                .min_w_0()
                .flex_none()
                .flex()
                .flex_col()
                .gap_4()
                .when(effect_controls.is_empty(), |this| {
                    let inspector = render.inspector.clone();
                    this.context_menu(move |menu, _, cx| {
                        inspector.update(cx, |this, cx| this.effect_menu(menu, None, cx))
                    })
                })
                .children(effect_controls)
                .when(has_visual, |this| {
                    this.child(add_effect_picker(
                        self.editor.clone(),
                        render.inspector.clone(),
                    ))
                })
        });
        let item_editor = render.editor.clone();
        let item_controls = property_grid().p_3().gap_y_3().children(
            controls
                .into_iter()
                .flat_map(crate::ui::panels::property_inspector::rows::PropertyRow::into_cells),
        );
        let item_section = settings_section(render.colors)
            .capture_any_mouse_down(move |event, _, cx| {
                if event.button == MouseButton::Left {
                    activate_edit_target(&item_editor, None, cx);
                }
            })
            .child(header)
            .when(!collapsed, |this| this.child(item_controls));
        div()
            .id("property-inspector-scroll")
            .track_scroll(&self.scroll_handle)
            .size_full()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .gap_4()
            .p_3()
            .child(item_section)
            .when_some(effects, |this, effects| this.child(effects))
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
        let picker = effect_search_picker(&self.editor, cx.entity(), window, cx);
        cx.subscribe(&picker, |this, _, _: &DismissEvent, cx| {
            this.effect_picker = None;
            cx.notify();
        })
        .detach();
        picker.focus_handle(cx).focus(window, cx);
        self.effect_picker = Some(picker);
        cx.notify();
    }
}

fn effect_search_picker(
    editor: &Entity<TimelineEditor>,
    inspector: Entity<PropertyInspector>,
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
            move |target, _, cx| add_effect(&inspector, target, cx),
            window,
            cx,
        )
    })
}

fn add_effect(inspector: &Entity<PropertyInspector>, target: EffectPickerTarget, cx: &mut App) {
    let (plugin_id, effect_id) = target;
    inspector.update(cx, |inspector, cx| {
        let Some(item_id) = inspector.inspector_item_id(cx) else {
            return;
        };
        let result = inspector.editor.update(cx, |editor, cx| {
            let target = crate::app::defaults::DefaultsTarget::Effect {
                plugin_id: plugin_id.clone(),
                effect_id: effect_id.clone(),
            };
            let properties = editor
                .plugin_registry()
                .effect(&plugin_id, &effect_id)
                .map(|schema| crate::app::defaults::properties(cx, &target, schema.properties()))
                .unwrap_or_default();
            let result = editor.add_item_effect(item_id, &plugin_id, &effect_id, properties);
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

fn control_element(
    control: Control,
    render: &RenderCtx,
) -> Option<crate::ui::panels::property_inspector::rows::PropertyRow> {
    let address = match &control {
        Control::Tuple(tuple) => tuple.target.clone(),
        Control::Array(array) => array.target.clone(),
        Control::Effect(_) => return None,
        leaf => leaf.common()?.target.clone(),
    };
    let row = match control {
        Control::Tuple(tuple) => Some(group_box(
            ControlId::Group(tuple.target),
            tuple.label,
            &tuple.children,
            tuple.aspect_ratio_lock.as_ref(),
            render,
        )),
        Control::Array(array) => Some(elements_section(
            &array,
            render,
            render.colors.border,
            array.property.is_editable(None),
        )),
        Control::Effect(_) => None,
        leaf => scalar_full_row(&leaf, render),
    }?;
    Some(row.with_reset_menu(address, render))
}

fn item_header(
    item: &TimelineItem,
    collapsed: bool,
    render: &RenderCtx<'_>,
    selector: gpui::AnyElement,
) -> Div {
    let editor = render.editor.clone();
    let blend_editor = render.editor.clone();
    let item_id = item.id;
    let blend_mode = item.blend_mode;
    let item_hidden = item.hidden;
    settings_section_header(render.colors)
        .flex()
        .items_center()
        .gap_1()
        .py_1()
        .when(collapsed, |this| this.rounded_b_md().border_b_0())
        .child(section_toggle(
            PropertySection::Item(item_id),
            collapsed,
            &render.inspector,
        ))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .overflow_hidden()
                .child(selector),
        )
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
                            PopupMenuItem::new(t!(format!("blend_mode.{}", mode.id())).to_string())
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

fn effect_header(
    effect: &EffectControl,
    collapsed: bool,
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

    settings_section_header(render.colors)
        .flex()
        .items_center()
        .justify_between()
        .py_1()
        .when(collapsed, |this| this.rounded_b_md().border_b_0())
        .child(
            div()
                .id(SharedString::from(format!(
                    "effect-editor-title-{}",
                    effect_id.get()
                )))
                .min_w_0()
                .flex_1()
                .flex()
                .items_center()
                .gap_1()
                .text_sm()
                .text_color(if focused {
                    render.colors.accent_foreground
                } else if hidden {
                    render.colors.muted_foreground
                } else {
                    render.colors.foreground
                })
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    activate_edit_target(&title_editor, Some(effect_id), cx);
                })
                .child(section_toggle(
                    PropertySection::Effect(item_id, effect_id),
                    collapsed,
                    &render.inspector,
                ))
                .child(div().min_w_0().truncate().child(label)),
        )
        .child(
            div()
                .flex_none()
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
    inspector: Entity<PropertyInspector>,
) -> gpui::AnyElement {
    Popover::new("add-effect-picker")
        .trigger(
            Button::new("add-effect")
                .small()
                .label(t!("inspector.add_effect").to_string())
                .dropdown_caret(true),
        )
        .content(move |window, cx| effect_search_picker(&editor, inspector.clone(), window, cx))
        .into_any_element()
}

fn effect_element(
    effect: &EffectControl,
    collapsed: bool,
    render: &RenderCtx<'_>,
    cx: &Context<PropertyInspector>,
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
    let inspector = render.inspector.clone();
    let header = effect_header(
        effect,
        collapsed,
        can_move_up,
        can_move_down,
        focused,
        render,
    )
    .id(SharedString::from(format!(
        "effect-header-{}",
        effect_id.get()
    )))
    .context_menu(move |menu, _, cx| {
        inspector.update(cx, |this, cx| this.effect_menu(menu, Some(effect_id), cx))
    });

    settings_section(render.colors)
        .id(SharedString::from(format!(
            "effect-card-{}",
            effect_id.get()
        )))
        .relative()
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            activate_edit_target(&effect_editor, Some(effect_id), cx);
        })
        .child(header)
        .when(!collapsed, |this| {
            this.child(
                property_grid().p_3().gap_y_2().children(
                    effect
                        .children
                        .iter()
                        .cloned()
                        .filter_map(|control| control_element(control, render))
                        .flat_map(
                            crate::ui::panels::property_inspector::rows::PropertyRow::into_cells,
                        ),
                ),
            )
        })
        .into_any_element()
}

fn section_toggle(
    section: PropertySection,
    collapsed: bool,
    inspector: &Entity<PropertyInspector>,
) -> Button {
    let id = match section {
        PropertySection::Item(item_id) => ("toggle-item-properties", item_id.get()),
        PropertySection::Effect(_, effect_id) => ("toggle-effect-properties", effect_id.get()),
    };
    let inspector = inspector.clone();
    Button::new(id)
        .small()
        .compact()
        .ghost()
        .flex_none()
        .icon(if collapsed {
            IconName::ChevronRight
        } else {
            IconName::ChevronDown
        })
        .tooltip(if collapsed {
            t!("inspector.expand_properties").to_string()
        } else {
            t!("inspector.collapse_properties").to_string()
        })
        .on_click(move |_, window, cx| {
            inspector.update(cx, |this, cx| this.toggle_section(section, window, cx));
            cx.stop_propagation();
        })
}
