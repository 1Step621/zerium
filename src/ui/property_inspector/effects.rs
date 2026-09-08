use super::*;

impl PropertyInspector {
    pub(super) fn effects_element(
        &self,
        view: &InspectorSelectionView,
        render: &InspectorRenderContext<'_>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let cards = view
            .effects
            .iter()
            .cloned()
            .map(|effect| self.effect_element(effect, view, render, cx))
            .collect::<Vec<_>>();

        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .w_full()
                    .h(px(1.))
                    .flex_none()
                    .bg(render.colors.border),
            )
            .children(cards)
            .when(!view.multiple, |this| {
                this.child(Self::add_effect_picker(
                    view.available_effects.clone(),
                    render.inspector.clone(),
                ))
            })
            .into_any_element()
    }

    fn effect_element(
        &self,
        effect: EffectInstance,
        view: &InspectorSelectionView,
        render: &InspectorRenderContext<'_>,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        let mut property_controls = Self::effect_property_controls(&effect);
        if view.multiple {
            property_controls
                .iter_mut()
                .for_each(PropertyControl::disable_animation);
        }
        let effect_id = effect.id;
        let hidden = view.hidden_effects.contains(&effect_id);
        let (can_move_up, can_move_down) = {
            let editor = render.editor.read(cx);
            (
                editor.can_move_selected_effect(effect_id, -1),
                editor.can_move_selected_effect(effect_id, 1),
            )
        };
        let controls = property_controls
            .into_iter()
            .filter_map(|control| Self::property_control_element(control, view, render))
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
                effect.schema().label().to_owned(),
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
        render: &InspectorRenderContext<'_>,
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
