use super::*;

impl PropertyInspector {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn string_field_element(
        field: StringField,
        selected_item_id: ItemId,
        editing_scene: bool,
        scene_arguments: &[SceneArgumentOption],
        inspector: &Entity<Self>,
        inputs: &HashMap<PropertyPath, Entity<InputState>>,
    ) -> Option<gpui::Div> {
        let scene_binding = Self::scene_field_binding(
            editing_scene,
            false,
            field.scene_bindable,
            SceneBindingTarget::new(
                selected_item_id,
                SceneBindingOwner::from_effect(field.target.effect_id),
                field.target.parameter_id,
                field.target.value_path,
            ),
            &ParameterType::Value(ParameterValueType::Scalar(ScalarParameterType::String)),
            scene_arguments,
        );
        let is_bound = scene_binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = scene_binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                inspector,
                SharedString::from(format!("bind-scene-argument-{}", field.target.key)),
            )
        });
        inputs.get(&field.target.key).map(|input| {
            div()
                .w_full()
                .flex()
                .items_start()
                .gap_3()
                .child(Self::parameter_label_column(field.label))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .items_center()
                        .gap_1()
                        .when(!is_bound, |this| {
                            this.child(
                                Input::new(input)
                                    .small()
                                    .w_full()
                                    .when(field.multiline, |input| input.h(px(72.))),
                            )
                        })
                        .when_some(binding_button, |this, button| this.child(button)),
                )
        })
    }
}
