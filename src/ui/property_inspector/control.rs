use super::*;

#[derive(Clone)]
pub(super) struct NumberSpec {
    pub suffix: String,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub scalar_type: ScalarParameterType,
    pub is_size: bool,
}

#[derive(Clone)]
pub(super) struct LeafControl {
    pub id: ControlId,
    pub target: PropertyTarget,
    pub label: String,
    pub element_label: Option<String>,
    pub animatable: bool,
    pub scene_bindable: bool,
    pub mixed: bool,
    pub value: ParameterValue,
    pub animation_enabled: bool,
    pub binding: Option<SceneFieldBinding>,
}

#[derive(Clone)]
pub(super) struct NumberControl {
    pub common: LeafControl,
    pub spec: NumberSpec,
    pub animation: Option<NumberAnimationDisplay>,
}

#[derive(Clone)]
pub(super) struct TextControl {
    pub common: LeafControl,
    pub multiline: bool,
}

#[derive(Clone)]
pub(super) struct BoolControl {
    pub common: LeafControl,
}

#[derive(Clone)]
pub(super) struct ChoiceControl {
    pub common: LeafControl,
    pub ty: ScalarParameterType,
    pub options: Vec<(String, u32)>,
}

#[derive(Clone)]
pub(super) struct ColorControl {
    pub common: LeafControl,
    pub animation: Option<ColorAnimationDisplay>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ArrayElementKind {
    Scalar,
    FontFamily,
}

#[derive(Clone)]
pub(super) struct ArrayGroup {
    pub target: PropertyTarget,
    pub parameter: Box<ParameterSchema>,
    pub values: Vec<ParameterValue>,
    pub element_kind: ArrayElementKind,
    pub min_items: u32,
    pub max_items: u32,
    pub has_scene_binding: bool,
}

#[derive(Clone)]
pub(super) struct EffectGroup {
    pub id: EffectInstanceId,
    pub label: String,
    pub hidden: bool,
}

#[derive(Clone)]
pub(super) enum GroupKind {
    Plain,
    Tuple { size_key: Option<PropertyPath> },
    Array(ArrayGroup),
    Effect(EffectGroup),
}

#[derive(Clone)]
pub(super) enum Control {
    Group {
        id: ControlId,
        label: String,
        children: Vec<Control>,
        kind: GroupKind,
    },
    Number(NumberControl),
    Text(TextControl),
    Bool(BoolControl),
    Choice(ChoiceControl),
    Color(ColorControl),
}

impl Control {
    pub(super) fn id(&self) -> &ControlId {
        match self {
            Self::Group { id, .. } => id,
            Self::Number(control) => &control.common.id,
            Self::Text(control) => &control.common.id,
            Self::Bool(control) => &control.common.id,
            Self::Choice(control) => &control.common.id,
            Self::Color(control) => &control.common.id,
        }
    }

    pub(super) fn common(&self) -> Option<&LeafControl> {
        match self {
            Self::Number(control) => Some(&control.common),
            Self::Text(control) => Some(&control.common),
            Self::Bool(control) => Some(&control.common),
            Self::Choice(control) => Some(&control.common),
            Self::Color(control) => Some(&control.common),
            Self::Group { .. } => None,
        }
    }

    pub(super) fn common_mut(&mut self) -> Option<&mut LeafControl> {
        match self {
            Self::Number(control) => Some(&mut control.common),
            Self::Text(control) => Some(&mut control.common),
            Self::Bool(control) => Some(&mut control.common),
            Self::Choice(control) => Some(&mut control.common),
            Self::Color(control) => Some(&mut control.common),
            Self::Group { .. } => None,
        }
    }

    pub(super) fn parameter_id(&self) -> &str {
        self.common()
            .map(|common| common.target.parameter_id.as_str())
            .or_else(|| match self {
                Self::Group {
                    kind: GroupKind::Array(array),
                    ..
                } => Some(array.target.parameter_id.as_str()),
                Self::Group { children, .. } => children.first().map(Self::parameter_id),
                _ => None,
            })
            .unwrap_or("")
    }

    pub(super) fn disable_animation(&mut self) {
        if let Some(common) = self.common_mut() {
            common.animatable = false;
            common.animation_enabled = false;
        }
        if let Self::Group { children, .. } = self {
            for child in children {
                child.disable_animation();
            }
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct ControlTree {
    pub roots: Vec<Control>,
}

pub(super) struct ControlResolution<'a> {
    pub item: &'a TimelineItem,
    pub selected_items: &'a [TimelineItem],
    pub editing_scene: bool,
    pub arguments: &'a [SceneArgumentOption],
}

pub(super) enum ParameterOwner<'a> {
    Item {
        item: &'a TimelineItem,
        schema: &'a ItemSchema,
    },
    Effect(&'a EffectInstance),
}

impl ParameterOwner<'_> {
    fn key(&self, parameter: &ParameterSchema) -> PropertyPath {
        match self {
            Self::Item { item, schema } => PropertyInspector::parameter_key(
                item.plugin_id().unwrap_or_default(),
                schema,
                parameter,
            ),
            Self::Effect(effect) => PropertyInspector::effect_parameter_key(effect, parameter),
        }
    }

    fn value(&self, parameter_id: &str) -> Option<&ParameterValue> {
        match self {
            Self::Item { item, .. } => item.parameters.get(parameter_id),
            Self::Effect(effect) => effect.parameters.get(parameter_id),
        }
    }

    fn effect_id(&self) -> Option<EffectInstanceId> {
        match self {
            Self::Item { .. } => None,
            Self::Effect(effect) => Some(effect.id),
        }
    }

    fn is_size(&self, parameter_id: &str) -> bool {
        match self {
            Self::Item { schema, .. } => schema.is_size_parameter(parameter_id),
            Self::Effect(_) => false,
        }
    }
}

impl PropertyInspector {
    pub(super) fn parameter_key(
        plugin_id: &str,
        item_schema: &ItemSchema,
        parameter: &ParameterSchema,
    ) -> PropertyPath {
        PropertyPath::item_parameter(plugin_id, item_schema.id(), parameter.id())
    }

    pub(super) fn effect_parameter_key(
        effect: &EffectInstance,
        parameter: &ParameterSchema,
    ) -> PropertyPath {
        PropertyPath::effect_parameter(effect.id.get(), parameter.id())
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

    pub(super) fn format_value(value: impl Into<f64>) -> String {
        let value = value.into();
        if value.fract() == 0. {
            format!("{value:.0}")
        } else {
            format!("{value:.2}")
        }
    }

    pub(super) fn normalize_field_value(spec: &NumberSpec, value: f64) -> f64 {
        model::snap_to_step(value, spec.step).clamp(spec.min, spec.max)
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

    pub(super) fn color_to_hsla(color: [f32; 4]) -> gpui::Hsla {
        Rgba {
            r: color[0],
            g: color[1],
            b: color[2],
            a: color[3],
        }
        .into()
    }

    pub(super) fn number_spec(
        parameter: &ParameterSchema,
        tuple_element: Option<usize>,
        is_size: bool,
    ) -> Option<NumberSpec> {
        if !parameter.is_visible() || !parameter.scalar_ui(tuple_element).is_visible() {
            return None;
        }
        let scalar_type = parameter
            .ty()
            .element_type()
            .scalar_at(tuple_element)?
            .clone();
        if !matches!(
            scalar_type,
            ScalarParameterType::F32 | ScalarParameterType::I32 | ScalarParameterType::U32
        ) {
            return None;
        }
        let ui = parameter.scalar_ui(tuple_element);
        let constraints = parameter.scalar_constraints(tuple_element);
        let (type_min, type_max) = NumericInput::new(scalar_type.clone())?.bounds();
        let min = constraints.min.unwrap_or(type_min).max(type_min);
        let max = constraints.max.unwrap_or(type_max).min(type_max);
        let step = f64::from(ui.step());
        let (min, max, step) = match scalar_type {
            ScalarParameterType::I32 | ScalarParameterType::U32 => {
                (min.ceil(), max.floor(), step.max(1.))
            }
            _ => (min, max, step),
        };
        Some(NumberSpec {
            suffix: ui.unit().to_owned(),
            min,
            max,
            step,
            scalar_type,
            is_size,
        })
    }

    fn scalar_common(
        key: &PropertyPath,
        parameter: &ParameterSchema,
        value: ParameterValue,
        effect_id: Option<EffectInstanceId>,
        array: Option<usize>,
        element: Option<usize>,
        label: String,
    ) -> LeafControl {
        LeafControl {
            id: ControlId::property(key),
            target: PropertyTarget {
                key: key.clone(),
                parameter_id: parameter.id().to_owned(),
                effect_id,
                value_path: SceneBindingValuePath::from_elements(array, element),
            },
            label,
            element_label: parameter.scalar_label(element),
            animatable: parameter.is_animatable(),
            scene_bindable: parameter.is_scene_bindable(),
            mixed: false,
            value,
            animation_enabled: false,
            binding: None,
        }
    }

    fn scalar_controls(
        key: PropertyPath,
        parameter: &ParameterSchema,
        value: &ParameterValue,
        effect_id: Option<EffectInstanceId>,
        array: Option<usize>,
        is_size: bool,
        resolution: &ControlResolution<'_>,
    ) -> Vec<Control> {
        if !parameter.is_visible() {
            return Vec::new();
        }
        let label = array.map_or_else(
            || parameter.label().to_owned(),
            |index| format!("{} {}", parameter.label(), index + 1),
        );
        let ty = parameter.ty().element_type();
        let ui = parameter.ui();
        ty.scalars()
            .filter_map(|(element, ty)| {
                let value = value.scalar_at(element)?.clone();
                let scalar_ui = element.map_or(ui, |index| ui.for_element(index));
                if !scalar_ui.is_visible() {
                    return None;
                }
                let label = element.map_or_else(
                    || label.clone(),
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
                let scalar_key = key.scalar(array, element);
                let common = Self::scalar_common(
                    &scalar_key,
                    parameter,
                    value.clone(),
                    effect_id,
                    array,
                    element,
                    label,
                );
                let mut control = match (ty, value) {
                    (
                        ScalarParameterType::F32
                        | ScalarParameterType::I32
                        | ScalarParameterType::U32,
                        _value,
                    ) => Control::Number(NumberControl {
                        common,
                        spec: Self::number_spec(parameter, element, is_size)?,
                        animation: None,
                    }),
                    (ScalarParameterType::Color, ParameterValue::Color(_)) => {
                        Control::Color(ColorControl {
                            common,
                            animation: None,
                        })
                    }
                    (ScalarParameterType::Bool, ParameterValue::Bool(_)) => {
                        Control::Bool(BoolControl { common })
                    }
                    (ScalarParameterType::String, ParameterValue::String(_)) => {
                        Control::Text(TextControl {
                            common,
                            multiline: scalar_ui.is_multiline(),
                        })
                    }
                    (ScalarParameterType::Enum(_), ParameterValue::Enum(_)) => {
                        Control::Choice(ChoiceControl {
                            common,
                            ty: ty.clone(),
                            options: scalar_ui
                                .enum_options(ty)?
                                .into_iter()
                                .map(|(value, label)| (label, value))
                                .collect(),
                        })
                    }
                    _ => return None,
                };
                Self::resolve_leaf(resolution, &mut control);
                Some(control)
            })
            .collect()
    }

    fn group_controls(
        id: ControlId,
        label: String,
        children: Vec<Control>,
        size_key: Option<PropertyPath>,
    ) -> Vec<Control> {
        if children.is_empty() {
            return children;
        }
        vec![Control::Group {
            id,
            label,
            children,
            kind: GroupKind::Tuple { size_key },
        }]
    }

    fn owner_controls(
        owner: &ParameterOwner<'_>,
        parameter: &ParameterSchema,
        resolution: &ControlResolution<'_>,
    ) -> Vec<Control> {
        let key = owner.key(parameter);
        if let Some(array) = Self::array_group(owner, parameter, key.clone(), resolution) {
            return vec![array];
        }
        let Some(value) = owner.value(parameter.id()) else {
            return Vec::new();
        };
        let controls = Self::scalar_controls(
            key.clone(),
            parameter,
            value,
            owner.effect_id(),
            None,
            owner.is_size(parameter.id()),
            resolution,
        );
        if matches!(parameter.ty().element_type(), ParameterValueType::Tuple(_)) {
            Self::group_controls(
                ControlId::group(&key),
                parameter.label().to_owned(),
                controls,
                owner.is_size(parameter.id()).then(|| key.clone()),
            )
        } else {
            controls
        }
    }

    fn array_group(
        owner: &ParameterOwner<'_>,
        parameter: &ParameterSchema,
        key: PropertyPath,
        resolution: &ControlResolution<'_>,
    ) -> Option<Control> {
        if !parameter.is_visible() {
            return None;
        }
        let ParameterType::Array {
            element,
            min_items,
            max_items,
        } = parameter.ty()
        else {
            return None;
        };
        let ParameterValue::Array(values) = owner.value(parameter.id())? else {
            return None;
        };
        let element_kind = match element {
            ParameterValueType::Scalar(ScalarParameterType::String)
                if parameter.ui().uses_font_family_editor() =>
            {
                ArrayElementKind::FontFamily
            }
            _ => ArrayElementKind::Scalar,
        };
        let target = PropertyTarget {
            key: key.clone(),
            parameter_id: parameter.id().to_owned(),
            effect_id: owner.effect_id(),
            value_path: SceneBindingValuePath::Whole,
        };
        let has_scene_binding = resolution.arguments.iter().any(|argument| {
            argument.bindings.iter().any(|binding| {
                binding.item_id() == resolution.item.id
                    && binding.owner() == SceneBindingOwner::from_effect(target.effect_id)
                    && binding.parameter_id() == target.parameter_id
                    && binding.value_path().array_element().is_some()
            })
        });
        let children = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let element_controls = Self::scalar_controls(
                    key.clone(),
                    parameter,
                    value,
                    owner.effect_id(),
                    Some(index),
                    false,
                    resolution,
                );
                Control::Group {
                    id: ControlId::group(&key.scalar(Some(index), None)),
                    label: format!("要素 {}", index + 1),
                    children: element_controls,
                    kind: GroupKind::Plain,
                }
            })
            .collect();
        Some(Control::Group {
            id: ControlId::group(&key),
            label: parameter.label().to_owned(),
            children,
            kind: GroupKind::Array(ArrayGroup {
                target,
                parameter: Box::new(parameter.clone()),
                values: values.clone(),
                element_kind,
                min_items: *min_items,
                max_items: *max_items,
                has_scene_binding,
            }),
        })
    }

    pub(super) fn item_controls(
        item: &TimelineItem,
        resolution: &ControlResolution<'_>,
    ) -> Vec<Control> {
        let Some(schema) = Self::selected_schema(item) else {
            return Vec::new();
        };
        let owner = ParameterOwner::Item { item, schema };
        schema
            .parameters()
            .iter()
            .flat_map(|parameter| Self::owner_controls(&owner, parameter, resolution))
            .collect()
    }

    pub(super) fn effect_controls(
        effect: &EffectInstance,
        resolution: &ControlResolution<'_>,
    ) -> Vec<Control> {
        let owner = ParameterOwner::Effect(effect);
        effect
            .schema()
            .parameters()
            .iter()
            .flat_map(|parameter| Self::owner_controls(&owner, parameter, resolution))
            .collect()
    }

    fn scene_value_controls(
        scene_id: SceneId,
        parameter: &ParameterSchema,
        value: &ParameterValue,
        resolution: &ControlResolution<'_>,
    ) -> Vec<Control> {
        let key = PropertyPath::scene_parameter(scene_id.get(), parameter.id());
        let controls =
            Self::scalar_controls(key.clone(), parameter, value, None, None, false, resolution);
        if matches!(parameter.ty().element_type(), ParameterValueType::Tuple(_)) {
            Self::group_controls(
                ControlId::group(&key),
                parameter.label().to_owned(),
                controls,
                None,
            )
        } else {
            controls
        }
    }

    pub(super) fn scene_argument_value_controls(
        scene_id: SceneId,
        arguments: &[SceneArgument],
        item: &TimelineItem,
        resolution: &ControlResolution<'_>,
    ) -> Vec<Control> {
        arguments
            .iter()
            .flat_map(|argument| {
                item.parameters
                    .get(argument.schema.id())
                    .map(|value| {
                        Self::scene_value_controls(
                            scene_id,
                            argument.schema.parameter(),
                            value,
                            resolution,
                        )
                    })
                    .unwrap_or_default()
            })
            .collect()
    }

    fn resolve_common(
        resolution: &ControlResolution<'_>,
        common: &mut LeafControl,
        scalar_type: ScalarParameterType,
        animation_visible: bool,
    ) {
        common.animation_enabled = common.target.animation_enabled(resolution.item);
        common.binding = Self::scene_field_binding(
            resolution.editing_scene,
            animation_visible,
            common.scene_bindable,
            SceneBindingTarget::new(
                resolution.item.id,
                SceneBindingOwner::from_effect(common.target.effect_id),
                common.target.parameter_id.clone(),
                common.target.value_path,
            ),
            &ParameterType::Value(ParameterValueType::Scalar(scalar_type)),
            resolution.arguments,
        );
        if matches!(common.value, ParameterValue::Bool(_))
            && common.target.effect_id.is_none()
            && let ParameterValue::Bool(value) = common.value
        {
            common.mixed = resolution.selected_items.iter().skip(1).any(|selected| {
                selected
                    .parameters
                    .get(&common.target.parameter_id)
                    .and_then(|value| value.scalar_at(common.target.value_path.tuple_element()))
                    != Some(&ParameterValue::Bool(value))
            });
        }
    }

    fn resolve_leaf(resolution: &ControlResolution<'_>, control: &mut Control) {
        match control {
            Control::Group { .. } => {}
            Control::Number(number) => {
                number.animation =
                    Self::number_animation(resolution.item, &number.common.target, &number.spec);
                Self::resolve_common(
                    resolution,
                    &mut number.common,
                    number.spec.scalar_type.clone(),
                    number.animation.is_some(),
                );
                number.common.animation_enabled = number.animation.is_some();
            }
            Control::Text(text) => Self::resolve_common(
                resolution,
                &mut text.common,
                ScalarParameterType::String,
                false,
            ),
            Control::Bool(boolean) => Self::resolve_common(
                resolution,
                &mut boolean.common,
                ScalarParameterType::Bool,
                false,
            ),
            Control::Choice(choice) => {
                Self::resolve_common(resolution, &mut choice.common, choice.ty.clone(), false)
            }
            Control::Color(color) => {
                color.animation = Self::color_animation(resolution.item, &color.common.target);
                Self::resolve_common(
                    resolution,
                    &mut color.common,
                    ScalarParameterType::Color,
                    color.animation.is_some(),
                );
                color.common.animation_enabled = color.animation.is_some();
            }
        }
    }

    fn color_animation(
        item: &TimelineItem,
        target: &PropertyTarget,
    ) -> Option<ColorAnimationDisplay> {
        let animation = item.animation(
            target.effect_id,
            &target.parameter_id,
            target.value_path.array_element(),
        )?;
        let (from, to) = animation.endpoints(target.animation_address().channel)?;
        let (ParameterValue::Color(from), ParameterValue::Color(to)) = (from, to) else {
            return None;
        };
        Some(ColorAnimationDisplay {
            from: *from,
            to: *to,
        })
    }

    pub(super) fn scene_field_binding(
        editing_scene: bool,
        animation_enabled: bool,
        scene_bindable: bool,
        target: SceneBindingTarget,
        ty: &ParameterType,
        arguments: &[SceneArgumentOption],
    ) -> Option<SceneFieldBinding> {
        if !editing_scene || animation_enabled || !scene_bindable {
            return None;
        }
        let connected = arguments.iter().find_map(|argument| {
            argument
                .bindings
                .contains(&target)
                .then(|| (argument.id.clone(), argument.label.clone()))
        });
        let compatible = arguments
            .iter()
            .filter(|argument| argument.schema.ty() == ty)
            .map(|argument| (argument.id.clone(), argument.label.clone()))
            .collect();
        Some(SceneFieldBinding {
            target,
            connected,
            compatible,
        })
    }

    pub(super) fn aspect_ratio_lock_state(
        item: &TimelineItem,
        selected_items: &[TimelineItem],
        scene_arguments: &[SceneArgumentOption],
        editing_scene: bool,
    ) -> Option<AspectRatioLockState> {
        if item.scene_id().is_some() {
            return None;
        }
        let size = item.schema()?.size_parameter()?;
        if !Self::parameter_is_common(selected_items, size.id())
            || selected_items.iter().any(|selected| {
                selected
                    .schema()
                    .is_none_or(|schema| !schema.supports_aspect_ratio_lock())
            })
        {
            return None;
        }
        let size_is_bound = scene_arguments.iter().any(|argument| {
            argument.bindings.iter().any(|binding| {
                binding.item_id() == item.id
                    && binding.owner() == SceneBindingOwner::Item
                    && binding.parameter_id() == size.id()
                    && binding.value_path() == SceneBindingValuePath::Whole
            })
        });
        Some(AspectRatioLockState {
            value: item.aspect_ratio_locked,
            mixed: selected_items
                .iter()
                .skip(1)
                .any(|selected| selected.aspect_ratio_locked != item.aspect_ratio_locked),
            multiple: selected_items.len() > 1,
            disabled_by_scene_size_argument: editing_scene && size_is_bound,
        })
    }
}
