use super::*;
use crate::ui::input::color_to_hsla;
use crate::ui::property_inspector::control::{AnimationStopControl, Control, ControlTree};
use crate::ui::{
    input::{InputStore, TextOptions},
    number_input::{NumberEdit, subscribe_number_input},
};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct InspectorSource {
    pub item_id: ItemId,
    pub properties: Vec<PropertySchema>,
}

#[derive(Default)]
pub(super) struct ControlStore {
    pub inputs: InputStore<ControlId>,
    pub tree: ControlTree,
    pub source: Option<InspectorSource>,
    pub number_drag: Option<PropertyValueDrag>,
    pub animation_value_edit: Option<ValueEdit>,
}

impl PropertyInspector {
    /// Get-or-create an input state for a control, syncing its displayed text.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn ensure_text(
        &mut self,
        key: ControlId,
        initial: String,
        multiline: bool,
        subscribe: impl FnOnce(
            &Entity<InputState>,
            &mut Window,
            &mut Context<Self>,
        ) -> Vec<Subscription>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        self.store.inputs.ensure_text(
            key,
            initial,
            TextOptions {
                multiline: multiline.then_some((2, 6)),
                ..Default::default()
            },
            subscribe,
            window,
            cx,
        )
    }

    fn ensure_number_text(
        &mut self,
        control: &crate::ui::property_inspector::control::NumberControl,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let spec = control.spec.clone();
        let target = control.common.target.clone();
        self.ensure_text(
            control.common.id(),
            text,
            false,
            |input, window, cx| {
                subscribe_number_input(
                    input,
                    spec.number,
                    window,
                    cx,
                    move |this, input, edit, window, cx| match edit {
                        NumberEdit::Value(value) => {
                            this.update_numeric_scalar(
                                &target,
                                value.clamp(spec.min, spec.max),
                                cx,
                            );
                        }
                        NumberEdit::Step(event) => {
                            this.apply_scalar_step(&target, &spec, input, event, window, cx)
                        }
                        NumberEdit::Commit => {}
                    },
                )
            },
            window,
            cx,
        );
        for stop in &control.common.animation_stops {
            self.ensure_number_stop(control, stop, window, cx);
        }
    }

    fn ensure_number_stop(
        &mut self,
        control: &crate::ui::property_inspector::control::NumberControl,
        stop: &AnimationStopControl,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if stop.plan.value.numeric_scalar().is_none() {
            return;
        }
        let spec = control.spec.clone();
        let id = stop.id.clone();
        self.ensure_text(
            stop.id.clone(),
            stop.plan.value.numeric_text().unwrap_or_default(),
            false,
            |input, window, cx| {
                subscribe_number_input(
                    input,
                    spec.number,
                    window,
                    cx,
                    move |this, input, edit, window, cx| match edit {
                        NumberEdit::Value(value) => {
                            this.update_animation_stop_numeric(
                                &id,
                                value.clamp(spec.min, spec.max),
                                cx,
                            );
                        }
                        NumberEdit::Step(event) => {
                            this.apply_animation_stop_step(&id, &spec, input, event, window, cx)
                        }
                        NumberEdit::Commit => this.finish_animation_value_edit(),
                    },
                )
            },
            window,
            cx,
        );
    }

    fn ensure_text_control(
        &mut self,
        control: &crate::ui::property_inspector::control::TextControl,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = control.common.target.clone();
        self.ensure_text(
            control.common.id(),
            text,
            control.multiline,
            |input, window, cx| {
                vec![
                    cx.subscribe_in(input, window, move |this, input, event, window, cx| {
                        this.apply_string_text(&target, input, event, window, cx);
                    }),
                ]
            },
            window,
            cx,
        );
    }

    fn ensure_color_control(
        &mut self,
        control: &crate::ui::property_inspector::control::ValueControl<[f32; 4]>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let common = &control.common;
        let color = color_to_hsla(control.value);
        let target = common.target.clone();
        self.store.inputs.ensure_color(
            common.id(),
            color,
            |picker, window, cx| {
                cx.subscribe_in(picker, window, move |this, _, event, _, cx| {
                    this.apply_color(&target, event, cx);
                })
            },
            window,
            cx,
        );
        for stop in &common.animation_stops {
            let PropertyValue::Color(color) = stop.plan.value else {
                continue;
            };
            let id = stop.id.clone();
            self.store.inputs.ensure_color(
                stop.id.clone(),
                color_to_hsla(color),
                |picker, window, cx| {
                    cx.subscribe_in(picker, window, move |this, _, event, _, cx| {
                        this.apply_animation_stop_color(&id, event, cx);
                    })
                },
                window,
                cx,
            );
        }
    }

    fn ensure_control_states(
        &mut self,
        control: &Control,
        seen: &mut HashSet<ControlId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let unique = seen.insert(control.id());
        debug_assert!(unique, "duplicate control id");
        if let Some(common) = control.common() {
            seen.extend(common.animation_stops.iter().map(|stop| stop.id.clone()));
        }
        for child in control.children() {
            self.ensure_control_states(child, seen, window, cx);
        }
        match control {
            Control::Number(number) => {
                let text = number.spec.number.format(number.value);
                self.ensure_number_text(number, text, window, cx);
            }
            Control::Text(text) => self.ensure_text_control(text, text.value.clone(), window, cx),
            Control::Color(color) => self.ensure_color_control(color, window, cx),
            Control::File(_)
            | Control::Bool(_)
            | Control::Choice(_)
            | Control::Tuple(_)
            | Control::Array(_)
            | Control::Effect(_) => {}
        }
    }

    fn ensure_tree_states(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut seen = HashSet::new();
        let roots = self.store.tree.roots.clone();
        for control in &roots {
            self.ensure_control_states(control, &mut seen, window, cx);
        }
        self.store.inputs.retain(&seen);
    }

    pub(super) fn reset_input_state(&mut self) {
        self.store = ControlStore::default();
    }

    pub(super) fn sync_from_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        {
            let editor = self.editor.read(cx);
            if self.item_id.is_none_or(|id| !editor.is_item_selected(id)) {
                self.item_id = editor.selected_item_ids().next();
                self.effect_picker = None;
                self.reset_input_state();
            }
        }
        let selected_item = {
            let editor = self.editor.read(cx);
            self.inspector_item_id(cx)
                .and_then(|id| editor.item(id))
                .map(|item| {
                    editor.evaluated_item_at(item, TimelineTime::from_frame(editor.playhead()))
                })
        };
        let source = selected_item.as_ref().map(|item| InspectorSource {
            item_id: item.source_id,
            properties: self
                .editor
                .read(cx)
                .evaluated_property_schemas(item, None)
                .cloned()
                .collect(),
        });
        if self.store.source != source {
            self.effect_picker = None;
            self.reset_input_state();
            self.store.source = source;
        }
        self.store.tree = selected_item
            .as_ref()
            .map(|item| {
                let editor = self.editor.read(cx);
                ControlTree::resolve(editor, item)
            })
            .unwrap_or_default();
        if selected_item.is_some() {
            self.ensure_tree_states(window, cx);
        } else {
            self.reset_input_state();
        }
        cx.notify();
    }
}
