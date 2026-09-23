use super::*;

impl Timeline {
    fn transport_button(id: &'static str, icon: Icon, tooltip: &'static str) -> Button {
        Button::new(id)
            .icon(icon)
            .tooltip(tooltip)
            .small()
            .compact()
    }

    fn ruler(
        &self,
        colors: ThemeColor,
        viewport_width: f32,
        grid: TimelineGrid,
        cx: &mut Context<Self>,
    ) -> Div {
        let viewport = self.viewport;
        let (playhead_seconds, active_scene, scenes) = {
            let editor = self.editor.read(cx);
            (
                editor.playhead_seconds(),
                editor.active_scene_id(),
                editor
                    .scenes()
                    .map(|scene| (scene.id, scene.name.clone()))
                    .collect::<Vec<_>>(),
            )
        };
        let playhead_x = viewport.x_at_seconds(playhead_seconds);
        let ruler_ticks = grid.major_ticks.as_ref().clone();
        let active_scene_name = active_scene
            .and_then(|id| {
                scenes
                    .iter()
                    .find(|(scene_id, _)| *scene_id == id)
                    .map(|(_, name)| name.clone())
            })
            .unwrap_or_else(|| "メイン".to_owned());
        let mut label_width = 0;
        let active_scene_label = active_scene_name
            .char_indices()
            .find_map(|(index, character)| {
                label_width += if character.is_ascii() { 1 } else { 2 };
                (label_width > SCENE_SWITCHER_LABEL_WIDTH - 2)
                    .then(|| format!("{}…", &active_scene_name[..index]))
            })
            .unwrap_or_else(|| active_scene_name.clone());
        let playback_icon = if self.transport.read(cx).is_playing() {
            IconName::Pause
        } else {
            IconName::Play
        };
        let playback_tooltip = if self.transport.read(cx).is_playing() {
            "一時停止"
        } else {
            "再生"
        };

        div()
            .h(px(PANE_HEADER_HEIGHT))
            .flex_none()
            .flex()
            .border_b_1()
            .border_color(colors.border)
            .bg(colors.background)
            .child(
                div()
                    .w(px(LAYER_HEADER_WIDTH))
                    .h_full()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_1()
                    .px_1()
                    .border_r_1()
                    .border_color(colors.border)
                    .bg(colors.title_bar)
                    .on_scroll_wheel(cx.listener(Self::on_label_scroll))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                Self::transport_button(
                                    "timeline-previous",
                                    Icon::new(IconName::ChevronLeft),
                                    "前のフレーム",
                                )
                                .on_click(cx.listener(Self::previous_frame)),
                            )
                            .child(
                                Self::transport_button(
                                    "timeline-play",
                                    Icon::new(playback_icon),
                                    playback_tooltip,
                                )
                                .on_click(cx.listener(Self::toggle_playback_button)),
                            )
                            .child(
                                Self::transport_button(
                                    "timeline-next",
                                    Icon::new(IconName::ChevronRight),
                                    "次のフレーム",
                                )
                                .on_click(cx.listener(Self::next_frame)),
                            ),
                    )
                    .child(
                        Button::new("timeline-scene-switcher")
                            .small()
                            .compact()
                            .ghost()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .label(active_scene_label)
                            .dropdown_caret(true)
                            .tooltip(active_scene_name)
                            .popup_menu({
                                let timeline = cx.entity();
                                move |menu, _, _| {
                                    let root_timeline = timeline.clone();
                                    let menu = menu.item(PopupMenuItem::new("メイン").on_click(
                                        move |_, _, cx| {
                                            root_timeline.update(cx, |timeline, cx| {
                                                timeline.switch_scene(None, cx);
                                            });
                                        },
                                    ));
                                    scenes.iter().cloned().fold(
                                        menu.separator(),
                                        |menu, (id, name)| {
                                            let timeline = timeline.clone();
                                            menu.item(PopupMenuItem::new(name).on_click(
                                                move |_, _, cx| {
                                                    timeline.update(cx, |timeline, cx| {
                                                        timeline.switch_scene(Some(id), cx);
                                                    });
                                                },
                                            ))
                                        },
                                    )
                                }
                            }),
                    ),
            )
            .child(
                div()
                    .id("timeline-ruler-track")
                    .relative()
                    .flex_1()
                    .h_full()
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::begin_playhead_scrub))
                    .on_mouse_move(cx.listener(Self::update_playhead_scrub))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.finish_playhead_scrub(cx)),
                    )
                    .on_scroll_wheel(cx.listener(Self::on_track_scroll))
                    .child(
                        div()
                            .absolute()
                            .size_full()
                            .child(Self::grid_canvas(
                                grid,
                                colors.border.opacity(0.45),
                                colors.border.opacity(0.20),
                            ))
                            .children(ruler_ticks.into_iter().map(|(seconds, tick_x)| {
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .left(px(tick_x))
                                    .border_l_1()
                                    .border_color(colors.border)
                                    .flex()
                                    .items_center()
                                    .pl_1()
                                    .text_sm()
                                    .text_color(colors.muted_foreground)
                                    .child(Self::ruler_tick_label(seconds))
                            }))
                            .when(
                                playhead_x >= -4. && playhead_x <= viewport_width + 4.,
                                |this| {
                                    this.child(
                                        div()
                                            .absolute()
                                            .top_0()
                                            .bottom_0()
                                            .left(px(playhead_x))
                                            .w(px(2.))
                                            .bg(colors.primary),
                                    )
                                },
                            )
                            .when(
                                playhead_x >= -4. && playhead_x <= viewport_width + 4.,
                                |this| {
                                    this.child(
                                        div()
                                            .absolute()
                                            .top_0()
                                            .left(px(playhead_x - 4.))
                                            .size(px(8.))
                                            .rounded_b_sm()
                                            .bg(colors.primary),
                                    )
                                },
                            ),
                    ),
            )
    }

    fn layer_header(layer_number: usize, state: &LayerRenderState, cx: &mut Context<Self>) -> Div {
        let layer = LayerId::new(layer_number.saturating_sub(1) as u64);
        let hidden = state.hidden_layers.contains(&layer);
        let editor = state.editor.clone();
        div()
            .w(px(LAYER_HEADER_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .border_r_1()
            .border_color(state.colors.border)
            .on_scroll_wheel(cx.listener(Self::on_label_scroll))
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
                    "レイヤーを表示"
                } else {
                    "レイヤーを非表示"
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
    }

    fn timeline_item(
        render_item: TimelineItemRenderData,
        state: &LayerRenderState,
        timeline_id: EntityId,
        clip_top: f32,
        clip_height: f32,
        visible_range: (f64, f64),
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let TimelineItemRenderData {
            item,
            label: item_label,
        } = render_item;
        let item_start = state.frame_rate.frame_to_seconds(item.start);
        let item_end = state.frame_rate.frame_to_seconds(item.end_exclusive());
        let (visible_start, visible_end) = visible_range;

        if item_end <= visible_start || item_start >= visible_end {
            return None;
        }

        let item_id = item.id;
        let scene_id = item.scene_id();
        let item_hidden = state.hidden_items.contains(&item_id);
        let item_left = state.viewport.x_at_seconds(item_start);
        let item_width = (state
            .frame_rate
            .frame_to_seconds(Frame::new(item.duration.get()))
            * state.viewport.pixels_per_second()) as f32;
        let is_selected = state.selected_item_ids.contains(&item_id);
        let snap_frame = state.editor.read(cx).playhead();
        let mut animation_stops = Vec::new();
        for (_, track) in item.animations.tracks() {
            animation_stops.extend(track.stops().iter().map(|stop| stop.position()));
        }
        for effect in &item.effects {
            for (_, track) in effect.animations.tracks() {
                animation_stops.extend(track.stops().iter().map(|stop| stop.position()));
            }
        }
        animation_stops.sort_by(f32::total_cmp);
        animation_stops
            .dedup_by(|left, right| (*left - *right).abs() < ANIMATION_STOP_POSITION_EPSILON);
        let focused_target = state
            .animation_target
            .as_ref()
            .filter(|target| target.item_id == item_id && item.contains(snap_frame))
            .cloned();
        let has_focused_target = focused_target.is_some();
        let focused_animation_stops = focused_target
            .as_ref()
            .and_then(|target| {
                let track = item.animation_track(
                    target.effect_id,
                    &target.property_id,
                    target.element_id,
                    target.scalar_index,
                )?;
                Some((target.clone(), track))
            })
            .map(|(target, track)| {
                let stop_count = track.stops().len();
                track
                    .stops()
                    .iter()
                    .enumerate()
                    .map(|(stop, animation_stop)| {
                        let progress = animation_stop.position();
                        let frame = Frame::new(
                            item.animation_timeline_frame(progress).round().max(0.) as u64,
                        );
                        let follow_focus = (snap_frame == frame)
                            .then_some(state.focused_animation_segment)
                            .flatten()
                            .filter(|segment| {
                                *segment == stop || segment.saturating_add(1) == stop
                            });
                        (
                            target.clone(),
                            stop,
                            progress,
                            frame,
                            follow_focus,
                            stop > 0 && stop + 1 < stop_count,
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let move_drag = MoveTimelineItem {
            timeline_id,
            item_id,
        };
        let resize_origins =
            Rc::<[TimelineItem]>::from(if state.selected_item_ids.contains(&item_id) {
                state.editor.read(cx).selected_items()
            } else {
                vec![item.clone()]
            });
        let left_drag = ResizeTimelineItem {
            timeline_id,
            origins: resize_origins.clone(),
            anchor_id: item_id,
            edge: ResizeEdge::Left,
        };
        let right_drag = ResizeTimelineItem {
            timeline_id,
            origins: resize_origins,
            anchor_id: item_id,
            edge: ResizeEdge::Right,
        };
        Some(
            div()
                .id(("timeline-item", item_id.get()))
                .absolute()
                .top(px(clip_top))
                .left(px(item_left))
                .h(px(clip_height))
                .w(px(item_width))
                .min_w_0()
                .max_w(px(item_width))
                .flex()
                .items_center()
                .gap_1()
                .overflow_hidden()
                .cursor_pointer()
                .rounded_sm()
                .border_1()
                .border_color(if is_selected {
                    state.colors.primary
                } else {
                    state.colors.border
                })
                .bg(if is_selected {
                    state.colors.primary.opacity(0.24)
                } else if item_hidden {
                    state.colors.accent.opacity(0.35)
                } else {
                    state.colors.accent
                })
                .when(is_selected, |item| item.border_2())
                .text_sm()
                .text_color(if item_hidden {
                    state.colors.muted_foreground
                } else {
                    state.colors.accent_foreground
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event, _, cx| {
                        this.begin_item_interaction(item_id, event, cx);
                    }),
                )
                .on_click(cx.listener(move |this, event, _, cx| {
                    this.open_scene_item(item_id, event, cx);
                }))
                .on_drag(move_drag, |drag, _, _, cx| {
                    cx.stop_propagation();
                    cx.new(|_| drag.clone())
                })
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.finish_item_move(cx)),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .items_center()
                        .gap_1()
                        .pl_1()
                        .overflow_hidden()
                        .when(scene_id.is_none(), |this| {
                            this.child(
                                div()
                                    .text_color(state.colors.primary)
                                    .child(item.symbol().to_owned()),
                            )
                        })
                        .child(item_label),
                )
                .child(
                    div()
                        .id(("timeline-item-left-handle", item_id.get()))
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left_0()
                        .w(px(6.))
                        .cursor_col_resize()
                        .bg(state.colors.primary.opacity(0.35))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event, _, cx| {
                                this.select_item(item_id, event, cx);
                            }),
                        )
                        .on_drag(left_drag, |drag, _, _, cx| {
                            cx.stop_propagation();
                            cx.new(|_| drag.clone())
                        }),
                )
                .child(
                    div()
                        .id(("timeline-item-right-handle", item_id.get()))
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .right_0()
                        .w(px(6.))
                        .cursor_col_resize()
                        .bg(state.colors.primary.opacity(0.35))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event, _, cx| {
                                this.select_item(item_id, event, cx);
                            }),
                        )
                        .on_drag(right_drag, |drag, _, _, cx| {
                            cx.stop_propagation();
                            cx.new(|_| drag.clone())
                        }),
                )
                .children(animation_stops.into_iter().map(|progress| {
                    let margin_left = if progress <= f32::EPSILON {
                        0.
                    } else if progress >= 1. - f32::EPSILON {
                        -5.
                    } else {
                        -2.5
                    };
                    div()
                        .absolute()
                        .top(px(2.))
                        .left(relative(progress))
                        .ml(px(margin_left))
                        .size(px(5.))
                        .rounded_full()
                        .border_1()
                        .border_color(state.colors.background.opacity(0.7))
                        .bg(state.colors.primary.opacity(if has_focused_target {
                            0.35
                        } else if is_selected {
                            0.95
                        } else {
                            0.65
                        }))
                }))
                .children(focused_animation_stops.into_iter().map({
                    let timeline_editor = state.editor.clone();
                    let transport = state.transport.clone();
                    let colors = state.colors;
                    move |(target, stop, progress, frame, follow_focus, movable)| {
                        let drag = MoveAnimationStop {
                            timeline_id,
                            target,
                            stop,
                            snap_frame,
                            follow_focus,
                        };
                        let start_editor = timeline_editor.clone();
                        let seek_transport = transport.clone();
                        let margin_left = if progress <= f32::EPSILON {
                            0.
                        } else if progress >= 1. - f32::EPSILON {
                            -8.
                        } else {
                            -4.
                        };
                        div()
                            .id(("focused-animation-stop", stop))
                            .absolute()
                            .top(px(1.))
                            .left(relative(progress))
                            .ml(px(margin_left))
                            .size(px(8.))
                            .rounded_full()
                            .border_2()
                            .border_color(colors.background)
                            .bg(colors.warning)
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                cx.stop_propagation();
                                if movable {
                                    start_editor.update(cx, |editor, _| {
                                        editor.finish_history_group();
                                    });
                                }
                            })
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                seek_transport.update(cx, |transport, cx| {
                                    transport.set_playhead(frame, cx);
                                });
                            })
                            .when(movable, |marker| {
                                marker
                                    .cursor_col_resize()
                                    .hover(move |style| style.bg(colors.warning.lighten(0.14)))
                                    .active(move |style| style.bg(colors.warning.darken(0.14)))
                                    .on_drag(drag, |drag, _, _, cx| {
                                        cx.stop_propagation();
                                        cx.new(|_| drag.clone())
                                    })
                            })
                    }
                })),
        )
    }

    fn layer_row(
        layer_index: usize,
        state: LayerRenderState,
        items: Vec<TimelineItemRenderData>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let layer_number = layer_index + 1;
        let clip_top = (state.layer_height * 0.1).max(3.);
        let clip_height = (state.layer_height - clip_top * 2.).max(16.);
        let playhead_x = state.viewport.x_at_seconds(state.playhead_seconds);
        let visible_range = state.viewport.visible_time_range(state.viewport_width);
        let explorer_drop_x = state
            .explorer_drop_target
            .filter(|target| target.layer == LayerId::new(layer_index as u64))
            .map(|target| {
                state
                    .viewport
                    .x_at_seconds(state.frame_rate.frame_to_seconds(target.start))
            });
        let explorer_drop_highlight = state.colors.primary.opacity(0.08);
        let render_result_highlights = state
            .render_result_highlights
            .iter()
            .copied()
            .filter_map(|highlight| {
                let layer = layer_index as u64;
                if !(highlight.top_layer.get()..=highlight.bottom_layer.get()).contains(&layer) {
                    return None;
                }
                let start = state.frame_rate.frame_to_seconds(highlight.start);
                let end = state.frame_rate.frame_to_seconds(highlight.end);
                if end <= visible_range.0 || start >= visible_range.1 {
                    return None;
                }
                let left = state.viewport.x_at_seconds(start);
                let width = ((end - start) * state.viewport.pixels_per_second()) as f32;
                Some((
                    left,
                    width,
                    layer == highlight.top_layer.get(),
                    layer == highlight.bottom_layer.get(),
                ))
            })
            .collect::<Vec<_>>();
        let timeline_id = cx.entity_id();

        div()
            .id(("timeline-layer", layer_index))
            .h(px(state.layer_height))
            .w_full()
            .flex_none()
            .flex()
            .border_b_1()
            .border_color(state.colors.table_row_border)
            .bg(if layer_index.is_multiple_of(2) {
                state.colors.background
            } else {
                state.colors.table_even
            })
            .child(Self::layer_header(layer_number, &state, cx))
            .child(
                div()
                    .id(("timeline-track", layer_index))
                    .relative()
                    .flex_1()
                    .h_full()
                    .overflow_hidden()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event, window, cx| {
                            this.begin_primary_track_interaction(layer_index, event, window, cx);
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event, _, cx| {
                            this.begin_marquee_selection(layer_index, event, cx);
                        }),
                    )
                    .on_scroll_wheel(cx.listener(Self::on_track_scroll))
                    .on_drag_move(cx.listener(
                        move |this, event: &DragMoveEvent<ExplorerFileDrag>, _, cx| {
                            this.update_explorer_drop_target(layer_index, event, cx);
                        },
                    ))
                    .on_drag_hover::<ExplorerFileDrag>(cx.listener(move |this, hovered, _, cx| {
                        this.clear_explorer_drop_target(layer_index, *hovered, cx);
                    }))
                    .drag_over::<ExplorerFileDrag>(move |style, _, _, _| {
                        style.bg(explorer_drop_highlight)
                    })
                    .on_drop(
                        cx.listener(move |this, drag: &ExplorerFileDrag, window, cx| {
                            this.drop_explorer_items(layer_index, drag, window, cx);
                        }),
                    )
                    .child(
                        div()
                            .absolute()
                            .size_full()
                            .child(Self::grid_canvas(
                                state.grid.clone(),
                                state.colors.border.opacity(0.45),
                                state.colors.border.opacity(0.20),
                            ))
                            .children(render_result_highlights.into_iter().map(
                                |(left, width, is_top, is_bottom)| {
                                    div()
                                        .absolute()
                                        .top_0()
                                        .bottom_0()
                                        .left(px(left))
                                        .w(px(width))
                                        .border_l_1()
                                        .border_r_1()
                                        .when(is_top, |highlight| highlight.border_t_1())
                                        .when(is_bottom, |highlight| highlight.border_b_1())
                                        .border_color(state.colors.primary.opacity(0.55))
                                        .bg(state.colors.primary.opacity(0.09))
                                },
                            ))
                            .children(items.into_iter().filter_map(|item| {
                                Self::timeline_item(
                                    item,
                                    &state,
                                    timeline_id,
                                    clip_top,
                                    clip_height,
                                    visible_range,
                                    cx,
                                )
                            }))
                            .when(
                                playhead_x >= -2. && playhead_x <= state.viewport_width + 2.,
                                |this| {
                                    this.child(
                                        div()
                                            .absolute()
                                            .top_0()
                                            .bottom_0()
                                            .left(px(playhead_x))
                                            .w(px(2.))
                                            .bg(state.colors.primary),
                                    )
                                },
                            )
                            .when_some(explorer_drop_x, |this, drop_x| {
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
                    ),
            )
    }
}

impl Render for Timeline {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let layer_count = self.dynamic_layer_count(window, cx);
        let colors = cx.theme().colors;
        let viewport_width = Self::track_viewport_width(window);
        let layer_height = self.viewport.layer_height;
        let (
            frame_rate,
            playhead,
            playhead_seconds,
            selected_item_ids,
            render_result_highlights,
            hidden_layers,
            hidden_items,
            active_scene,
        ) = {
            let editor = self.editor.read(cx);
            let selected_items = editor.selected_items();
            let render_result_highlights = selected_items
                .iter()
                .filter_map(|item| {
                    let source_layer = editor.item_layer(item.id)?;
                    let settings = item.render_result_settings()?;
                    let (top_layer, bottom_layer) = settings.layer_bounds(source_layer)?;
                    Some(RenderResultHighlight {
                        top_layer,
                        bottom_layer,
                        start: item.start,
                        end: item.end_exclusive(),
                    })
                })
                .collect();
            (
                editor.frame_rate(),
                editor.playhead(),
                editor.playhead_seconds(),
                Rc::new(selected_items.iter().map(|item| item.id).collect()),
                Rc::new(render_result_highlights),
                Rc::new(editor.hidden_layer_ids().collect()),
                Rc::new(editor.hidden_item_ids().collect()),
                editor
                    .active_scene_id()
                    .and_then(|id| editor.scene(id))
                    .map(|scene| (scene.name.clone(), scene.is_empty())),
            )
        };
        self.viewport
            .follow_playhead(playhead, viewport_width, frame_rate);
        let active_scene_is_empty = active_scene.is_some_and(|(_, is_empty)| is_empty);
        let grid = Self::timeline_grid(self.viewport, viewport_width, frame_rate);
        let ruler = self.ruler(colors, viewport_width, grid.clone(), cx);
        let animation_selection = self.animation_selection.read(cx);
        let row_state = LayerRenderState {
            editor: self.editor.clone(),
            transport: self.transport.clone(),
            animation_target: animation_selection.target().cloned(),
            focused_animation_segment: animation_selection.focused_segment(),
            colors,
            layer_height,
            viewport: self.viewport,
            viewport_width,
            frame_rate,
            playhead_seconds,
            selected_item_ids,
            render_result_highlights,
            hidden_layers,
            hidden_items,
            explorer_drop_target: self.explorer_drop_target,
            grid,
        };
        let timeline = cx.entity();
        let timeline_for_mouse_leave = timeline.clone();
        let file_drop_error = self.file_drop_error.clone();
        let focus_handle = self.focus_handle.clone();

        div()
            .id("timeline-root")
            .relative()
            .track_focus(&self.focus_handle)
            .capture_any_mouse_down(move |_, window, cx| {
                focus_handle.focus(window, cx);
            })
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_mouse_move(cx.listener(|this, event, window, cx| {
                this.update_playhead_scrub(event, window, cx);
                this.update_marquee_selection(event, window, cx);
            }))
            .on_mouse_leave(move |_, cx| {
                timeline_for_mouse_leave.update(cx, |timeline, cx| {
                    if timeline.marquee_selection.take().is_some() {
                        cx.notify();
                    }
                });
            })
            .on_click(cx.listener(Self::clear_selection_on_double_click))
            .capture_any_mouse_up(cx.listener(|this, event: &MouseUpEvent, window, cx| {
                if event.button == MouseButton::Left {
                    this.finish_playhead_scrub(cx);
                    this.finish_item_move(cx);
                    this.finish_marquee_selection(event, window, cx);
                } else if event.button == MouseButton::Right {
                    this.finish_marquee_selection(event, window, cx);
                }
            }))
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<MoveTimelineItem>, window, cx| {
                    let drag = event.drag(cx).clone();
                    cx.set_active_drag_cursor_style(CursorStyle::ClosedHand, window);
                    this.move_item_from_pointer(
                        &drag,
                        f32::from(event.event.position.x),
                        f32::from(event.event.position.y),
                        event.event.modifiers.alt,
                        window,
                        cx,
                    );
                },
            ))
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<ResizeTimelineItem>, window, cx| {
                    let drag = event.drag(cx).clone();
                    this.resize_item_from_pointer(
                        &drag,
                        f32::from(event.event.position.x),
                        event.event.modifiers.alt,
                        window,
                        cx,
                    );
                },
            ))
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<MoveAnimationStop>, window, cx| {
                    let drag = event.drag(cx).clone();
                    cx.set_active_drag_cursor_style(CursorStyle::ResizeLeftRight, window);
                    this.move_animation_stop_from_pointer(
                        &drag,
                        f32::from(event.event.position.x),
                        event.event.modifiers.alt,
                        window,
                        cx,
                    );
                },
            ))
            .bg(colors.background)
            .text_color(colors.foreground)
            .child(ruler)
            .child(
                uniform_list(
                    "timeline-layers",
                    layer_count,
                    move |visible_range, _window, cx| {
                        timeline.update(cx, |timeline, cx| {
                            visible_range
                                .map(|layer_index| {
                                    let items = {
                                        let editor = timeline.editor.read(cx);
                                        editor
                                            .items_on_layer(LayerId::new(layer_index as u64))
                                            .into_iter()
                                            .map(|item| {
                                                let label = editor
                                                    .item_label(item.id)
                                                    .unwrap_or_else(|| "不明なアイテム".to_owned());
                                                TimelineItemRenderData { item, label }
                                            })
                                            .collect()
                                    };
                                    Self::layer_row(layer_index, row_state.clone(), items, cx)
                                })
                                .collect::<Vec<_>>()
                        })
                    },
                )
                .track_scroll(&self.layer_scroll)
                .flex_1()
                .min_h_0()
                .w_full(),
            )
            .when_some(self.marquee_overlay(colors), |this, marquee| {
                this.child(marquee)
            })
            .when_some(
                self.context_menu_overlay(colors, cx.entity()),
                |this, menu| this.child(menu),
            )
            .when(active_scene_is_empty, |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap_3()
                                .px_5()
                                .py_4()
                                .rounded_md()
                                .border_1()
                                .border_color(colors.border)
                                .bg(colors.background.opacity(0.96))
                                .shadow_md()
                                .text_sm()
                                .child("このシーンは空です")
                                .child(
                                    Button::new("timeline-delete-empty-scene")
                                        .small()
                                        .danger()
                                        .icon(IconName::Delete)
                                        .label("シーンを削除")
                                        .tooltip("このシーンと、配置済みの全インスタンスを削除")
                                        .on_click(cx.listener(Self::delete_empty_scene)),
                                ),
                        ),
                )
            })
            .when_some(file_drop_error, |this, error| {
                this.child(
                    div()
                        .absolute()
                        .left(px(8.))
                        .bottom(px(8.))
                        .max_w(px(520.))
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .border_1()
                        .border_color(colors.danger)
                        .bg(colors.background.opacity(0.96))
                        .text_sm()
                        .text_color(colors.danger)
                        .child(error),
                )
            })
    }
}
