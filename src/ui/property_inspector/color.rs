use super::*;

impl PropertyInspector {
    pub(super) fn color_field_element(
        field: ColorField,
        picker: &Entity<ColorPickerState>,
        animation_pickers: &HashMap<(PropertyPath, AnimationEndpoint), Entity<ColorPickerState>>,
        animation_enabled: bool,
        scene_binding: Option<SceneFieldBinding>,
        inspector: &Entity<Self>,
    ) -> gpui::AnyElement {
        let property = field.target.clone();
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
        let animation_button = (field.animatable && !is_bound).then(|| {
            let inspector = inspector.clone();
            let property = property.clone();
            Button::new(SharedString::from(format!(
                "toggle-animation-{}",
                field.target.key
            )))
            .icon(Icon::new(IconName::Keyframe))
            .small()
            .compact()
            .ghost()
            .selected(animation_enabled)
            .tooltip(if animation_enabled {
                "アニメーションを解除"
            } else {
                "色全体をアニメーションする"
            })
            .on_click(move |_, window, cx| {
                inspector.update(cx, |inspector, cx| {
                    inspector.set_animation_enabled(&property, !animation_enabled, window, cx);
                });
            })
        });
        let value = if animation_enabled {
            let from = animation_pickers.get(&(field.target.key.clone(), AnimationEndpoint::From));
            let to = animation_pickers.get(&(field.target.key.clone(), AnimationEndpoint::To));
            match (from, to) {
                (Some(from), Some(to)) => div()
                    .min_w_0()
                    .flex()
                    .flex_1()
                    .gap_1()
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_xs().child("開始"))
                            .child(ColorPicker::new(from).small().w_full()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_xs().child("終了"))
                            .child(ColorPicker::new(to).small().w_full()),
                    )
                    .into_any_element(),
                _ => div().flex_1().into_any_element(),
            }
        } else {
            ColorPicker::new(picker).small().w_full().into_any_element()
        };
        let select_inspector = inspector.clone();
        let select_property = property.clone();
        div()
            .w_full()
            .flex()
            .items_center()
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
                            div()
                                .min_w_0()
                                .flex_1()
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    select_inspector.update(cx, |inspector, cx| {
                                        inspector.select_animation(&select_property, cx);
                                    });
                                })
                                .child(value),
                        )
                    })
                    .when_some(animation_button, |this, button| this.child(button))
                    .when_some(binding_button, |this, button| this.child(button)),
            )
            .into_any_element()
    }
}
