use super::*;

#[derive(Clone)]
pub(super) struct ControlMeta {
    pub id: ControlId,
    pub target: PropertyAddress,
    pub label: String,
    pub scalar_label: Option<String>,
    pub animatable: bool,
    pub read_only: bool,
    pub animation_enabled: bool,
    pub animation_stops: Vec<AnimationStopControl>,
    pub binding: Option<SceneFieldBinding>,
}

#[derive(Clone)]
pub(super) struct AnimationStopControl {
    pub id: ControlId,
    pub plan: AnimationValuePlan,
}

#[derive(Clone)]
pub(super) struct ValueControl<T> {
    pub common: ControlMeta,
    pub value: T,
}

#[derive(Clone)]
pub(super) struct NumberControl {
    pub common: ControlMeta,
    pub value: f64,
    pub spec: NumericInputSpec,
    pub applied_value: Option<String>,
}

#[derive(Clone)]
pub(super) struct TextControl {
    pub common: ControlMeta,
    pub value: String,
    pub multiline: bool,
}

#[derive(Clone)]
pub(super) struct ChoiceControl {
    pub common: ControlMeta,
    pub value: u32,
    pub options: Vec<(String, u32)>,
}

#[derive(Clone)]
pub(super) struct TupleControl {
    pub target: PropertyAddress,
    pub label: String,
    pub children: Vec<Control>,
    pub extensions: Vec<EditorControl>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ElementKind {
    Scalar,
    FontFamily,
}

#[derive(Clone)]
pub(super) struct ArrayRow {
    pub element_id: PropertyElementId,
    pub controls: Vec<Control>,
}

impl ArrayRow {
    pub(super) fn font(&self) -> Option<&str> {
        self.controls.iter().find_map(|control| match control {
            Control::Text(text) => Some(text.value.as_str()),
            _ => None,
        })
    }
}

#[derive(Clone)]
pub(super) struct ArrayControl {
    pub target: PropertyAddress,
    pub property: Box<PropertySchema>,
    pub rows: Vec<ArrayRow>,
    pub element_kind: ElementKind,
    pub min_items: u32,
    pub max_items: u32,
    pub has_scene_binding: bool,
}

#[derive(Clone)]
pub(super) struct EffectControl {
    pub id: EffectInstanceId,
    pub label: String,
    pub hidden: bool,
    pub children: Vec<Control>,
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
    File(ValueControl<Option<std::path::PathBuf>>),
    Number(NumberControl),
    Text(TextControl),
    Bool(ValueControl<bool>),
    Choice(ChoiceControl),
    Color(ValueControl<[f32; 4]>),
    Tuple(TupleControl),
    Array(Box<ArrayControl>),
    Effect(EffectControl),
}

impl Control {
    pub(super) fn id(&self) -> ControlId {
        match self {
            Self::Tuple(tuple) => ControlId::Group(tuple.target.clone()),
            Self::Array(array) => ControlId::Group(array.target.clone()),
            Self::Effect(effect) => ControlId::EffectGroup(effect.id),
            Self::File(control) => control.common.id.clone(),
            Self::Number(control) => control.common.id.clone(),
            Self::Text(control) => control.common.id.clone(),
            Self::Bool(control) => control.common.id.clone(),
            Self::Choice(control) => control.common.id.clone(),
            Self::Color(control) => control.common.id.clone(),
        }
    }

    pub(super) fn common(&self) -> Option<&ControlMeta> {
        match self {
            Self::Number(control) => Some(&control.common),
            Self::Text(control) => Some(&control.common),
            Self::Bool(control) => Some(&control.common),
            Self::Choice(control) => Some(&control.common),
            Self::Color(control) => Some(&control.common),
            Self::File(control) => Some(&control.common),
            Self::Tuple(_) | Self::Array(_) | Self::Effect(_) => None,
        }
    }

    pub(super) fn children(&self) -> impl Iterator<Item = &Control> {
        let (children, rows): (&[Control], &[ArrayRow]) = match self {
            Self::Tuple(tuple) => (&tuple.children, &[]),
            Self::Array(array) => (&[], &array.rows),
            Self::Effect(effect) => (&effect.children, &[]),
            _ => (&[], &[]),
        };
        children
            .iter()
            .chain(rows.iter().flat_map(|row| &row.controls))
    }
}

#[derive(Clone, Default)]
pub(super) struct ControlTree {
    pub roots: Vec<Control>,
}

impl ControlTree {
    pub(super) fn animation_stop(&self, id: &ControlId) -> Option<&AnimationStopControl> {
        fn find<'a>(control: &'a Control, id: &ControlId) -> Option<&'a AnimationStopControl> {
            control
                .common()
                .and_then(|common| common.animation_stops.iter().find(|stop| &stop.id == id))
                .or_else(|| control.children().find_map(|child| find(child, id)))
        }
        self.roots.iter().find_map(|control| find(control, id))
    }
}

struct ControlResolution<'a> {
    editor: &'a TimelineEditor,
    item: &'a TimelineItem,
    playhead: TimelineTime,
    editing_scene: bool,
    variables: &'a [SceneVariable],
}

impl PropertyInspector {
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
}

impl ControlTree {
    pub(super) fn resolve(editor: &TimelineEditor, item: &TimelineItem) -> Self {
        let scene_arguments = editor
            .active_scene_id()
            .and_then(|id| editor.scene(id))
            .map(|scene| scene.variables())
            .unwrap_or(&[]);
        let resolution = ControlResolution {
            editor,
            item,
            playhead: zerium_core::timeline::TimelineTime::from_frame(editor.playhead()),
            editing_scene: editor.active_scene_id().is_some(),
            variables: scene_arguments,
        };
        let item_controls = Self::property_controls(None, &resolution);
        let effect_groups: Vec<Control> = item
            .effects
            .iter()
            .map(|effect| {
                let controls = Self::property_controls(Some(effect.id), &resolution);
                Control::Effect(EffectControl {
                    id: effect.id,
                    label: effect.schema().label().to_owned(),
                    hidden: editor.is_effect_hidden(effect.id),
                    children: controls,
                })
            })
            .collect();

        let roots = item_controls.into_iter().chain(effect_groups).collect();
        Self { roots }
    }

    fn leaf_control(
        property: &PropertySchema,
        address: PropertyAddress,
        label: String,
        ty: &ScalarPropertyType,
        resolution: &ControlResolution<'_>,
    ) -> ControlMeta {
        let scalar_index = address.scalar_index;
        let animation_enabled = resolution.editor.animation_track(&address).is_some();
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
            resolution.variables,
        );
        ControlMeta {
            id: ControlId::Property(address.clone()),
            target: address,
            label,
            scalar_label: property.configuration_label(scalar_index),
            animatable: property.is_animatable(scalar_index),
            read_only: !property.is_editable(scalar_index) || stops.is_none(),
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
        let tuple = property.value_schema().tuple().is_some();
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
                        value,
                    ) => {
                        let applied_value = (|| {
                            if effect_id.is_some() {
                                return None;
                            }
                            let scene = resolution.editor.scene(resolution.item.scene_id()?)?;
                            let argument = scene.argument(property.id())?;
                            let authored = resolution
                                .editor
                                .evaluated_property_value(&common.target, resolution.playhead)?;
                            let applied = argument.apply_argument_limits(&authored)?;
                            (applied != authored)
                                .then(|| applied.numeric_text())
                                .flatten()
                        })();
                        Control::Number(NumberControl {
                            common,
                            value: value.numeric_scalar()?,
                            spec: numeric_input_spec(property, scalar_index)?,
                            applied_value,
                        })
                    }
                    (ScalarPropertyType::File, PropertyValue::File(value)) => {
                        Control::File(ValueControl { common, value })
                    }
                    (ScalarPropertyType::Color, PropertyValue::Color(value)) => {
                        Control::Color(ValueControl { common, value })
                    }
                    (ScalarPropertyType::Bool, PropertyValue::Bool(value)) => {
                        Control::Bool(ValueControl { common, value })
                    }
                    (ScalarPropertyType::String, PropertyValue::String(value)) => {
                        Control::Text(TextControl {
                            common,
                            value,
                            multiline: scalar_ui.is_multiline(),
                        })
                    }
                    (ScalarPropertyType::Enum(enumeration), PropertyValue::Enum(value)) => {
                        Control::Choice(ChoiceControl {
                            common,
                            value,
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
        if let ValueSchema::Tuple(_) = property.value_schema() {
            Some(Control::Tuple(TupleControl {
                target: PropertyAddress {
                    item_id: resolution.item.id,
                    effect_id,
                    property_id: property.id().to_owned(),
                    element_id: None,
                    scalar_index: None,
                },
                label: property.label().to_owned(),
                children,
                extensions: Self::editor_controls(effect_id, property, resolution),
            }))
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
        let has_scene_binding = resolution.variables.iter().any(|argument| {
            argument.bindings.iter().any(|binding| {
                binding.item_id() == resolution.item.id
                    && binding.owner() == SceneBindingOwner::from_effect(target.effect_id)
                    && binding.property_id() == target.property_id
                    && binding.element_id().is_some()
            })
        });
        let rows = values
            .iter()
            .enumerate()
            .map(|(index, element)| ArrayRow {
                element_id: element.element_id(),
                controls: Self::leaf_controls(
                    property,
                    element.value(),
                    effect_id,
                    Some((index, element.element_id())),
                    resolution,
                ),
            })
            .collect();
        Some(Control::Array(Box::new(ArrayControl {
            target,
            property: Box::new(property.clone()),
            rows,
            element_kind,
            min_items: *min_items,
            max_items: *max_items,
            has_scene_binding,
        })))
    }

    fn property_controls(
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
                .map(|plan| AnimationStopControl {
                    id: ControlId::AnimationStop {
                        property: target.clone(),
                        stop: plan.index(),
                    },
                    plan,
                })
                .collect(),
        )
    }

    fn scene_field_binding(
        editing_scene: bool,
        animation_enabled: bool,
        scene_bindable: bool,
        target: SceneBindingTarget,
        ty: &ScalarPropertyType,
        arguments: &[SceneVariable],
    ) -> Option<SceneFieldBinding> {
        if !editing_scene || animation_enabled || !scene_bindable {
            return None;
        }
        let connected = arguments.iter().find_map(|argument| {
            argument
                .bindings
                .contains(&target)
                .then(|| (argument.id().to_owned(), argument.label().to_owned()))
        });
        let compatible = arguments
            .iter()
            .filter(|argument| argument.ty().same_type(ty))
            .map(|argument| (argument.id().to_owned(), argument.label().to_owned()))
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
