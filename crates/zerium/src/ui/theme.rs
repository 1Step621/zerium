use ::ui::{Theme, ThemeMode};
use gpui::{App, Hsla, rgb};
use std::borrow::Cow;

use crate::app::storage::config::{AccentColor, ConfigStore};

pub(crate) const FONT_FAMILY: &str = "Inter";

const INTER_FONTS: [&[u8]; 8] = [
    include_bytes!("../../../../assets/inter/Inter-Regular.otf"),
    include_bytes!("../../../../assets/inter/Inter-Italic.otf"),
    include_bytes!("../../../../assets/inter/Inter-Medium.otf"),
    include_bytes!("../../../../assets/inter/Inter-MediumItalic.otf"),
    include_bytes!("../../../../assets/inter/Inter-SemiBold.otf"),
    include_bytes!("../../../../assets/inter/Inter-SemiBoldItalic.otf"),
    include_bytes!("../../../../assets/inter/Inter-Bold.otf"),
    include_bytes!("../../../../assets/inter/Inter-BoldItalic.otf"),
];

struct BasePalette {
    background: u32,
    surface: u32,
    raised: u32,
    hover: u32,
    border: u32,
    foreground: u32,
    muted_foreground: u32,
    popover: u32,
    scrollbar_hover: u32,
    warning: [u32; 3],
    warning_foreground: u32,
}

impl BasePalette {
    fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self {
                background: 0x282c34,
                surface: 0x2c313a,
                raised: 0x343a45,
                hover: 0x3e4451,
                border: 0x4b5263,
                foreground: 0xabb2bf,
                muted_foreground: 0x7f848e,
                popover: 0x303640,
                scrollbar_hover: 0x9da5b4,
                warning: [0xf97316, 0xfb923c, 0xea580c],
                warning_foreground: 0x21160e,
            },
            ThemeMode::Light => Self {
                background: 0xf5f6f8,
                surface: 0xffffff,
                raised: 0xedeff2,
                hover: 0xe2e5ea,
                border: 0xc4c9d1,
                foreground: 0x282c34,
                muted_foreground: 0x5f6877,
                popover: 0xffffff,
                scrollbar_hover: 0x7c8593,
                warning: [0xc2410c, 0x9a3412, 0x7c2d12],
                warning_foreground: 0xffffff,
            },
        }
    }
}

fn color(hex: u32) -> Hsla {
    rgb(hex).into()
}

struct AccentPalette {
    primary: [Hsla; 3],
    foreground: Hsla,
    text: Hsla,
    link: [Hsla; 3],
}

impl AccentPalette {
    fn for_theme(mode: ThemeMode, accent: AccentColor) -> Self {
        use AccentColor::*;
        use ThemeMode::*;

        let (primary, foreground, text, link) = match (mode, accent) {
            (Dark, Orange) => (
                [0xf97316, 0xfb923c, 0xea580c],
                0x21160e,
                0xfb923c,
                [0xf97316, 0xfb923c, 0xea580c],
            ),
            (Dark, Teal) => (
                [0x2dd4bf, 0x5eead4, 0x14b8a6],
                0x21160e,
                0x5eead4,
                [0x2dd4bf, 0x5eead4, 0x14b8a6],
            ),
            (Dark, Red) => (
                [0xc24353, 0xc74859, 0xaa3445],
                0xffffff,
                0xe995a3,
                [0xe995a3, 0xf2b7c0, 0xdf7d8f],
            ),
            (Dark, Pink) => (
                [0xf2a6c4, 0xf7bfd5, 0xe88fb3],
                0x21160e,
                0xf7bfd5,
                [0xf2a6c4, 0xf7bfd5, 0xe88fb3],
            ),
            (Dark, Purple) => (
                [0xa78bfa, 0xc4b5fd, 0x9868f5],
                0x21160e,
                0xc4b5fd,
                [0xa78bfa, 0xc4b5fd, 0x9868f5],
            ),
            (Dark, Yellow) => (
                [0xfacc15, 0xfde047, 0xeab308],
                0x21160e,
                0xfde047,
                [0xfacc15, 0xfde047, 0xeab308],
            ),
            (Light, Orange) => (
                [0xc2410c, 0x9a3412, 0x7c2d12],
                0xffffff,
                0x7c2d12,
                [0xc2410c, 0x9a3412, 0x7c2d12],
            ),
            (Light, Teal) => (
                [0x0f766e, 0x115e59, 0x134e4a],
                0xffffff,
                0x134e4a,
                [0x0f766e, 0x115e59, 0x134e4a],
            ),
            (Light, Red) => (
                [0xc24353, 0xb03a4b, 0x963042],
                0xffffff,
                0x963042,
                [0xc24353, 0xb03a4b, 0x963042],
            ),
            (Light, Pink) => (
                [0xf2a6c4, 0xf7bfd5, 0xe88fb3],
                0x21160e,
                0x9c3f65,
                [0x9c3f65, 0x842e51, 0x6e2443],
            ),
            (Light, Purple) => (
                [0x6d28d9, 0x5b21b6, 0x4c1d95],
                0xffffff,
                0x4c1d95,
                [0x6d28d9, 0x5b21b6, 0x4c1d95],
            ),
            (Light, Yellow) => (
                [0xfacc15, 0xfde047, 0xeab308],
                0x21160e,
                0x854d0e,
                [0x854d0e, 0x713f12, 0x713f12],
            ),
        };
        Self {
            primary: primary.map(color),
            foreground: color(foreground),
            text: color(text),
            link: link.map(color),
        }
    }
}

pub(crate) fn install(cx: &mut App) {
    cx.text_system()
        .add_fonts(INTER_FONTS.into_iter().map(Cow::Borrowed).collect())
        .expect("bundled Inter fonts must be valid");
    apply(cx);
}

pub(super) fn apply(cx: &mut App) {
    let config = ConfigStore::read(cx).cloned().unwrap_or_default();
    let mode = config.theme_mode.unwrap_or(ThemeMode::Dark);
    let accent = config.accent_color.unwrap_or_default();
    Theme::change(mode, None, cx);
    let theme = Theme::global_mut(cx);
    theme.font_family = FONT_FAMILY.into();
    let colors = &mut theme.colors;
    let base = BasePalette::for_mode(mode);
    let accent = AccentPalette::for_theme(mode, accent);
    let [primary, primary_hover, primary_active] = accent.primary;
    let soft = color(base.background).blend(primary.opacity(0.1));

    colors.background = color(base.background);
    colors.foreground = color(base.foreground);
    colors.border = color(base.border);
    colors.input = color(base.hover);
    colors.ring = primary;
    colors.caret = primary;
    colors.drag_border = primary;
    colors.drop_target = primary.opacity(0.25);
    colors.selection = primary.opacity(0.3);

    colors.primary = primary;
    colors.primary_hover = primary_hover;
    colors.primary_active = primary_active;
    colors.primary_foreground = accent.foreground;
    colors.progress_bar = primary;
    colors.accent = soft;
    colors.accent_foreground = accent.text;
    colors.secondary = color(base.raised);
    colors.secondary_hover = color(base.hover);
    colors.secondary_active = soft;
    colors.secondary_foreground = color(base.foreground);
    colors.muted = color(base.raised);
    colors.muted_foreground = color(base.muted_foreground);

    colors.accordion = color(base.surface);
    colors.accordion_hover = color(base.hover);
    colors.group_box = color(base.surface);
    colors.group_box_foreground = color(base.foreground);
    colors.popover = color(base.popover);
    colors.popover_foreground = color(base.foreground);

    colors.list = color(base.background);
    colors.list_head = color(base.surface);
    colors.list_even = color(base.surface);
    colors.list_hover = color(base.hover);
    colors.list_active = primary.opacity(0.18);
    colors.list_active_border = primary;
    colors.table = color(base.background);
    colors.table_head = color(base.surface);
    colors.table_head_foreground = color(base.muted_foreground);
    colors.table_even = color(base.surface);
    colors.table_hover = color(base.hover);
    colors.table_active = primary.opacity(0.18);
    colors.table_active_border = primary;
    colors.table_row_border = color(base.hover);

    colors.sidebar = color(base.surface);
    colors.sidebar_foreground = color(base.foreground);
    colors.sidebar_border = color(base.hover);
    colors.sidebar_accent = soft;
    colors.sidebar_accent_foreground = accent.text;
    colors.sidebar_primary = primary;
    colors.sidebar_primary_foreground = accent.foreground;
    colors.tab = color(base.background);
    colors.tab_bar = color(base.surface);
    colors.tab_bar_segmented = color(base.raised);
    colors.tab_foreground = color(base.muted_foreground);
    colors.tab_active = soft;
    colors.tab_active_foreground = accent.text;

    colors.title_bar = color(base.surface);
    colors.title_bar_border = color(base.hover);
    colors.tiles = color(base.background);
    colors.window_border = color(base.border);
    colors.scrollbar = Hsla::transparent_black();
    colors.scrollbar_thumb = color(base.muted_foreground).opacity(0.44);
    colors.scrollbar_thumb_hover = color(base.scrollbar_hover);
    colors.slider_bar = color(base.hover);
    colors.slider_thumb = primary;
    colors.switch = color(base.hover);
    colors.skeleton = color(base.raised);

    [colors.link, colors.link_hover, colors.link_active] = accent.link;

    [colors.warning, colors.warning_hover, colors.warning_active] = base.warning.map(color);
    colors.warning_foreground = color(base.warning_foreground);

    colors.chart_1 = primary.blend(Hsla::white().opacity(0.7));
    colors.chart_2 = primary.blend(Hsla::white().opacity(0.5));
    colors.chart_3 = primary_hover;
    colors.chart_4 = primary;
    colors.chart_5 = primary_active;

    cx.refresh_windows();
}
