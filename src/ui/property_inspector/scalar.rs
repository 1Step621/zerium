use super::*;

impl PropertyInspector {
    /// Write a structural scalar without replacing its tuple or array siblings.
    pub(super) fn set_scalar_value(
        editor: &mut TimelineEditor,
        effect_id: Option<EffectInstanceId>,
        parameter_id: &str,
        path: SceneBindingValuePath,
        value: ParameterValue,
    ) -> bool {
        editor.update_selected_scalar(effect_id, parameter_id, path, value)
    }

    pub(super) fn scalar_controls(
        key: PropertyPath,
        parameter: &ParameterSchema,
        value: &ParameterValue,
        effect_id: Option<EffectInstanceId>,
        array: Option<usize>,
        is_size: bool,
    ) -> Vec<PropertyControl> {
        if !parameter.is_visible() {
            return Vec::new();
        }
        let id = parameter.id();
        let label = array.map_or_else(
            || parameter.label().to_owned(),
            |index| format!("{} {}", parameter.label(), index + 1),
        );
        let ty = parameter.ty().element_type();
        let ui = parameter.ui();
        let scene_bindable = parameter.is_scene_bindable();
        ty.scalars()
            .filter_map(|(element, ty)| {
                let value = value.scalar_at(element)?;
                let scalar_ui = element.map_or(ui, |index| ui.for_element(index));
                if !scalar_ui.is_visible() {
                    return None;
                }
                let label = element.map_or_else(
                    || label.to_owned(),
                    |index| {
                        format!(
                            "{label} {}",
                            scalar_ui
                                .label()
                                .map(str::to_owned)
                                .unwrap_or_else(|| (index + 1).to_string())
                        )
                    },
                );
                let target = PropertyTarget {
                    key: key.scalar(array, element),
                    parameter_id: id.to_owned(),
                    effect_id,
                    value_path: SceneBindingValuePath::from_elements(array, element),
                };
                let element_label = parameter.scalar_label(element);
                match (ty, value) {
                    (
                        ScalarParameterType::F32
                        | ScalarParameterType::I32
                        | ScalarParameterType::U32,
                        _,
                    ) => {
                        let mut field = Self::numeric_field(target, parameter, is_size)?;
                        field.label = array.map_or_else(
                            || parameter.label().to_owned(),
                            |index| format!("{} {}", parameter.label(), index + 1),
                        );
                        Some(PropertyControl::Number(field))
                    }
                    (ScalarParameterType::Color, ParameterValue::Color(_)) => {
                        Some(PropertyControl::Color(ColorField {
                            target,
                            label,
                            element_label: element_label.clone(),
                            animatable: parameter.is_animatable(),
                            scene_bindable,
                        }))
                    }
                    (ScalarParameterType::Bool, ParameterValue::Bool(value)) => {
                        Some(PropertyControl::Bool(BoolField {
                            target,
                            label,
                            element_label: element_label.clone(),
                            value: *value,
                            mixed: false,
                            scene_bindable,
                        }))
                    }
                    (ScalarParameterType::String, ParameterValue::String(value)) => {
                        Some(PropertyControl::String(StringField {
                            target,
                            label,
                            element_label: element_label.clone(),
                            value: value.clone(),
                            multiline: scalar_ui.is_multiline(),
                            scene_bindable,
                        }))
                    }
                    (ScalarParameterType::Enum(_), ParameterValue::Enum(value)) => {
                        Some(PropertyControl::Choice(ChoiceField {
                            target,
                            ty: ParameterType::Value(ParameterValueType::Scalar(ty.clone())),
                            scene_bindable,
                            label,
                            element_label,
                            value: *value,
                            options: scalar_ui
                                .enum_options(ty)?
                                .into_iter()
                                .map(|(value, label)| (label, value))
                                .collect(),
                        }))
                    }
                    _ => None,
                }
            })
            .collect()
    }
}
