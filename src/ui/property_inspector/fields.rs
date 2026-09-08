use super::*;

pub(super) enum ParameterOwner<'a> {
    Item {
        item: &'a TimelineItem,
        schema: &'a ItemSchema,
    },
    Effect(&'a EffectInstance),
}

impl ParameterOwner<'_> {
    pub(super) fn key(&self, parameter: &ParameterSchema) -> PropertyPath {
        match self {
            Self::Item { item, schema } => PropertyInspector::parameter_key(
                item.plugin_id().unwrap_or_default(),
                schema,
                parameter,
            ),
            Self::Effect(effect) => PropertyInspector::effect_parameter_key(effect, parameter),
        }
    }

    pub(super) fn value(&self, parameter_id: &str) -> Option<&ParameterValue> {
        match self {
            Self::Item { item, .. } => item.parameters.get(parameter_id),
            Self::Effect(effect) => effect.parameters.get(parameter_id),
        }
    }

    pub(super) fn is_size(&self, parameter_id: &str) -> bool {
        match self {
            Self::Item { schema, .. } => schema.is_size_parameter(parameter_id),
            Self::Effect(_) => false,
        }
    }

    pub(super) fn effect_id(&self) -> Option<EffectInstanceId> {
        match self {
            Self::Item { .. } => None,
            Self::Effect(effect) => Some(effect.id),
        }
    }
}

impl PropertyInspector {
    pub(super) fn scaled_display_value(value: impl Into<f64>, scale: impl Into<f64>) -> f64 {
        value.into() * scale.into()
    }

    fn parameter_label(parameter: &ParameterSchema) -> String {
        parameter.label().to_owned()
    }

    pub(super) fn parameter_key(
        plugin_id: &str,
        item_schema: &ItemSchema,
        parameter: &ParameterSchema,
    ) -> PropertyPath {
        PropertyPath::item_parameter(plugin_id, item_schema.id(), parameter.id())
    }

    pub(super) fn selected_schema(item: &TimelineItem) -> Option<&ItemSchema> {
        item.schema()
    }

    pub(super) fn parameter_is_common(items: &[TimelineItem], parameter_id: &str) -> bool {
        let Some(primary) = items.first() else {
            return false;
        };
        let Some(primary_parameter) = primary
            .schema()
            .and_then(|schema| schema.parameter(parameter_id))
        else {
            return false;
        };
        items.iter().skip(1).all(|item| {
            let Some(parameter) = item
                .schema()
                .and_then(|schema| schema.parameter(parameter_id))
            else {
                return false;
            };
            parameter.is_visible() && parameter.ty() == primary_parameter.ty()
        })
    }

    pub(super) fn common_effects(items: &[TimelineItem]) -> Vec<EffectInstance> {
        let Some(primary) = items.first() else {
            return Vec::new();
        };
        primary
            .effects
            .iter()
            .enumerate()
            .filter(|(index, effect)| {
                items.iter().skip(1).all(|item| {
                    item.effects.get(*index).is_some_and(|candidate| {
                        candidate.plugin_id == effect.plugin_id
                            && candidate.effect_id == effect.effect_id
                    })
                })
            })
            .map(|(_, effect)| effect.clone())
            .collect()
    }

    fn array_field(owner: &ParameterOwner<'_>, parameter: &ParameterSchema) -> Option<ArrayField> {
        if !parameter.is_visible() {
            return None;
        }
        let ParameterType::Array { element, .. } = parameter.ty() else {
            return None;
        };
        let ParameterValue::Array(values) = owner.value(parameter.id())? else {
            return None;
        };
        let element_editor = match element {
            ParameterValueType::Scalar(ScalarParameterType::String)
                if parameter.ui().uses_font_family_editor() =>
            {
                ArrayElementEditor::FontFamily
            }
            _ => ArrayElementEditor::Scalar,
        };
        Some(ArrayField {
            target: PropertyTarget {
                key: owner.key(parameter),
                parameter_id: parameter.id().to_owned(),
                effect_id: owner.effect_id(),
                value_path: SceneBindingValuePath::Whole,
            },
            parameter: Box::new(parameter.clone()),
            values: values.clone(),
            element_editor,
            animation_allowed: true,
        })
    }

    pub(super) fn array_fields(item: &TimelineItem) -> Vec<ArrayField> {
        let Some(schema) = item.schema() else {
            return Vec::new();
        };
        schema
            .parameters()
            .iter()
            .filter_map(|parameter| {
                Self::array_field(&ParameterOwner::Item { item, schema }, parameter)
            })
            .collect()
    }

    pub(super) fn effect_array_fields(effect: &EffectInstance) -> Vec<ArrayField> {
        effect
            .schema()
            .parameters()
            .iter()
            .filter_map(|parameter| Self::array_field(&ParameterOwner::Effect(effect), parameter))
            .collect()
    }

    pub(super) fn array_controls(
        field: &ArrayField,
        index: usize,
        value: &ParameterValue,
    ) -> Vec<PropertyControl> {
        let mut controls = Self::scalar_controls(
            field.target.key.clone(),
            &field.parameter,
            value,
            field.target.effect_id,
            Some(index),
            false,
        );
        if !field.animation_allowed {
            for control in &mut controls {
                control.disable_animation();
            }
        }
        controls
    }

    fn array_numbers(field: &ArrayField) -> Vec<NumberField> {
        field
            .values
            .iter()
            .enumerate()
            .flat_map(|(index, value)| {
                Self::number_fields(Self::array_controls(field, index, value))
            })
            .collect()
    }
    pub(super) fn array_property_fields(item: &TimelineItem) -> Vec<NumberField> {
        Self::array_fields(item)
            .iter()
            .flat_map(Self::array_numbers)
            .collect()
    }
    pub(super) fn effect_array_property_fields(effect: &EffectInstance) -> Vec<NumberField> {
        Self::effect_array_fields(effect)
            .iter()
            .flat_map(Self::array_numbers)
            .collect()
    }

    pub(super) fn array_property_field_for_path(
        item: &TimelineItem,
        path: &PropertyPath,
    ) -> Option<NumberField> {
        Self::array_property_fields(item)
            .into_iter()
            .chain(
                item.effects
                    .iter()
                    .flat_map(Self::effect_array_property_fields),
            )
            .find(|field| &field.target.key == path)
    }

    pub(super) fn format_value(value: impl Into<f64>) -> String {
        let value = value.into();
        if value.fract() == 0. {
            format!("{value:.0}")
        } else {
            format!("{value:.2}")
        }
    }

    pub(super) fn normalize_field_value(field: &NumberField, value: f64) -> f64 {
        model::snap_to_step(value, field.input.step).clamp(field.input.min, field.input.max)
    }

    pub(super) fn drag_sensitivity(min: f64, max: f64, step: f64) -> f64 {
        let range = max - min;
        if range == 0. {
            return 0.;
        }
        if !range.is_finite() || range < 0. {
            return step;
        }
        (range / Self::DRAG_RANGE_PIXELS).clamp(
            step * Self::MIN_STEP_MULTIPLIER,
            step * Self::MAX_STEP_MULTIPLIER,
        )
    }

    pub(super) fn numeric_field(
        target: PropertyTarget,
        parameter: &ParameterSchema,
        is_size: bool,
    ) -> Option<NumberField> {
        let tuple_element = target.value_path.tuple_element();
        if !parameter.is_visible() || !parameter.scalar_ui(tuple_element).is_visible() {
            return None;
        }
        let scalar_type = parameter
            .ty()
            .element_type()
            .scalar_at(tuple_element)?
            .clone();
        let ui = parameter.scalar_ui(tuple_element);
        let constraints = parameter.scalar_constraints(tuple_element);
        let (type_min, type_max) = NumericInput::new(scalar_type.clone(), 1.)?.bounds();
        let min = constraints.min.unwrap_or(type_min).max(type_min);
        let max = constraints.max.unwrap_or(type_max).min(type_max);
        let step = f64::from(ui.step());
        let (min, max, step) = match scalar_type {
            ScalarParameterType::I32 | ScalarParameterType::U32 => {
                (min.ceil(), max.floor(), step.max(1.))
            }
            _ => (min, max, step),
        };
        let scale = f64::from(ui.display_scale());
        Some(NumberField {
            target,
            input: NumberInputSettings {
                suffix: ui.unit().to_owned(),
                min: min * scale,
                max: max * scale,
                step: step * scale,
                display_scale: scale,
            },
            label: Self::parameter_label(parameter),
            element_label: parameter.scalar_label(tuple_element),
            animatable: parameter.is_animatable(),
            is_size,
            scalar_type,
            scene_bindable: parameter.is_scene_bindable(),
        })
    }

    pub(super) fn number_fields(controls: Vec<PropertyControl>) -> Vec<NumberField> {
        controls
            .into_iter()
            .flat_map(|control| match control {
                PropertyControl::Number(fields) => fields,
                _ => Vec::new(),
            })
            .collect()
    }

    pub(super) fn property_fields(item: &TimelineItem) -> Vec<NumberField> {
        Self::number_fields(Self::property_controls(item))
    }

    pub(super) fn effect_parameter_key(
        effect: &EffectInstance,
        parameter: &ParameterSchema,
    ) -> PropertyPath {
        PropertyPath::effect_parameter(effect.id.get(), parameter.id())
    }

    pub(super) fn effect_property_fields(effect: &EffectInstance) -> Vec<NumberField> {
        Self::number_fields(Self::effect_property_controls(effect))
    }

    fn colors(controls: Vec<PropertyControl>) -> Vec<ColorField> {
        controls
            .into_iter()
            .flat_map(|control| match control {
                PropertyControl::Color(field) => vec![field],
                PropertyControl::Array(field) => field
                    .values
                    .iter()
                    .enumerate()
                    .flat_map(|(index, value)| {
                        Self::colors(Self::array_controls(&field, index, value))
                    })
                    .collect(),
                _ => Vec::new(),
            })
            .collect()
    }

    pub(super) fn color_fields(item: &TimelineItem) -> Vec<ColorField> {
        Self::colors(Self::property_controls(item))
    }

    pub(super) fn effect_color_fields(effect: &EffectInstance) -> Vec<ColorField> {
        Self::colors(Self::effect_property_controls(effect))
    }

    fn owner_property_controls(
        owner: &ParameterOwner<'_>,
        parameter: &ParameterSchema,
    ) -> Vec<PropertyControl> {
        if let Some(field) = Self::array_field(owner, parameter) {
            return vec![PropertyControl::Array(field)];
        }
        let Some(value) = owner.value(parameter.id()) else {
            return Vec::new();
        };
        let controls = Self::scalar_controls(
            owner.key(parameter),
            parameter,
            value,
            owner.effect_id(),
            None,
            owner.is_size(parameter.id()),
        );
        // Keep homogeneous numeric tuples grouped for the aspect-ratio and per-element UI.
        if controls
            .iter()
            .all(|control| matches!(control, PropertyControl::Number(_)))
        {
            let fields = Self::number_fields(controls);
            if fields.is_empty() {
                Vec::new()
            } else {
                vec![PropertyControl::Number(fields)]
            }
        } else {
            controls
        }
    }

    pub(super) fn property_controls(item: &TimelineItem) -> Vec<PropertyControl> {
        let Some(schema) = Self::selected_schema(item) else {
            return Vec::new();
        };
        schema
            .parameters()
            .iter()
            .flat_map(|parameter| {
                Self::owner_property_controls(&ParameterOwner::Item { item, schema }, parameter)
            })
            .collect()
    }

    pub(super) fn effect_property_controls(effect: &EffectInstance) -> Vec<PropertyControl> {
        effect
            .schema()
            .parameters()
            .iter()
            .flat_map(|parameter| {
                Self::owner_property_controls(&ParameterOwner::Effect(effect), parameter)
            })
            .collect()
    }

    fn scene_controls(
        scene_id: SceneId,
        parameter: &ParameterSchema,
        value: &ParameterValue,
    ) -> Vec<PropertyControl> {
        Self::scalar_controls(
            PropertyPath::scene_parameter(scene_id.get(), parameter.id()),
            parameter,
            value,
            None,
            None,
            false,
        )
    }

    pub(super) fn scene_property_fields(
        scene_id: SceneId,
        arguments: &[SceneArgument],
    ) -> Vec<NumberField> {
        arguments
            .iter()
            .flat_map(|argument| {
                Self::number_fields(Self::scene_controls(
                    scene_id,
                    argument.schema.parameter(),
                    argument.schema.parameter().default_value(),
                ))
            })
            .collect()
    }

    pub(super) fn scene_color_fields(
        scene_id: SceneId,
        arguments: &[SceneArgument],
    ) -> Vec<ColorField> {
        arguments
            .iter()
            .flat_map(|argument| {
                Self::colors(Self::scene_controls(
                    scene_id,
                    argument.schema.parameter(),
                    argument.schema.parameter().default_value(),
                ))
            })
            .collect()
    }

    pub(super) fn selected_scene_property_fields(
        &self,
        item: &TimelineItem,
        cx: &Context<Self>,
    ) -> Vec<NumberField> {
        item.scene_id()
            .and_then(|scene_id| {
                self.editor
                    .read(cx)
                    .scene(scene_id)
                    .map(|scene| Self::scene_property_fields(scene_id, &scene.arguments))
            })
            .unwrap_or_default()
    }

    pub(super) fn scene_property_controls(
        scene_id: SceneId,
        arguments: &[SceneArgument],
        item: &TimelineItem,
    ) -> Vec<PropertyControl> {
        arguments
            .iter()
            .flat_map(|argument| {
                let parameter = argument.schema.parameter();
                item.parameters
                    .get(parameter.id())
                    .map(|value| Self::scene_controls(scene_id, parameter, value))
                    .unwrap_or_default()
            })
            .collect()
    }

    pub(super) fn color_to_hsla(color: [f32; 4]) -> gpui::Hsla {
        Rgba {
            r: color[0],
            g: color[1],
            b: color[2],
            a: color[3],
        }
        .into()
    }
}
