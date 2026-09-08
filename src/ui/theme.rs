use ::ui::{Theme, ThemeMode};
use gpui::{App, Hsla, rgb, rgba};

const BACKGROUND: u32 = 0x282c34;
const SURFACE: u32 = 0x2c313a;
const SURFACE_RAISED: u32 = 0x343a45;
const SURFACE_HOVER: u32 = 0x3e4451;

const BORDER: u32 = 0x4b5263;

const FOREGROUND: u32 = 0xabb2bf;
const MUTED_FOREGROUND: u32 = 0x7f848e;

const ORANGE: u32 = 0xf97316;
const ORANGE_HOVER: u32 = 0xfb923c;
const ORANGE_ACTIVE: u32 = 0xea580c;

const ORANGE_SOFT: u32 = 0x3d3733;
const ORANGE_FOREGROUND: u32 = 0x21160e;

fn color(hex: u32) -> Hsla {
    rgb(hex).into()
}

fn color_with_alpha(hex: u32) -> Hsla {
    rgba(hex).into()
}

pub(crate) fn install(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);

    let theme = Theme::global_mut(cx);

    let colors = &mut theme.colors;

    colors.background = color(BACKGROUND);
    colors.foreground = color(FOREGROUND);
    colors.border = color(BORDER);

    colors.input = color(0x3e4451);
    colors.ring = color(ORANGE);
    colors.caret = color(ORANGE);
    colors.drag_border = color(ORANGE);
    colors.drop_target = color_with_alpha(0xf9731640);
    colors.selection = color_with_alpha(0xf973164d);

    // Primary
    colors.primary = color(ORANGE);
    colors.primary_hover = color(ORANGE_HOVER);
    colors.primary_active = color(ORANGE_ACTIVE);
    colors.primary_foreground = color(ORANGE_FOREGROUND);
    colors.progress_bar = color(ORANGE);

    // Accent / Secondary / Muted
    colors.accent = color(ORANGE_SOFT);
    colors.accent_foreground = color(ORANGE_HOVER);

    colors.secondary = color(SURFACE_RAISED);
    colors.secondary_hover = color(SURFACE_HOVER);
    colors.secondary_active = color(ORANGE_SOFT);
    colors.secondary_foreground = color(FOREGROUND);

    colors.muted = color(SURFACE_RAISED);
    colors.muted_foreground = color(MUTED_FOREGROUND);

    // Containers
    colors.accordion = color(SURFACE);
    colors.accordion_hover = color(SURFACE_HOVER);

    colors.group_box = color(SURFACE);
    colors.group_box_foreground = color(FOREGROUND);

    colors.popover = color(0x303640);
    colors.popover_foreground = color(FOREGROUND);

    // Lists
    colors.list = color(BACKGROUND);
    colors.list_head = color(SURFACE);
    colors.list_even = color(0x2a2f37);
    colors.list_hover = color(SURFACE_HOVER);
    colors.list_active = color_with_alpha(0xf973162e);
    colors.list_active_border = color(ORANGE);

    // Tables
    colors.table = color(BACKGROUND);
    colors.table_head = color(SURFACE);
    colors.table_head_foreground = color(MUTED_FOREGROUND);
    colors.table_even = color(0x2a2f37);
    colors.table_hover = color(SURFACE_HOVER);
    colors.table_active = color_with_alpha(0xf973162e);
    colors.table_active_border = color(ORANGE);
    colors.table_row_border = color(0x3e4451);

    // Sidebar
    colors.sidebar = color(SURFACE);
    colors.sidebar_foreground = color(FOREGROUND);
    colors.sidebar_border = color(0x3e4451);
    colors.sidebar_accent = color(ORANGE_SOFT);
    colors.sidebar_accent_foreground = color(ORANGE_HOVER);
    colors.sidebar_primary = color(ORANGE);
    colors.sidebar_primary_foreground = color(ORANGE_FOREGROUND);

    // Tabs
    colors.tab = color(BACKGROUND);
    colors.tab_bar = color(SURFACE);
    colors.tab_bar_segmented = color(SURFACE_RAISED);
    colors.tab_foreground = color(MUTED_FOREGROUND);
    colors.tab_active = color(ORANGE_SOFT);
    colors.tab_active_foreground = color(ORANGE_HOVER);

    // Window chrome
    colors.title_bar = color(SURFACE);
    colors.title_bar_border = color(0x3e4451);
    colors.tiles = color(BACKGROUND);
    colors.window_border = color(BORDER);

    // Controls
    colors.scrollbar = color_with_alpha(0x282c3400);
    colors.scrollbar_thumb = color_with_alpha(0x7f848e70);
    colors.scrollbar_thumb_hover = color(0x9da5b4);

    colors.slider_bar = color(SURFACE_HOVER);
    colors.slider_thumb = color(ORANGE);

    colors.switch = color(SURFACE_HOVER);
    colors.skeleton = color(SURFACE_RAISED);

    // Links / warnings
    colors.link = color(ORANGE);
    colors.link_hover = color(ORANGE_HOVER);
    colors.link_active = color(ORANGE_ACTIVE);

    colors.warning = color(ORANGE);
    colors.warning_hover = color(ORANGE_HOVER);
    colors.warning_active = color(ORANGE_ACTIVE);
    colors.warning_foreground = color(ORANGE_FOREGROUND);

    // Charts
    colors.chart_1 = color(0xffd7b5);
    colors.chart_2 = color(0xfdba74);
    colors.chart_3 = color(ORANGE_HOVER);
    colors.chart_4 = color(ORANGE);
    colors.chart_5 = color(ORANGE_ACTIVE);
}
