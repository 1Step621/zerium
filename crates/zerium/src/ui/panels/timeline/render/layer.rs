use crate::app::actions::{OpenPlacement, PlacementKind};
use crate::ui::panels::timeline::LAYER_HEIGHT;
use crate::ui::{
    components::input::file::import::FileImportTarget,
    panels::{
        explorer::ExplorerFileDrag,
        timeline::{LAYER_HEADER_WIDTH, Timeline, grid_canvas, timeline_item},
    },
    workspace::operation_hints::{Hint, OperationHintExt as _},
};
use rust_i18n::t;

use ::ui::{
    Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    menu::context_menu::ContextMenuExt as _,
};
use gpui::{
    Context, Div, DragMoveEvent, ExternalPaths, MouseButton, SharedString, Stateful, div,
    prelude::*, px,
};

use zerium_core::timeline::LayerId;

use super::{LayerRenderState, TimelineItemRenderData, navigation_hints};

fn layer_header(
    layer_number: usize,
    state: &LayerRenderState,
    has_animation: bool,
    cx: &mut Context<Timeline>,
) -> impl IntoElement {
    let layer = LayerId::new(layer_number.saturating_sub(1) as u64);
    let hidden = state.hidden_layers.contains(&layer);
    let editor = state.editor.clone();
    let timeline = cx.entity();
    div()
        .id(("layer-header", layer_number))
        .w(px(LAYER_HEADER_WIDTH))
        .h_full()
        .flex_none()
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .border_r_1()
        .border_color(state.colors.border)
        .on_scroll_wheel(cx.listener(Timeline::on_label_scroll))
        .context_menu(move |menu, _, cx| {
            menu.action_context(timeline.read(cx).focus_handle.clone())
                .menu(
                    t!("text_placement.title").to_string(),
                    Box::new(OpenPlacement {
                        kind: PlacementKind::Text,
                        layer: layer.get(),
                    }),
                )
                .menu(
                    t!("audio_placement.title").to_string(),
                    Box::new(OpenPlacement {
                        kind: PlacementKind::Audio,
                        layer: layer.get(),
                    }),
                )
        })
        .child(
            Button::new(("expand-layer-animation", layer.get()))
                .small()
                .compact()
                .ghost()
                .icon(if state.expanded_layers.contains(&layer) {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .disabled(!has_animation)
                .tooltip(t!("timeline.animation_rows").to_string())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_animation_rows(layer, cx);
                    cx.stop_propagation();
                })),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .overflow_hidden()
                .text_sm()
                .text_color(if hidden {
                    state.colors.muted_foreground
                } else {
                    state.colors.foreground
                })
                .child(format!("Layer {layer_number}")),
        )
        .child(
            Button::new(SharedString::from(format!(
                "toggle-layer-visibility-{}",
                layer.get()
            )))
            .small()
            .compact()
            .ghost()
            .icon(if hidden {
                IconName::EyeOff
            } else {
                IconName::Eye
            })
            .tooltip(if hidden {
                t!("timeline.show_layer").to_string()
            } else {
                t!("timeline.hide_layer").to_string()
            })
            .on_click(move |_, _, cx| {
                editor.update(cx, |editor, cx| {
                    editor.toggle_layer_visibility(layer);
                    cx.notify();
                });
            }),
        )
        .child(
            div()
                .size(px(7.))
                .rounded_full()
                .bg(state.colors.muted_foreground),
        )
        .operation_hints([Hint::gesture(
            "hints.scroll",
            "hints.zoom_time",
            gpui::Modifiers::control(),
        )])
}

pub(in crate::ui::panels::timeline) fn timeline_track(
    id: impl Into<gpui::ElementId>,
    layer_index: usize,
    state: &LayerRenderState,
    cx: &mut Context<Timeline>,
) -> Stateful<Div> {
    let file_drop_highlight = state.colors.primary.opacity(0.08);
    let drop_plugins = state.editor.read(cx).plugin_registry_arc();
    div()
        .id(id)
        .relative()
        .flex_1()
        .h_full()
        .overflow_hidden()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event, window, cx| {
                this.begin_track_interaction(layer_index, event, window, cx);
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event, window, cx| {
                this.begin_track_interaction(layer_index, event, window, cx);
            }),
        )
        .on_scroll_wheel(cx.listener(Timeline::on_track_scroll))
        .on_drag_move(cx.listener(
            move |this, event: &DragMoveEvent<ExplorerFileDrag>, window, cx| {
                this.update_file_drop_target(layer_index, event, window, cx);
            },
        ))
        .on_drag_hover::<ExplorerFileDrag>(cx.listener(move |this, hovered, _, cx| {
            this.clear_file_drop_target(layer_index, *hovered, cx);
        }))
        .drag_over::<ExplorerFileDrag>(move |style, _, _, _| style.bg(file_drop_highlight))
        .on_drop(
            cx.listener(move |this, drag: &ExplorerFileDrag, window, cx| {
                this.drop_files(layer_index, drag.files().to_vec(), window, cx);
            }),
        )
        .can_drop(move |drag, _, _| {
            drag.downcast_ref::<ExternalPaths>().is_none_or(|paths| {
                paths
                    .paths()
                    .iter()
                    .any(|path| FileImportTarget::for_path(path, &drop_plugins).is_some())
            })
        })
        .on_drag_move(cx.listener(
            move |this, event: &DragMoveEvent<ExternalPaths>, window, cx| {
                let plugins = this.editor.read(cx).plugin_registry_arc();
                if event
                    .drag(cx)
                    .paths()
                    .iter()
                    .any(|path| FileImportTarget::for_path(path, &plugins).is_some())
                {
                    this.update_file_drop_target(layer_index, event, window, cx);
                }
            },
        ))
        .on_drag_hover::<ExternalPaths>(cx.listener(move |this, hovered, _, cx| {
            this.clear_file_drop_target(layer_index, *hovered, cx);
        }))
        .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(file_drop_highlight))
        .on_drop(cx.listener(move |this, paths: &ExternalPaths, window, cx| {
            this.drop_external_files(layer_index, paths, window, cx);
        }))
}

pub(in crate::ui::panels::timeline) fn layer_row(
    layer_index: usize,
    state: LayerRenderState,
    items: Vec<TimelineItemRenderData>,
    cx: &mut Context<Timeline>,
) -> Stateful<Div> {
    let layer_number = layer_index + 1;
    let clip_top = (LAYER_HEIGHT * 0.1).max(3.);
    let clip_height = (LAYER_HEIGHT - clip_top * 2.).max(16.);
    let visible_range = state.viewport.visible_time_range(state.viewport_width);
    let file_drop_x = state
        .file_drop_target
        .filter(|target| target.layer == LayerId::new(layer_index as u64))
        .map(|target| {
            state
                .viewport
                .x_at_seconds(state.frame_rate.frame_to_seconds(target.start))
        });
    let timeline_id = cx.entity_id();

    div()
        .id(("timeline-layer", layer_index))
        .relative()
        .h(px(LAYER_HEIGHT))
        .w_full()
        .flex_none()
        .flex()
        .bg(state.layer_background(layer_index))
        .child(layer_header(
            layer_number,
            &state,
            items.iter().any(|item| {
                state.selected_item_ids.contains(&item.item.id)
                    && item.item.animation_tracks().next().is_some()
            }),
            cx,
        ))
        .child(
            timeline_track(("timeline-track", layer_index), layer_index, &state, cx)
                .child(
                    div()
                        .absolute()
                        .size_full()
                        .child(grid_canvas(
                            state.grid.clone(),
                            state.colors.border.opacity(0.45),
                            state.colors.border.opacity(0.20),
                        ))
                        .children(items.into_iter().filter_map(|item| {
                            timeline_item(
                                item,
                                &state,
                                timeline_id,
                                clip_top,
                                clip_height,
                                visible_range,
                                cx,
                            )
                        }))
                        .when_some(file_drop_x, |this, drop_x| {
                            this.child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .left(px(drop_x - 1.))
                                    .w(px(2.))
                                    .bg(state.colors.primary),
                            )
                        }),
                )
                .operation_hints(navigation_hints()),
        )
        .child(
            div()
                .absolute()
                .bottom_0()
                .w_full()
                .h(px(1.))
                .bg(state.colors.table_row_border),
        )
}
