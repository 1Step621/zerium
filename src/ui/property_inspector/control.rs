use super::*;

#[derive(Clone)]
pub(super) struct NumberSpec {
    pub suffix: String,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub scalar_type: ScalarPropertyType,
    pub is_size: bool,
}

#[derive(Clone)]
pub(super) struct LeafControl {
    pub id: ControlId,
    pub target: PropertyTarget,
    pub label: String,
    pub scalar_label: Option<String>,
    pub animatable: bool,
    pub scene_bindable: bool,
    pub read_only: bool,
    pub mixed: bool,
    pub value: PropertyValue,
    pub animation_enabled: bool,
    pub animation_stops: Vec<AnimationStopControl>,
    pub binding: Option<SceneFieldBinding>,
}

#[derive(Clone)]
pub(super) struct AnimationStopControl {
    pub id: ControlId,
    pub index: usize,
    pub property_id: String,
    pub element_id: Option<PropertyElementId>,
    pub scalar_index: Option<usize>,
    pub value: PropertyValue,
    pub value_factor: f64,
}

#[derive(Clone)]
pub(super) struct NumberControl {
    pub common: LeafControl,
    pub spec: NumberSpec,
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
    pub ty: ScalarPropertyType,
    pub options: Vec<(String, u32)>,
}

#[derive(Clone)]
pub(super) struct ColorControl {
    pub common: LeafControl,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ElementKind {
    Scalar,
    FontFamily,
}

#[derive(Clone)]
pub(super) struct ElementGroup {
    pub target: PropertyTarget,
    pub property: Box<PropertySchema>,
    pub elements: Vec<PropertyElement>,
    pub element_kind: ElementKind,
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
    Tuple { size_key: Option<InspectorPath> },
    Elements(Box<ElementGroup>),
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

    pub(super) fn property_id(&self) -> &str {
        self.common()
            .map(|common| common.target.property_id.as_str())
            .or_else(|| match self {
                Self::Group {
                    kind: GroupKind::Elements(group),
                    ..
                } => Some(group.target.property_id.as_str()),
                Self::Group { children, .. } => children.first().map(Self::property_id),
                _ => None,
            })
            .unwrap_or("")
    }

    pub(super) fn disable_animation(&mut self) {
        if let Some(common) = self.common_mut() {
            common.animatable = false;
            common.animation_enabled = false;
            common.animation_stops.clear();
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
    pub playhead: TimelineTime,
    pub editing_scene: bool,
    pub arguments: &'a [SceneArgumentOption],
}

pub(super) enum PropertyOwner<'a> {
    Item {
        item: &'a TimelineItem,
        schema: &'a ItemSchema,
    },
    Effect(&'a EffectInstance),
}

impl PropertyOwner<'_> {
    fn key(&self, property: &PropertySchema) -> InspectorPath {
        match self {
            Self::Item { item, schema } => PropertyInspector::property_key(
                item.plugin_id().unwrap_or_default(),
                schema,
                property,
            ),
            Self::Effect(effect) => PropertyInspector::effect_property_key(effect, property),
        }
    }

    fn value(&self, property_id: &str) -> Option<&PropertyValue> {
        match self {
            Self::Item { item, .. } => item.properties.property(property_id),
            Self::Effect(effect) => effect.properties.property(property_id),
        }
    }

    fn effect_id(&self) -> Option<EffectInstanceId> {
        match self {
            Self::Item { .. } => None,
            Self::Effect(effect) => Some(effect.id),
        }
    }

    fn is_size(&self, property_id: &str) -> bool {
        match self {
            Self::Item { schema, .. } => schema.is_size_property(property_id),
            Self::Effect(_) => false,
        }
    }
}

impl PropertyInspector {
    pub(super) fn property_key(
        plugin_id: &str,
        item_schema: &ItemSchema,
        property: &PropertySchema,
    ) -> InspectorPath {
        InspectorPath::item_property(plugin_id, item_schema.id(), property.id())
    }

    pub(super) fn effect_property_key(
        effect: &EffectInstance,
        property: &PropertySchema,
    ) -> InspectorPath {
        InspectorPath::effect_property(effect.id.get(), property.id())
    }

    pub(super) fn selected_schema(item: &TimelineItem) -> Option<&ItemSchema> {
        item.schema()
    }

    pub(super) fn property_is_common(items: &[TimelineItem], property_id: &str) -> bool {
        let Some(primary) = items.first() else {
            return false;
        };
        let Some(primary_property) = primary
            .schema()
            .and_then(|schema| schema.property(property_id))
        else {
            return false;
        };
        items.iter().skip(1).all(|item| {
            let Some(property) = item
                .schema()
                .and_then(|schema| schema.property(property_id))
            else {
                return false;
            };
            property.is_visible() && property.ty() == primary_property.ty()
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
        property: &PropertySchema,
        scalar_index: Option<usize>,
        is_size: bool,
    ) -> Option<NumberSpec> {
        if !property.is_visible() || !property.configuration_ui(scalar_index).is_visible() {
            return None;
        }
        let value_type = match property.ty() {
            PropertyType::Value(value_type)
            | PropertyType::Array {
                element_type: value_type,
                ..
            } => value_type,
        };
        let scalar_type = value_type.scalar_at(scalar_index)?.clone();
        if !matches!(
            scalar_type,
            ScalarPropertyType::F32 | ScalarPropertyType::I32 | ScalarPropertyType::U32
        ) {
            return None;
        }
        let ui = property.configuration_ui(scalar_index);
        let constraints = property.configuration_constraints(scalar_index);
        let (type_min, type_max) = NumericInput::new(scalar_type.clone())?.bounds();
        let min = constraints.min.unwrap_or(type_min).max(type_min);
        let max = constraints.max.unwrap_or(type_max).min(type_max);
        let step = f64::from(ui.step());
        let (min, max, step) = match scalar_type {
            ScalarPropertyType::I32 | ScalarPropertyType::U32 => {
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
        key: &InspectorPath,
        property: &PropertySchema,
        value: PropertyValue,
        effect_id: Option<EffectInstanceId>,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
        label: String,
    ) -> LeafControl {
        LeafControl {
            id: ControlId::property(key),
            target: PropertyTarget {
                key: key.clone(),
                property_id: property.id().to_owned(),
                effect_id,
                path: InspectorPath::new(element_id, scalar_index),
            },
            label,
            scalar_label: property.configuration_label(scalar_index),
            animatable: property.is_animatable(scalar_index),
            scene_bindable: property.is_scene_bindable(scalar_index),
            read_only: !property.is_editable(scalar_index),
            mixed: false,
            value,
            animation_enabled: false,
            animation_stops: Vec::new(),
            binding: None,
        }
    }

    fn scalar_controls(
        key: InspectorPath,
        property: &PropertySchema,
        value: &PropertyValue,
        effect_id: Option<EffectInstanceId>,
        element: Option<(usize, PropertyElementId)>,
        is_size: bool,
        resolution: &ControlResolution<'_>,
    ) -> Vec<Control> {
        if !property.is_visible() {
            return Vec::new();
        }
        let label = element.map_or_else(
            || property.label().to_owned(),
            |(index, _)| format!("{} {}", property.label(), index + 1),
        );
        let ty = match property.ty() {
            PropertyType::Value(value_type)
            | PropertyType::Array {
                element_type: value_type,
                ..
            } => value_type,
        };
        ty.scalars()
            .filter_map(|(scalar_index, scalar_type)| {
                let value = value.scalar_at(scalar_index)?.clone();
                let scalar_ui = property.configuration_ui(scalar_index);
                if !scalar_ui.is_visible() {
                    return None;
                }
                let label = scalar_index.map_or_else(
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
                let scalar_key = key.scalar(element.map(|(index, _)| index), scalar_index);
                let common = Self::scalar_common(
                    &scalar_key,
                    property,
                    value.clone(),
                    effect_id,
                    element.map(|(_, id)| id),
                    scalar_index,
                    label,
                );
                let mut control = match (scalar_type, value) {
                    (
                        ScalarPropertyType::F32 | ScalarPropertyType::I32 | ScalarPropertyType::U32,
                        _value,
                    ) => Control::Number(NumberControl {
                        common,
                        spec: Self::number_spec(property, scalar_index, is_size)?,
                    }),
                    (ScalarPropertyType::Color, PropertyValue::Color(_)) => {
                        Control::Color(ColorControl { common })
                    }
                    (ScalarPropertyType::Bool, PropertyValue::Bool(_)) => {
                        Control::Bool(BoolControl { common })
                    }
                    (ScalarPropertyType::String, PropertyValue::String(_)) => {
                        Control::Text(TextControl {
                            common,
                            multiline: scalar_ui.is_multiline(),
                        })
                    }
                    (ScalarPropertyType::Enum(_), PropertyValue::Enum(_)) => {
                        Control::Choice(ChoiceControl {
                            common,
                            ty: scalar_type.clone(),
                            options: scalar_ui
                                .enum_options(scalar_type)?
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
        size_key: Option<InspectorPath>,
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
        owner: &PropertyOwner<'_>,
        property: &PropertySchema,
        resolution: &ControlResolution<'_>,
    ) -> Vec<Control> {
        let key = owner.key(property);
        if let Some(group) = Self::elements_group(owner, property, key.clone(), resolution) {
            return vec![group];
        }
        let Some(value) = owner.value(property.id()) else {
            return Vec::new();
        };
        let controls = Self::scalar_controls(
            key.clone(),
            property,
            value,
            owner.effect_id(),
            None,
            owner.is_size(property.id()),
            resolution,
        );
        if matches!(
            property.ty(),
            PropertyType::Value(PropertyValueType::Tuple(_))
                | PropertyType::Array {
                    element_type: PropertyValueType::Tuple(_),
                    ..
                }
        ) {
            Self::group_controls(
                ControlId::group(&key),
                property.label().to_owned(),
                controls,
                owner.is_size(property.id()).then(|| key.clone()),
            )
        } else {
            controls
        }
    }

    fn elements_group(
        owner: &PropertyOwner<'_>,
        property: &PropertySchema,
        key: InspectorPath,
        resolution: &ControlResolution<'_>,
    ) -> Option<Control> {
        if !property.is_visible() {
            return None;
        }
        let PropertyType::Array {
            element_type,
            min_items,
            max_items,
        } = property.ty()
        else {
            return None;
        };
        let PropertyValue::Array(values) = owner.value(property.id())? else {
            return None;
        };
        let element_kind = match element_type {
            PropertyValueType::Scalar(ScalarPropertyType::String)
                if property.configuration_ui(None).uses_font_family_editor() =>
            {
                ElementKind::FontFamily
            }
            _ => ElementKind::Scalar,
        };
        let target = PropertyTarget {
            key: key.clone(),
            property_id: property.id().to_owned(),
            effect_id: owner.effect_id(),
            path: key.clone(),
        };
        let has_scene_binding = resolution.arguments.iter().any(|argument| {
            argument.bindings.iter().any(|binding| {
                binding.item_id() == resolution.item.id
                    && binding.owner() == SceneBindingOwner::from_effect(target.effect_id)
                    && binding.property_id() == target.property_id
                    && binding.element_id().is_some()
            })
        });
        let children = values
            .iter()
            .enumerate()
            .map(|(element_index, element)| {
                let row_controls = Self::scalar_controls(
                    key.clone(),
                    property,
                    element.value(),
                    owner.effect_id(),
                    Some((element_index, element.element_id())),
                    false,
                    resolution,
                );
                Control::Group {
                    id: ControlId::group(&key.scalar(Some(element_index), None)),
                    label: format!("要素 {}", element_index + 1),
                    children: row_controls,
                    kind: GroupKind::Plain,
                }
            })
            .collect();
        Some(Control::Group {
            id: ControlId::group(&key),
            label: property.label().to_owned(),
            children,
            kind: GroupKind::Elements(Box::new(ElementGroup {
                target,
                property: Box::new(property.clone()),
                elements: values.clone(),
                element_kind,
                min_items: *min_items,
                max_items: *max_items,
                has_scene_binding,
            })),
        })
    }

    pub(super) fn item_controls(
        item: &TimelineItem,
        resolution: &ControlResolution<'_>,
    ) -> Vec<Control> {
        let Some(schema) = Self::selected_schema(item) else {
            return Vec::new();
        };
        let owner = PropertyOwner::Item { item, schema };
        schema
            .properties()
            .iter()
            .flat_map(|property| Self::owner_controls(&owner, property, resolution))
            .collect()
    }

    pub(super) fn effect_controls(
        effect: &EffectInstance,
        resolution: &ControlResolution<'_>,
    ) -> Vec<Control> {
        let owner = PropertyOwner::Effect(effect);
        effect
            .schema()
            .properties()
            .iter()
            .flat_map(|property| Self::owner_controls(&owner, property, resolution))
            .collect()
    }

    fn scene_value_controls(
        scene_id: SceneId,
        property: &PropertySchema,
        value: &PropertyValue,
        resolution: &ControlResolution<'_>,
    ) -> Vec<Control> {
        let key = InspectorPath::scene_property(scene_id.get(), property.id());
        let controls =
            Self::scalar_controls(key.clone(), property, value, None, None, false, resolution);
        if matches!(
            property.ty(),
            PropertyType::Value(PropertyValueType::Tuple(_))
                | PropertyType::Array {
                    element_type: PropertyValueType::Tuple(_),
                    ..
                }
        ) {
            Self::group_controls(
                ControlId::group(&key),
                property.label().to_owned(),
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
        values: &HashMap<String, PropertyValue>,
        resolution: &ControlResolution<'_>,
    ) -> Vec<Control> {
        arguments
            .iter()
            .flat_map(|argument| {
                values
                    .get(argument.schema.id())
                    .map(|value| {
                        Self::scene_value_controls(
                            scene_id,
                            argument.schema.property(),
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
        scalar_type: ScalarPropertyType,
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
                common.target.property_id.clone(),
                common.target.path.element_id(),
                common.target.path.scalar_index(),
            ),
            &PropertyType::Value(PropertyValueType::Scalar(scalar_type)),
            resolution.arguments,
        );
        if matches!(common.value, PropertyValue::Bool(_))
            && common.target.effect_id.is_none()
            && let PropertyValue::Bool(value) = common.value
        {
            common.mixed = resolution.selected_items.iter().skip(1).any(|selected| {
                selected
                    .properties
                    .property(&common.target.property_id)
                    .and_then(|value| value.scalar_at(common.target.path.scalar_index()))
                    != Some(&PropertyValue::Bool(value))
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn animation_stop_controls(
        item: &TimelineItem,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
        property: &InspectorPath,
        value_factor: f64,
        time: TimelineTime,
    ) -> Vec<AnimationStopControl> {
        let progress = item.animation_progress_at_time(time);
        let Some(track) = item.animation_track(effect_id, property_id, element_id, scalar_index)
        else {
            return Vec::new();
        };
        track
            .stop_indices_for_segment(progress)
            .into_iter()
            .filter_map(|index| {
                let stop = track.stops().get(index)?;
                let value = if value_factor == 1. {
                    stop.value().clone()
                } else {
                    let value = stop.value().numeric_scalar()? * value_factor;
                    if !value.is_finite() {
                        return None;
                    }
                    stop.value().with_numeric_scalar(value)?
                };
                Some(AnimationStopControl {
                    id: ControlId::animation_stop(property, index),
                    index,
                    property_id: property_id.to_owned(),
                    element_id,
                    scalar_index,
                    value,
                    value_factor,
                })
            })
            .collect()
    }

    fn resolve_leaf(resolution: &ControlResolution<'_>, control: &mut Control) {
        match control {
            Control::Group { .. } => {}
            Control::Number(number) => {
                let animation_source = Self::number_animation_source(
                    resolution.item,
                    &number.common.target,
                    &number.spec,
                );
                let animation_enabled = animation_source.is_some();
                Self::resolve_common(
                    resolution,
                    &mut number.common,
                    number.spec.scalar_type.clone(),
                    animation_enabled,
                );
                number.common.animation_enabled = animation_enabled;
                number.common.animation_stops =
                    animation_source.as_ref().map_or_else(Vec::new, |source| {
                        Self::animation_stop_controls(
                            resolution.item,
                            number.common.target.effect_id,
                            &source.property_id,
                            source.element_id,
                            source.scalar_index,
                            &number.common.target.key,
                            source.value_factor,
                            resolution.playhead,
                        )
                    });
            }
            Control::Text(text) => Self::resolve_common(
                resolution,
                &mut text.common,
                ScalarPropertyType::String,
                false,
            ),
            Control::Bool(boolean) => Self::resolve_common(
                resolution,
                &mut boolean.common,
                ScalarPropertyType::Bool,
                false,
            ),
            Control::Choice(choice) => {
                Self::resolve_common(resolution, &mut choice.common, choice.ty.clone(), false)
            }
            Control::Color(color) => {
                let animation_enabled = color.common.target.animation_enabled(resolution.item);
                Self::resolve_common(
                    resolution,
                    &mut color.common,
                    ScalarPropertyType::Color,
                    animation_enabled,
                );
                color.common.animation_enabled = animation_enabled;
                if animation_enabled {
                    color.common.animation_stops = Self::animation_stop_controls(
                        resolution.item,
                        color.common.target.effect_id,
                        &color.common.target.property_id,
                        color.common.target.path.element_id(),
                        color.common.target.path.scalar_index(),
                        &color.common.target.key,
                        1.,
                        resolution.playhead,
                    );
                }
            }
        }
    }

    pub(super) fn scene_field_binding(
        editing_scene: bool,
        animation_enabled: bool,
        scene_bindable: bool,
        target: SceneBindingTarget,
        ty: &PropertyType,
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
        let size = item.schema()?.size_property()?;
        if !size.is_editable(None) {
            return None;
        }
        if !Self::property_is_common(selected_items, size.id())
            || selected_items.iter().any(|selected| {
                selected.schema().is_none_or(|schema| {
                    !schema.supports_aspect_ratio_lock()
                        || schema
                            .size_property()
                            .is_none_or(|property| !property.is_editable(None))
                })
            })
        {
            return None;
        }
        let size_is_bound = scene_arguments.iter().any(|argument| {
            argument.bindings.iter().any(|binding| {
                binding.item_id() == item.id
                    && binding.owner() == SceneBindingOwner::Item
                    && binding.property_id() == size.id()
                    && binding.element_id().is_none()
                    && binding.scalar_index().is_none()
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
