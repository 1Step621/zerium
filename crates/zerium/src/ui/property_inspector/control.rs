use rust_i18n::t;

use super::*;

#[derive(Clone)]
pub(super) struct LeafControl {
    pub id: ControlId,
    pub target: PropertyAddress,
    pub label: String,
    pub scalar_label: Option<String>,
    pub animatable: bool,
    pub read_only: bool,
    pub value: PropertyValue,
    pub animation_enabled: bool,
    pub animation_stops: Vec<AnimationStopControl>,
    pub binding: Option<SceneFieldBinding>,
}

#[derive(Clone)]
pub(super) struct AnimationStopControl {
    pub id: ControlId,
    pub edit: AnimationStopEdit,
}

#[derive(Clone)]
pub(super) struct NumberControl {
    pub common: LeafControl,
    pub spec: NumericInputSpec,
}

#[derive(Clone)]
pub(super) struct TextControl {
    pub common: LeafControl,
    pub multiline: bool,
}

#[derive(Clone)]
pub(super) struct ChoiceControl {
    pub common: LeafControl,
    pub options: Vec<(String, u32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ElementKind {
    Scalar,
    FontFamily,
}

#[derive(Clone)]
pub(super) struct ElementGroup {
    pub target: PropertyAddress,
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
    Plain(Vec<EditorControl>),
    Elements(Box<ElementGroup>),
    Effect(EffectGroup),
}

/// Controls declared by editor capabilities, independent of property values.
#[derive(Clone)]
pub(super) enum EditorControl {
    AspectRatioLock {
        address: PropertyAddress,
        locked: bool,
        read_only: bool,
    },
}

#[derive(Clone)]
pub(super) enum Control {
    File(LeafControl),
    Group {
        id: ControlId,
        label: String,
        children: Vec<Control>,
        kind: GroupKind,
    },
    Number(NumberControl),
    Text(TextControl),
    Bool(LeafControl),
    Choice(ChoiceControl),
    Color(LeafControl),
}

impl Control {
    pub(super) fn id(&self) -> &ControlId {
        match self {
            Self::File(file) => &file.id,
            Self::Group { id, .. } => id,
            Self::Number(control) => &control.common.id,
            Self::Text(control) => &control.common.id,
            Self::Bool(control) => &control.id,
            Self::Choice(control) => &control.common.id,
            Self::Color(control) => &control.id,
        }
    }

    pub(super) fn common(&self) -> Option<&LeafControl> {
        match self {
            Self::Number(control) => Some(&control.common),
            Self::Text(control) => Some(&control.common),
            Self::Bool(control) => Some(control),
            Self::Choice(control) => Some(&control.common),
            Self::Color(control) => Some(control),
            Self::File(control) => Some(control),
            Self::Group { .. } => None,
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct ControlTree {
    pub roots: Vec<Control>,
}

impl ControlTree {
    pub(super) fn animation_stop(&self, id: &ControlId) -> Option<&AnimationStopControl> {
        fn find<'a>(controls: &'a [Control], id: &ControlId) -> Option<&'a AnimationStopControl> {
            controls.iter().find_map(|control| match control {
                Control::Group { children, .. } => find(children, id),
                _ => control
                    .common()?
                    .animation_stops
                    .iter()
                    .find(|stop| &stop.id == id),
            })
        }
        find(&self.roots, id)
    }
}

pub(super) struct ControlResolution<'a> {
    pub editor: &'a TimelineEditor,
    pub item: &'a TimelineItem,
    pub playhead: TimelineTime,
    pub editing_scene: bool,
    pub arguments: &'a [SceneArgumentOption],
}

impl PropertyInspector {
    pub(super) fn format_value(value: impl Into<f64>) -> String {
        value.into().to_string()
    }

    pub(super) fn numeric_value_text(value: &PropertyValue) -> String {
        value.numeric_text().unwrap_or_default()
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

    fn leaf_control(
        property: &PropertySchema,
        value: PropertyValue,
        address: PropertyAddress,
        label: String,
        ty: &ScalarPropertyType,
        resolution: &ControlResolution<'_>,
    ) -> LeafControl {
        let scalar_index = address.scalar_index;
        let animation_enabled = resolution
            .item
            .animation_track(
                address.effect_id,
                &address.property_id,
                address.element_id,
                scalar_index,
            )
            .is_some();
        let stops = if animation_enabled {
            Self::animation_stop_controls(resolution, &address)
        } else {
            Some(Vec::new())
        };
        let scene_bindable = property.is_scene_bindable(scalar_index);
        let binding = Self::scene_field_binding(
            resolution.editing_scene,
            animation_enabled,
            scene_bindable,
            SceneBindingTarget::new(
                address.item_id,
                SceneBindingOwner::from_effect(address.effect_id),
                address.property_id.clone(),
                address.element_id,
                scalar_index,
            ),
            ty,
            resolution.arguments,
        );
        LeafControl {
            id: ControlId::Property(address.clone()),
            target: address,
            label,
            scalar_label: property.configuration_label(scalar_index),
            animatable: property.is_animatable(scalar_index),
            read_only: !property.is_editable(scalar_index) || stops.is_none(),
            value,
            animation_enabled,
            animation_stops: stops.unwrap_or_default(),
            binding,
        }
    }

    fn leaf_controls(
        property: &PropertySchema,
        value: &PropertyValue,
        effect_id: Option<EffectInstanceId>,
        element: Option<(usize, PropertyElementId)>,
        resolution: &ControlResolution<'_>,
    ) -> Vec<Control> {
        if !property.is_visible() {
            return Vec::new();
        }
        let label = element.map_or_else(
            || property.label().to_owned(),
            |(index, _)| format!("{} {}", property.label(), index + 1),
        );
        let tuple = matches!(property.value_schema(), ValueSchema::Tuple(_));
        property
            .value_schema()
            .scalars()
            .iter()
            .enumerate()
            .filter_map(|(index, scalar)| {
                let scalar_index = tuple.then_some(index);
                let scalar_type = &scalar.ty;
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
                let common = Self::leaf_control(
                    property,
                    value.clone(),
                    PropertyAddress {
                        item_id: resolution.item.id,
                        effect_id,
                        property_id: property.id().to_owned(),
                        element_id: element.map(|(_, id)| id),
                        scalar_index,
                    },
                    label,
                    scalar_type,
                    resolution,
                );
                let control = match (scalar_type, value) {
                    (
                        ScalarPropertyType::F32 | ScalarPropertyType::I32 | ScalarPropertyType::U32,
                        _value,
                    ) => Control::Number(NumberControl {
                        common,
                        spec: numeric_input_spec(property, scalar_index)?,
                    }),
                    (ScalarPropertyType::File, PropertyValue::File(_)) => Control::File(common),
                    (ScalarPropertyType::Color, PropertyValue::Color(_)) => Control::Color(common),
                    (ScalarPropertyType::Bool, PropertyValue::Bool(_)) => Control::Bool(common),
                    (ScalarPropertyType::String, PropertyValue::String(_)) => {
                        Control::Text(TextControl {
                            common,
                            multiline: scalar_ui.is_multiline(),
                        })
                    }
                    (ScalarPropertyType::Enum(enumeration), PropertyValue::Enum(_)) => {
                        Control::Choice(ChoiceControl {
                            common,
                            options: enumeration
                                .options()
                                .into_iter()
                                .map(|(value, label)| (label, value))
                                .collect(),
                        })
                    }
                    _ => return None,
                };
                Some(control)
            })
            .collect()
    }

    fn editor_controls(
        effect_id: Option<EffectInstanceId>,
        property: &PropertySchema,
        resolution: &ControlResolution<'_>,
    ) -> Vec<EditorControl> {
        let item = resolution.item;
        let Some(declared) = item
            .aspect_lock_property(effect_id)
            .filter(|declared| declared.id() == property.id())
        else {
            return Vec::new();
        };
        vec![EditorControl::AspectRatioLock {
            address: PropertyAddress {
                item_id: item.id,
                effect_id,
                property_id: property.id().to_owned(),
                element_id: None,
                scalar_index: None,
            },
            locked: item.aspect_ratio(effect_id).is_some(),
            read_only: !declared.is_editable(None),
        }]
    }

    fn property_control(
        effect_id: Option<EffectInstanceId>,
        property: &PropertySchema,
        resolution: &ControlResolution<'_>,
    ) -> Option<Control> {
        if let Some(group) = Self::elements_group(effect_id, property, resolution) {
            return Some(group);
        }
        let value = resolution
            .item
            .property_values(effect_id)?
            .property(property.id())?;
        let children = Self::leaf_controls(property, value, effect_id, None, resolution);
        if children.is_empty() {
            return None;
        }
        if matches!(property.value_schema(), ValueSchema::Tuple(_)) {
            Some(Control::Group {
                id: ControlId::Group(PropertyAddress {
                    item_id: resolution.item.id,
                    effect_id,
                    property_id: property.id().to_owned(),
                    element_id: None,
                    scalar_index: None,
                }),
                label: property.label().to_owned(),
                children,
                kind: GroupKind::Plain(Self::editor_controls(effect_id, property, resolution)),
            })
        } else {
            children.into_iter().next()
        }
    }

    fn elements_group(
        effect_id: Option<EffectInstanceId>,
        property: &PropertySchema,
        resolution: &ControlResolution<'_>,
    ) -> Option<Control> {
        if !property.is_visible() {
            return None;
        }
        let PropertyDefinition::Array {
            min_items,
            max_items,
            ..
        } = property.definition()
        else {
            return None;
        };
        let value = resolution
            .item
            .property_values(effect_id)?
            .property(property.id())?;
        let PropertyValue::Array(values) = value else {
            return None;
        };
        let element_kind = if property.configuration_ui(None).uses_font_family_editor() {
            ElementKind::FontFamily
        } else {
            ElementKind::Scalar
        };
        let target = PropertyAddress {
            item_id: resolution.item.id,
            property_id: property.id().to_owned(),
            effect_id,
            element_id: None,
            scalar_index: None,
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
                let row_controls = Self::leaf_controls(
                    property,
                    element.value(),
                    effect_id,
                    Some((element_index, element.element_id())),
                    resolution,
                );
                Control::Group {
                    id: ControlId::Group(PropertyAddress {
                        element_id: Some(element.element_id()),
                        ..target.clone()
                    }),
                    label: t!("rows.element", index = element_index + 1).to_string(),
                    children: row_controls,
                    kind: GroupKind::Plain(Vec::new()),
                }
            })
            .collect();
        Some(Control::Group {
            id: ControlId::Group(target.clone()),
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

    pub(super) fn property_controls(
        effect_id: Option<EffectInstanceId>,
        resolution: &ControlResolution<'_>,
    ) -> Vec<Control> {
        resolution
            .editor
            .property_schemas(resolution.item, effect_id)
            .filter_map(|property| Self::property_control(effect_id, property, resolution))
            .collect()
    }

    fn animation_stop_controls(
        resolution: &ControlResolution<'_>,
        target: &PropertyAddress,
    ) -> Option<Vec<AnimationStopControl>> {
        Some(
            resolution
                .editor
                .property_animation_stops(target, resolution.playhead)?
                .into_iter()
                .map(|edit| AnimationStopControl {
                    id: ControlId::AnimationStop {
                        property: target.clone(),
                        stop: edit.index(),
                    },
                    edit,
                })
                .collect(),
        )
    }

    pub(super) fn scene_field_binding(
        editing_scene: bool,
        animation_enabled: bool,
        scene_bindable: bool,
        target: SceneBindingTarget,
        ty: &ScalarPropertyType,
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
            .filter(|argument| {
                argument
                    .schema
                    .scalar_type(None, None)
                    .is_some_and(|actual| actual.same_type(ty))
            })
            .map(|argument| (argument.id.clone(), argument.label.clone()))
            .collect::<Vec<_>>();
        if connected.is_none() && compatible.is_empty() {
            return None;
        }
        Some(SceneFieldBinding {
            target,
            connected,
            compatible,
        })
    }
}
