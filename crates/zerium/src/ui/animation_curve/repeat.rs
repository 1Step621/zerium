//! Playback controls belong to the whole scalar track, not its intervals.
use super::*;
use crate::ui::{
    TimelineEditorEntityExt as _,
    input::{InputControl, set_input_text},
    number_input::{NumberEdit, NumberValueDrag, number_input_drag, subscribe_number_input},
    numeric_property::{NumericDrag, NumericInput},
};
use ::ui::input::{InputState, NumberInput, NumberInputEvent, StepAction};
use gpui::{Div, DragMoveEvent, MouseDownEvent, Stateful};
use rust_i18n::t;
use zerium_core::property::ScalarPropertyType;

#[derive(Clone, Copy)]
pub(super) enum RepeatParameter {
    Period,
    Phase,
}

impl RepeatParameter {
    const ALL: [Self; 2] = [Self::Period, Self::Phase];

    fn index(self) -> usize {
        self as usize
    }

    fn value(self, repeat: AnimationRepeat) -> f32 {
        match self {
            Self::Period => repeat.period(),
            Self::Phase => repeat.phase() * 100.,
        }
    }

    fn text(self, repeat: AnimationRepeat) -> String {
        match self {
            Self::Period => self.value(repeat).to_string(),
            Self::Phase => AnimationCurveEditor::format_number(self.value(repeat)),
        }
    }
}

fn repeat_mode_label(mode: RepeatMode) -> String {
    match mode {
        RepeatMode::None => t!("curve.repeat_none").to_string(),
        RepeatMode::Loop => t!("curve.repeat_loop").to_string(),
        RepeatMode::PingPong => t!("curve.repeat_ping_pong").to_string(),
    }
}

pub(super) struct RepeatInputs {
    address: PropertyAddress,
    fields: [InputControl; 2],
    // Last displayed model; unrelated redraws must preserve unfinished text.
    model: AnimationRepeat,
    pub(super) drag: Option<(RepeatParameter, NumericDrag)>,
}

impl AnimationCurveEditor {
    fn repeat_for(&self, address: &PropertyAddress, cx: &App) -> Option<AnimationRepeat> {
        if self.selection.read(cx).address() != Some(address) {
            return None;
        }
        let repeat = self
            .editor
            .read(cx)
            .item(address.item_id)?
            .animation_track(
                address.effect_id,
                &address.property_id,
                address.element_id,
                address.scalar_index,
            )?
            .repeat();
        (repeat.mode() != RepeatMode::None).then_some(repeat)
    }

    fn set_repeat_mode(&mut self, mode: RepeatMode, cx: &mut Context<Self>) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        self.end_pointer_drag(cx);
        self.finish_history_drag(cx);
        let repeat = selected.repeat.with_mode(mode);
        self.editor.update_if_changed(cx, |editor| {
            let changed = editor.set_animation_repeat(&selected.address, repeat);
            editor.finish_history_group();
            changed
        });
    }

    fn set_repeat_number(
        &mut self,
        address: &PropertyAddress,
        parameter: RepeatParameter,
        value: f32,
        cx: &mut Context<Self>,
    ) {
        if !value.is_finite() {
            return;
        }
        let Some(repeat) = self.repeat_for(address, cx) else {
            return;
        };
        let (period, phase) = match parameter {
            RepeatParameter::Period => (value.max(1.), repeat.phase()),
            RepeatParameter::Phase => (repeat.period(), value.clamp(0., 100.) / 100.),
        };
        let Some(repeat) = AnimationRepeat::new(repeat.mode(), period, phase) else {
            return;
        };
        self.editor
            .update_if_changed(cx, |editor| editor.set_animation_repeat(address, repeat));
    }

    fn ensure_repeat_inputs(
        &mut self,
        selected: &SelectedCurve,
        repeat: AnimationRepeat,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(inputs) = &mut self.repeat_inputs
            && inputs.address == selected.address
        {
            for parameter in RepeatParameter::ALL {
                if parameter.value(inputs.model) != parameter.value(repeat) {
                    set_input_text(
                        &inputs.fields[parameter.index()].input,
                        parameter.text(repeat),
                        window,
                        cx,
                    );
                }
            }
            inputs.model = repeat;
            return;
        }
        let fields = RepeatParameter::ALL.map(|parameter| {
            let input =
                cx.new(|cx| InputState::new(window, cx).default_value(parameter.text(repeat)));
            let address = selected.address.clone();
            let subscriptions = subscribe_number_input(
                &input,
                NumericInput {
                    scalar: ScalarPropertyType::F32,
                },
                window,
                cx,
                move |this, input, edit, window, cx| match edit {
                    NumberEdit::Value(value) => {
                        this.set_repeat_number(&address, parameter, value as f32, cx)
                    }
                    NumberEdit::Step(NumberInputEvent::Step { action, fine }) => {
                        let Some(repeat) = this.repeat_for(&address, cx) else {
                            return;
                        };
                        let step = if *fine { 0.1 } else { 1. };
                        let value = parameter.value(repeat)
                            + if *action == StepAction::Increment {
                                step
                            } else {
                                -step
                            };
                        this.set_repeat_number(&address, parameter, value, cx);
                    }
                    NumberEdit::Commit => {
                        this.finish_history_drag(cx);
                        if let Some(repeat) = this.repeat_for(&address, cx) {
                            set_input_text(input, parameter.text(repeat), window, cx);
                        }
                    }
                },
            );
            InputControl::new(input, subscriptions)
        });
        self.repeat_inputs = Some(RepeatInputs {
            address: selected.address.clone(),
            fields,
            model: repeat,
            drag: None,
        });
    }

    fn prepare_repeat_drag(
        &mut self,
        parameter: RepeatParameter,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        let Some(inputs) = &self.repeat_inputs else {
            return;
        };
        let Some(repeat) = self.repeat_for(&inputs.address, cx) else {
            return;
        };
        let start_value = f64::from(parameter.value(repeat));
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        self.repeat_inputs.as_mut().unwrap().drag = Some((
            parameter,
            NumericDrag {
                start_x: f32::from(event.position.x),
                start_value,
                step: 1.,
                sensitivity: 1.,
            },
        ));
    }

    pub(super) fn move_repeat_number(
        &mut self,
        event: &DragMoveEvent<NumberValueDrag>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let drag = event.drag(cx);
        let Some(inputs) = &self.repeat_inputs else {
            return;
        };
        let Some((parameter, adjustment)) = &inputs.drag else {
            return;
        };
        if drag.owner_id != cx.entity_id()
            || drag.input_id != inputs.fields[parameter.index()].input.entity_id()
        {
            return;
        }
        let address = inputs.address.clone();
        let parameter = *parameter;
        let value = adjustment.value_at(
            f32::from(event.event.position.x),
            event.event.modifiers.shift,
        ) as f32;
        self.focus_handle.focus(window, cx);
        cx.set_active_drag_cursor_style(gpui::CursorStyle::ResizeLeftRight, window);
        self.set_repeat_number(&address, parameter, value, cx);
    }

    pub(super) fn repeat_controls(
        &mut self,
        selected: &SelectedCurve,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let mode = selected.repeat.mode();
        let editor = cx.entity();
        let selector = Button::new("animation-repeat")
            .small()
            .compact()
            .outline()
            .label(repeat_mode_label(mode))
            .tooltip(t!("curve.repeat_hint").to_string())
            .dropdown_caret(true)
            .dropdown_menu_with_anchor(Corner::TopRight, move |menu, _, _| {
                [RepeatMode::None, RepeatMode::Loop, RepeatMode::PingPong]
                    .into_iter()
                    .fold(menu, |menu, option| {
                        let editor = editor.clone();
                        menu.item(
                            PopupMenuItem::new(repeat_mode_label(option))
                                .checked(mode == option)
                                .on_click(move |_, _, cx| {
                                    editor.update(cx, |editor, cx| {
                                        editor.set_repeat_mode(option, cx)
                                    });
                                }),
                        )
                    })
            });
        let mut controls = div()
            .id("animation-repeat-controls")
            .when(mode != RepeatMode::None, |this| this.w(px(440.)))
            .max_w_full()
            .min_w_0()
            .flex()
            .items_center()
            .flex_shrink()
            .overflow_x_scroll()
            .gap_2()
            .text_xs()
            .text_color(cx.theme().colors.muted_foreground)
            .whitespace_nowrap()
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .child(t!("curve.repeat").to_string())
                    .child(selector),
            );
        if selected.repeat.mode() != RepeatMode::None {
            let repeat = selected.repeat;
            self.ensure_repeat_inputs(selected, repeat, window, cx);
            for parameter in RepeatParameter::ALL {
                let input = self.repeat_inputs.as_ref().unwrap().fields[parameter.index()]
                    .input
                    .clone();
                let (label, suffix) = match parameter {
                    RepeatParameter::Period => (t!("curve.period"), "F"),
                    RepeatParameter::Phase => (t!("curve.phase"), "%"),
                };
                let control = NumberInput::new(&input)
                    .small()
                    .min_w_0()
                    .suffix(div().child(suffix));
                controls = controls.child(
                    div()
                        .flex()
                        .items_center()
                        .flex_1()
                        .min_w(px(105.))
                        .gap_1()
                        .child(label.to_string())
                        .child(number_input_drag(
                            &cx.entity(),
                            &input,
                            control,
                            false,
                            move |this, event, cx| this.prepare_repeat_drag(parameter, event, cx),
                        )),
                );
            }
        } else {
            self.repeat_inputs = None;
        }
        controls
    }
}
