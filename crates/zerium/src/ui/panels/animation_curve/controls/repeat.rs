//! Playback controls belong to the whole scalar track, not its intervals.
use crate::ui::{
    TimelineEditorEntityExt as _,
    components::input::{
        InputControl,
        number::{
            NumberDragStart, NumberEdit, number_input_drag, subscribe_number_input,
            value::{NumericDrag, NumericInput},
        },
        set_input_text,
    },
    panels::animation_curve::{format_number, *},
};
use ::ui::input::{InputState, NumberInput, NumberInputEvent, StepAction};
use gpui::{Div, Stateful};
use rust_i18n::t;

#[derive(Clone, Copy)]
pub(in crate::ui::panels::animation_curve) enum RepeatParameter {
    Period,
    Phase,
}

impl RepeatParameter {
    const ALL: [Self; 2] = [Self::Period, Self::Phase];

    pub(in crate::ui::panels::animation_curve) fn index(self) -> usize {
        self as usize
    }

    fn value(self, repeat: AnimationRepeat) -> f64 {
        match self {
            Self::Period => repeat.period().get() as f64,
            Self::Phase => repeat.phase().get() as f64 / repeat.period().get() as f64 * 100.,
        }
    }

    fn text(self, repeat: AnimationRepeat) -> String {
        match self {
            Self::Period => self.value(repeat).to_string(),
            Self::Phase => format_number(self.value(repeat)),
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

pub(in crate::ui::panels::animation_curve) struct RepeatInputs {
    pub(in crate::ui::panels::animation_curve) address: PropertyAddress,
    pub(in crate::ui::panels::animation_curve) fields: [InputControl; 2],
    // Last displayed model; unrelated redraws must preserve unfinished text.
    model: AnimationRepeat,
}

impl AnimationCurveEditor {
    fn repeat_for(&self, address: &PropertyAddress, cx: &App) -> Option<AnimationRepeat> {
        if self.selection.read(cx).address() != Some(address) {
            return None;
        }
        let repeat = self.editor.read(cx).animation_track(address)?.repeat();
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

    pub(in crate::ui::panels::animation_curve) fn set_repeat_number(
        &mut self,
        address: &PropertyAddress,
        parameter: RepeatParameter,
        value: f64,
        cx: &mut Context<Self>,
    ) {
        if !value.is_finite() {
            return;
        }
        let Some(repeat) = self.repeat_for(address, cx) else {
            return;
        };
        let phase_ratio = repeat.phase().get() as f64 / repeat.period().get() as f64;
        let (period, phase_ratio) = match parameter {
            RepeatParameter::Period => (
                FrameDuration::new(value.round().max(1.) as u64).unwrap(),
                phase_ratio,
            ),
            RepeatParameter::Phase => (repeat.period(), value.clamp(0., 100.) / 100.),
        };
        let normalized = AnimationRepeat::new(repeat.mode(), period, Frame::new(0));
        let phase = Frame::new((phase_ratio * normalized.period().get() as f64).round() as u64);
        let repeat = AnimationRepeat::new(repeat.mode(), normalized.period(), phase);
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
                NumericInput::F32,
                window,
                cx,
                move |this, input, edit, window, cx| match edit {
                    NumberEdit::Value(value) => {
                        this.set_repeat_number(&address, parameter, value, cx)
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
                        if this.number_drag.is_none() {
                            this.finish_history_drag(cx);
                        }
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
        });
    }

    fn prepare_repeat_drag(
        &mut self,
        parameter: RepeatParameter,
        event: &NumberDragStart,
        cx: &mut Context<Self>,
    ) {
        let Some(inputs) = &self.repeat_inputs else {
            return;
        };
        let Some(repeat) = self.repeat_for(&inputs.address, cx) else {
            return;
        };
        let start_value = parameter.value(repeat);
        let input = inputs.fields[parameter.index()].input.clone();
        self.end_pointer_drag(cx);
        self.finish_history_drag(cx);
        self.number_drag = Some(CurveNumberDrag {
            parameter: CurveNumberParameter::Repeat(parameter),
            input,
            motion: NumericDrag {
                start_x: f32::from(event.position.x),
                start_value,
                step: 1.,
                sensitivity: 1.,
            },
        });
    }

    pub(in crate::ui::panels::animation_curve) fn repeat_controls(
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
