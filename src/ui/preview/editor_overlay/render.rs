use gpui::{PathBuilder, canvas, point};

use super::*;

impl Preview {
    fn size_overlay(
        &self,
        overlay: PreviewSizeOverlay,
        resolution: crate::domain::timeline::ProjectResolution,
        composition_units_per_pixel: f32,
        color: Hsla,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let resolution = [resolution.width() as f32, resolution.height() as f32];
        let left = (overlay.center[0] - overlay.size[0] * 0.5) / resolution[0] + 0.5;
        let top = (overlay.center[1] - overlay.size[1] * 0.5) / resolution[1] + 0.5;
        let width = overlay.size[0] / resolution[0];
        let height = overlay.size[1] / resolution[1];
        let handles = [
            PreviewResizeHandle::Left,
            PreviewResizeHandle::Right,
            PreviewResizeHandle::Top,
            PreviewResizeHandle::Bottom,
        ]
        .into_iter()
        .map(|handle| self.resize_handle(&overlay, handle, composition_units_per_pixel, color, cx))
        .collect::<Vec<_>>();
        div()
            .id(SharedString::from(format!(
                "preview-size-overlay-{}-{}",
                overlay.item_id.get(),
                overlay.target.key()
            )))
            .absolute()
            .left(relative(left))
            .top(relative(top))
            .w(relative(width))
            .h(relative(height))
            .border_1()
            .border_color(color.opacity(if overlay.target.is_keyframe() {
                0.6
            } else {
                1.
            }))
            .children(handles)
    }

    fn motion_path(
        positions: Vec<[f32; 2]>,
        resolution: crate::domain::timeline::ProjectResolution,
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
        resolution: crate::domain::timeline::ProjectResolution,
        composition_units_per_pixel: f32,
        color: Hsla,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let item_id = overlay.item_id;
        let motion_path = overlay.motion_path;
        let positions = overlay
            .positions
            .into_iter()
            .map(|position| {
                self.position_handle(
                    item_id,
                    position,
                    resolution,
                    composition_units_per_pixel,
                    color,
                    cx,
                )
            })
            .collect::<Vec<_>>();
        let sizes = overlay
            .sizes
            .into_iter()
            .map(|size| self.size_overlay(size, resolution, composition_units_per_pixel, color, cx))
            .collect::<Vec<_>>();
        let points = overlay
            .points
            .into_iter()
            .map(|point| {
                self.point_handle(point, resolution, composition_units_per_pixel, color, cx)
            })
            .collect::<Vec<_>>();
        div()
            .absolute()
            .inset_0()
            .when(!motion_path.is_empty(), |this| {
                this.child(Self::motion_path(motion_path, resolution, color))
            })
            .children(sizes)
            .children(points)
            .children(positions)
    }
}
