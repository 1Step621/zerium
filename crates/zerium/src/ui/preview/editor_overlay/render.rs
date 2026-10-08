use gpui::{PathBuilder, canvas, point};

use super::*;

impl Preview {
    fn scalar_control(
        &self,
        control: PreviewScalarControl,
        resolution: zerium_core::timeline::ProjectResolution,
        composition_units_per_pixel: f32,
        color: Hsla,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let axis = control.scalar.axis();
        let key = control.key();
        let active = matches!(&self.editor_drag, Some(origin)
            if origin.control.key() == key);
        let position = [
            control.position[0] / resolution.width() as f32 + 0.5,
            control.position[1] / resolution.height() as f32 + 0.5,
        ];
        let drag = PreviewEditorDrag {
            preview_id: cx.entity_id(),
            control_key: key.clone(),
        };
        let hit_size = if axis == 0 { [8., 20.] } else { [20., 8.] };
        let preview_size = [resolution.width() as f32, resolution.height() as f32]
            .map(|extent| extent / composition_units_per_pixel);
        let guide = div()
            .absolute()
            .bg(color.opacity(0.6))
            .invisible()
            .group_hover(key.clone(), |style| style.visible())
            .when(active, |this| this.visible())
            .when(axis == 0, |this| {
                this.left(px(hit_size[0] / 2.))
                    .top(px(hit_size[1] / 2. - position[1] * preview_size[1]))
                    .w(px(1.))
                    .h(px(preview_size[1]))
            })
            .when(axis == 1, |this| {
                this.top(px(hit_size[1] / 2.))
                    .left(px(hit_size[0] / 2. - position[0] * preview_size[0]))
                    .h(px(1.))
                    .w(px(preview_size[0]))
            });
        div()
            .id(key.clone())
            .group(key)
            .absolute()
            .left(relative(position[0]))
            .top(relative(position[1]))
            .ml(px(-hit_size[0] / 2.))
            .mt(px(-hit_size[1] / 2.))
            .w(px(hit_size[0]))
            .h(px(hit_size[1]))
            .cursor(control.cursor())
            .child(guide)
            .child(
                div()
                    .absolute()
                    .bg(color)
                    .when(axis == 0, |this| {
                        this.left(px(3.)).top(px(4.)).w(px(2.)).h(px(12.))
                    })
                    .when(axis == 1, |this| {
                        this.top(px(3.)).left(px(4.)).h(px(2.)).w(px(12.))
                    }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event, _, cx| {
                    this.begin_scalar_drag(&control, composition_units_per_pixel, event, cx);
                }),
            )
            .on_drag(drag, move |drag, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| drag.clone())
            })
    }

    fn size_overlay(
        bounds: PreviewBounds,
        resolution: zerium_core::timeline::ProjectResolution,
        color: Hsla,
    ) -> impl IntoElement {
        let resolution = [resolution.width() as f32, resolution.height() as f32];
        div()
            .absolute()
            .left(relative(
                (bounds.center[0] - bounds.size[0] * 0.5) / resolution[0] + 0.5,
            ))
            .top(relative(
                (bounds.center[1] - bounds.size[1] * 0.5) / resolution[1] + 0.5,
            ))
            .w(relative(bounds.size[0] / resolution[0]))
            .h(relative(bounds.size[1] / resolution[1]))
            .border_1()
            .border_color(color.opacity(0.6))
    }

    fn path_overlay(
        positions: Vec<[f32; 2]>,
        resolution: zerium_core::timeline::ProjectResolution,
        color: Hsla,
    ) -> impl IntoElement {
        canvas(
            move |_, _, _| positions.clone(),
            move |bounds, positions, window, _| {
                let to_point = |position: [f32; 2]| {
                    point(
                        bounds.origin.x
                            + bounds.size.width * (position[0] / resolution.width() as f32 + 0.5),
                        bounds.origin.y
                            + bounds.size.height * (position[1] / resolution.height() as f32 + 0.5),
                    )
                };
                let mut builder = PathBuilder::stroke(px(1.5));
                let mut positions = positions.iter().copied();
                if let Some(first) = positions.next() {
                    builder.move_to(to_point(first));
                    for position in positions {
                        builder.line_to(to_point(position));
                    }
                    if let Ok(path) = builder.build() {
                        window.paint_path(path, color.opacity(0.75));
                    }
                }
            },
        )
        .absolute()
        .inset_0()
        .size_full()
    }

    pub(in crate::ui::preview) fn editor_overlay(
        &self,
        overlay: PreviewEditorOverlay,
        resolution: zerium_core::timeline::ProjectResolution,
        composition_units_per_pixel: f32,
        color: Hsla,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        div()
            .absolute()
            .inset_0()
            .children(
                overlay
                    .motion_paths
                    .into_iter()
                    .map(|path| Self::path_overlay(path, resolution, color)),
            )
            .when(!overlay.spline_path.is_empty(), |this| {
                this.child(Self::path_overlay(overlay.spline_path, resolution, color))
            })
            .when_some(overlay.bounds, |this, bounds| {
                this.child(Self::size_overlay(bounds, resolution, color))
            })
            .children(overlay.points.into_iter().map(|point| {
                div()
                    .absolute()
                    .left(relative(point[0] / resolution.width() as f32 + 0.5))
                    .top(relative(point[1] / resolution.height() as f32 + 0.5))
                    .ml(px(-2.5))
                    .mt(px(-2.5))
                    .size(px(5.))
                    .rounded_full()
                    .bg(color)
            }))
            .children(overlay.controls.into_iter().map(|control| {
                self.scalar_control(control, resolution, composition_units_per_pixel, color, cx)
            }))
    }
}
