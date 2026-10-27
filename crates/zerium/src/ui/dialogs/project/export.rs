use ::ui::{
    Selectable as _, Sizable as _,
    button::Button,
    menu::{PopupMenuItem, popup_menu::PopupMenuExt as _},
    switch::Switch,
};
use gpui::{AnyElement, App, Context, Entity, Render, Window, div, prelude::*};
use rust_i18n::t;
use zerium_core::timeline::{
    AudioCodec, ExportBitDepth, ExportColorRange, ExportColorSpace, ExportContainer, ExportSettings,
};

use crate::ui::components::{
    form::{form_fields, form_row},
    input::number::{settings::SettingsNumberInput, value::NumericInput},
};

/// A dialog draft. The project is updated only when the user confirms it.
pub(super) struct ExportSettingsForm {
    draft: ExportSettings,
    controls: [Entity<SettingsNumberInput>; 2],
}

impl ExportSettingsForm {
    pub(super) fn new(
        settings: ExportSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let controls = [
            settings.video_bitrate_kbps.unwrap_or(10_000).to_string(),
            settings.audio_bitrate_kbps.to_string(),
        ]
        .map(|value| {
            cx.new(|cx| SettingsNumberInput::new(value, NumericInput::U32, 1., "kbps", window, cx))
        });
        Self {
            draft: settings,
            controls,
        }
    }

    pub(super) fn settings(&self, cx: &App) -> Result<ExportSettings, String> {
        let mut settings = self.draft.clone();
        let number = |index: usize| {
            self.controls[index]
                .read(cx)
                .value(cx)
                .trim()
                .parse::<u32>()
                .map_err(|_| t!("export.invalid_number").to_string())
        };
        if settings.container != ExportContainer::Gif && settings.video_bitrate_kbps.is_some() {
            settings.video_bitrate_kbps = Some(number(0)?);
        }
        if settings.exports_audio() && settings.audio_codec != AudioCodec::Flac {
            settings.audio_bitrate_kbps = number(1)?;
        }
        settings.validate().map_err(|error| error.to_string())?;
        Ok(settings)
    }

    fn choice_menu<T: Copy + PartialEq + ToString + 'static>(
        &self,
        id: &'static str,
        selected: T,
        choices: &'static [T],
        update: fn(&mut ExportSettings, T),
        cx: &Context<Self>,
    ) -> AnyElement {
        let settings = cx.entity();
        Button::new(id)
            .small()
            .outline()
            .label(selected.to_string())
            .dropdown_caret(true)
            .popup_menu(move |menu, _, _| {
                choices.iter().fold(menu, |menu, &choice| {
                    let settings = settings.clone();
                    menu.item(
                        PopupMenuItem::new(choice.to_string())
                            .checked(choice == selected)
                            .on_click(move |_, _, cx| {
                                settings.update(cx, |this, cx| {
                                    update(&mut this.draft, choice);
                                    cx.notify();
                                })
                            }),
                    )
                })
            })
            .into_any_element()
    }
}

impl Render for ExportSettingsForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let color = self.draft.color;
        let container = self.draft.container;
        let video_codec = self.draft.video_codec;
        let audio_codec = self.draft.audio_codec;
        form_fields()
            .child(form_row(
                t!("export.format").to_string(),
                self.choice_menu(
                    "export-container",
                    container,
                    &ExportContainer::ALL,
                    ExportSettings::set_container,
                    cx,
                ),
            ))
            .when(container == ExportContainer::Gif, |form| {
                form.child(form_row(
                    t!("export.loop").to_string(),
                    Switch::new("export-gif-loop")
                        .small()
                        .checked(self.draft.gif_loop)
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.draft.gif_loop = *checked;
                            cx.notify();
                        })),
                ))
            })
            .when(container != ExportContainer::Gif, |form| {
                form.child(form_row(
                    t!("export.video_codec").to_string(),
                    self.choice_menu(
                        "export-video-codec",
                        video_codec,
                        container.video_codecs(),
                        |settings, codec| settings.video_codec = codec,
                        cx,
                    ),
                ))
                .child(form_row(
                    t!("export.color_space").to_string(),
                    self.choice_menu(
                        "export-color-space",
                        color.space,
                        &ExportColorSpace::ALL,
                        |settings, space| settings.color.set_space(space),
                        cx,
                    ),
                ))
                .child(form_row(
                    t!("export.bit_depth").to_string(),
                    div().flex().gap_1().children(
                        [ExportBitDepth::Eight, ExportBitDepth::Ten]
                            .into_iter()
                            .enumerate()
                            .map(|(index, depth)| {
                                Button::new(("export-bit-depth", index))
                                    .small()
                                    .outline()
                                    .label(format!("{} bit", depth.bits()))
                                    .selected(color.depth == depth)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.draft.color.set_depth(depth);
                                        cx.notify();
                                    }))
                            }),
                    ),
                ))
                .child(form_row(
                    t!("export.color_range").to_string(),
                    div().flex().gap_1().children(
                        [
                            (
                                ExportColorRange::Limited,
                                t!("export.range_limited").to_string(),
                            ),
                            (ExportColorRange::Full, t!("export.range_full").to_string()),
                        ]
                        .into_iter()
                        .enumerate()
                        .map(|(index, (range, label))| {
                            Button::new(("export-color-range", index))
                                .small()
                                .outline()
                                .label(label)
                                .selected(color.range == range)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.draft.color.range = range;
                                    cx.notify();
                                }))
                        }),
                    ),
                ))
                .child(form_row(
                    t!("export.video_bitrate").to_string(),
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            Switch::new("export-custom-bitrate")
                                .small()
                                .checked(self.draft.video_bitrate_kbps.is_some())
                                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                    this.draft.video_bitrate_kbps = checked.then_some(10_000);
                                    cx.notify();
                                })),
                        )
                        .when(self.draft.video_bitrate_kbps.is_some(), |row| {
                            row.child(self.controls[0].clone())
                        })
                        .when(self.draft.video_bitrate_kbps.is_none(), |row| {
                            row.child(t!("export.automatic").to_string())
                        }),
                ))
                .child(form_row(
                    t!("export.audio").to_string(),
                    Switch::new("export-audio")
                        .small()
                        .checked(self.draft.audio_enabled)
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.draft.audio_enabled = *checked;
                            cx.notify();
                        })),
                ))
            })
            .when(self.draft.exports_audio(), |form| {
                form.child(form_row(
                    t!("export.audio_codec").to_string(),
                    self.choice_menu(
                        "export-audio-codec",
                        audio_codec,
                        container.audio_codecs(),
                        ExportSettings::set_audio_codec,
                        cx,
                    ),
                ))
                .child(form_row(
                    t!("export.sample_rate").to_string(),
                    div()
                        .flex()
                        .gap_1()
                        .children(audio_codec.sample_rates().iter().copied().map(|rate| {
                            Button::new(("export-sample-rate", rate as usize))
                                .small()
                                .outline()
                                .label(format!("{} kHz", rate as f32 / 1000.))
                                .selected(self.draft.audio_sample_rate == rate)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.draft.audio_sample_rate = rate;
                                    cx.notify();
                                }))
                        })),
                ))
                .child(form_row(
                    t!("export.channels").to_string(),
                    div().flex().gap_1().children(
                        [
                            (1, t!("export.mono").to_string()),
                            (2, t!("export.stereo").to_string()),
                        ]
                        .into_iter()
                        .map(|(channels, label)| {
                            Button::new(("export-channels", channels as usize))
                                .small()
                                .outline()
                                .label(label)
                                .selected(self.draft.audio_channels == channels)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.draft.audio_channels = channels;
                                    cx.notify();
                                }))
                        }),
                    ),
                ))
                .when(audio_codec != AudioCodec::Flac, |form| {
                    form.child(form_row(
                        t!("export.audio_bitrate").to_string(),
                        self.controls[1].clone(),
                    ))
                })
            })
    }
}
