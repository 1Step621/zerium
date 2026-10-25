use super::{SceneSettingsRenderCtx, SceneVariableRow};
use crate::ui::panels::scene_settings::*;

impl SceneSettings {
    pub(in crate::ui::panels::scene_settings) fn edit_calculation_inputs(
        &mut self,
        id: &str,
        edit: impl FnOnce(&mut Vec<SceneCalculationInput>),
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(mut inputs) = self
            .editor
            .read(cx)
            .active_scene()
            .variable(id)
            .and_then(|variable| variable.calculation())
            .map(|calculation| calculation.inputs.clone())
        else {
            return false;
        };
        edit(&mut inputs);
        let result = self.editor.update(cx, |editor, cx| {
            let result = editor.update_scene_calculation_inputs(id, inputs);
            if result == Ok(true) {
                cx.notify();
            }
            result
        });
        if let Err(error) = result {
            self.notifications.update(cx, |notifications, cx| {
                notifications.push(t!("calc.edit_failed", error = error).to_string(), cx);
            });
            return false;
        }
        true
    }

    pub(in crate::ui::panels::scene_settings) fn calculation_details(
        &self,
        row: &SceneVariableRow,
        calculation: &zerium_core::timeline::SceneCalculation,
        render: &SceneSettingsRenderCtx<'_>,
    ) -> Div {
        let sources = render
            .sources
            .iter()
            .filter(|(id, _)| id != row.variable.id())
            .cloned()
            .collect::<Vec<_>>();
        let mut details = div().w_full().min_w_0().flex().flex_col().gap_2();
        let ty = match row.variable.ty() {
            zerium_core::property::ScalarPropertyType::F32 => t!("args.number"),
            zerium_core::property::ScalarPropertyType::I32 => t!("args.integer"),
            zerium_core::property::ScalarPropertyType::U32 => t!("args.unsigned_integer"),
            zerium_core::property::ScalarPropertyType::Bool => t!("args.boolean"),
            zerium_core::property::ScalarPropertyType::String => t!("args.string"),
            _ => t!("args.color"),
        };
        details = details.child(
            div()
                .text_sm()
                .text_color(render.colors.muted_foreground)
                .child(t!("calc.output_type", ty = ty).to_string()),
        );
        for (index, input) in calculation.inputs.iter().enumerate() {
            let Some(name_input) = self.store.inputs.text(&ControlId::CalculationInputName {
                scene_id: row.scene_id,
                variable_id: row.variable.id().to_owned(),
                input_name: input.name.clone(),
            }) else {
                continue;
            };
            let settings = render.settings.clone();
            let id = row.variable.id().to_owned();
            let choices = sources.clone();
            let label = sources
                .iter()
                .find(|(id, _)| id == &input.variable_id)
                .map(|(_, label)| label.clone())
                .unwrap_or_else(|| t!("calc.missing_input").to_string());
            let remove_settings = render.settings.clone();
            let remove_id = row.variable.id().to_owned();
            details = details.child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w_0()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&name_input).small().w_full()),
                    )
                    .child(
                        Button::new(SharedString::from(format!(
                            "calculation-source-{}-{index}",
                            row.variable.id()
                        )))
                        .small()
                        .compact()
                        .label(label)
                        .dropdown_caret(true)
                        .popup_menu(move |menu, _, _| {
                            choices.iter().fold(menu, |menu, (source_id, label)| {
                                let settings = settings.clone();
                                let id = id.clone();
                                let source_id = source_id.clone();
                                menu.item(PopupMenuItem::new(label.clone()).on_click(
                                    move |_, _, cx| {
                                        settings.update(cx, |settings, cx| {
                                            settings.edit_calculation_inputs(
                                                &id,
                                                |inputs| {
                                                    if let Some(input) = inputs.get_mut(index) {
                                                        input.variable_id = source_id.clone();
                                                    }
                                                },
                                                cx,
                                            );
                                        });
                                    },
                                ))
                            })
                        }),
                    )
                    .child(
                        Button::new(SharedString::from(format!(
                            "calculation-remove-input-{}-{index}",
                            row.variable.id()
                        )))
                        .small()
                        .compact()
                        .ghost()
                        .icon(IconName::Delete)
                        .tooltip(t!("calc.remove_input").to_string())
                        .on_click(move |_, _, cx| {
                            remove_settings.update(cx, |settings, cx| {
                                settings.edit_calculation_inputs(
                                    &remove_id,
                                    |inputs| {
                                        if index < inputs.len() {
                                            inputs.remove(index);
                                        }
                                    },
                                    cx,
                                );
                            });
                        }),
                    ),
            );
        }
        let settings = render.settings.clone();
        let id = row.variable.id().to_owned();
        details = details.child(
            Button::new(SharedString::from(format!(
                "calculation-add-input-{}",
                row.variable.id()
            )))
            .small()
            .compact()
            .label(t!("calc.add_input").to_string())
            .dropdown_caret(true)
            .disabled(sources.is_empty())
            .popup_menu(move |menu, _, _| {
                sources.iter().fold(menu, |menu, (source_id, label)| {
                    let settings = settings.clone();
                    let id = id.clone();
                    let source_id = source_id.clone();
                    menu.item(PopupMenuItem::new(label.clone()).on_click(move |_, _, cx| {
                        settings.update(cx, |settings, cx| {
                            settings.edit_calculation_inputs(
                                &id,
                                |inputs| {
                                    let ordinal = (1..)
                                        .find(|ordinal| {
                                            !inputs.iter().any(|input| {
                                                input.name == format!("input_{ordinal}")
                                            })
                                        })
                                        .expect("input alias available");
                                    inputs.push(SceneCalculationInput {
                                        name: format!("input_{ordinal}"),
                                        variable_id: source_id.clone(),
                                    });
                                },
                                cx,
                            );
                        });
                    }))
                })
            }),
        );
        if let Some(code) = self.store.inputs.text(&ControlId::CalculationScript {
            scene_id: row.scene_id,
            variable_id: row.variable.id().to_owned(),
        }) {
            details = details.child(Input::new(&code).small().w_full().min_h(px(96.)));
        }
        if let Some(result) = &row.result {
            let (text, color) = match result {
                Ok(value) => {
                    let text = match value {
                        PropertyValue::String(text) => text.clone(),
                        PropertyValue::Bool(value) => value.to_string(),
                        PropertyValue::Color(value) => format!(
                            "rgba({}, {}, {}, {})",
                            value[0], value[1], value[2], value[3]
                        ),
                        value => value.numeric_text().unwrap_or_default(),
                    };
                    (
                        t!("calc.result", value = text).to_string(),
                        render.colors.muted_foreground,
                    )
                }
                Err(error) => (error.clone(), render.colors.danger),
            };
            details = details.child(
                div()
                    .text_sm()
                    .whitespace_normal()
                    .text_color(color)
                    .child(text),
            );
        }
        details
    }
}
