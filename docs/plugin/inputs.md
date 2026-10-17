# Visual and audio inputs

[Plugin API v1](../plugin.md) · [Properties](properties.md) · [Rendering](rendering.md) · **Inputs**

Items and effects declare visual inputs in `render.inputs`. Array order fixes GPU
binding order; IDs must be unique WGSL identifiers. At most eight inputs are
allowed. Inputs contain scene-linear, premultiplied color and are accessed through
the generated [image sampling functions](rendering.md#image-sampling).

## Media

```json
{
  "type": "media",
  "id": "source",
  "file": "source_file",
  "reader": "zerium.video",
  "placement": { "position": "position", "size": "size", "origin": "origin" },
  "playback": {
    "source_start": "source_start",
    "source_duration": "source_duration",
    "playback_speed": "rate",
    "end_behavior": "end_behavior"
  }
}
```

`file` references a file property; `reader` selects the decoder. The built-in
`zerium.video` reader handles video, `zerium.image` handles raster images,
`zerium.audio` handles audio streams (including those in video files), and
`zerium.svg` handles SVG images. Multiple inputs can share the file with
different readers or playback settings. Missing frames produce a transparent texture.

`placement` is optional. Its position and size are two-`f32` tuples; its optional
origin uses the [origin contract](properties.md#editor-features). The host uses
placement to request sufficient source resolution. Without placement it requests the render
target size. The shader receives the resolution returned by the reader.

`playback` is optional and applies to temporal media. Without it, video starts at
zero, runs at 1× speed, and stops at EOF. Images provide a static texture.

## Playback and audio

Audio is declared in an item's top-level `audio` array, independently of visual
inputs:

```json
{
  "audio": [{
    "id": "sound",
    "file": "source_file",
    "reader": "zerium.audio",
    "volume": "volume",
    "preserve_pitch": "preserve_pitch"
  }]
}
```

Audio IDs must be unique within the item. `volume` references an `f32` linear gain
property; a missing audio stream contributes silence.

Visual playback, audio playback, and timeline editing each use their own explicit
property references. Share property IDs to synchronize their settings.

| Reference | Property type | Meaning |
| --- | --- | --- |
| `source_start` | `f32` | Nonnegative source start in seconds |
| `source_duration` | `f32` | Positive source duration in seconds |
| `playback_speed` | `f32` | Playback multiplier, 0.25–4 |
| `end_behavior` | Enum `[0, 1, 2]` | Stop, loop, hold |
| `preserve_pitch` (audio) | `bool` | Preserve pitch when changing speed |

Audio accepts the same optional `playback` block as visual media and additionally
requires `preserve_pitch` outside that block.
The three time-mapping references must be distinct. All settings in this
table must disable animation and set `scene_bindable: false`.

Playback reads the interval from `source_start` to `source_start + source_duration`
at the declared speed. Intervals can extend beyond the stream. Stop produces
transparent video or silent audio at EOF; loop wraps the stream; hold repeats
the last video frame and produces silent audio. EOF policy does not alter clip
length.

## Text

A text input declares all properties used by the rasterizer:

```json
{
  "type": "text",
  "id": "title",
  "text": "text",
  "style": {
    "size": "size",
    "font_family": "font_family",
    "font_size": "font_size",
    "color": "color",
    "outline_width": "outline_width",
    "outline_color": "outline_color",
    "bold": "bold",
    "italic": "italic",
    "horizontal_alignment": "horizontal_alignment",
    "vertical_alignment": "vertical_alignment"
  }
}
```

`text` references a `string` property. In `style`, `size` is a two-`f32` tuple and
`font_family` is a string array. Font size and outline width are `f32`; colors are
`color`; bold and italic are `bool`. Alignment enums must contain exactly `0`,
`1`, and `2`.

## Number

A `number` capability uses the same `style` and rasterizer as [text](#text),
with `value` and `decimal_places` references instead of `text`.

| Reference | Property type | Meaning |
| --- | --- | --- |
| `value` | `f32`, `i32`, or `u32` | Number to display |
| `decimal_places` | `u32` constrained to `0..=10` | Fixed decimal precision |

Formatting follows animation evaluation: `12.3` with two places displays as `12.30`.
See the bundled [Number shader](../../plugins/zerium.builtin/items/number/render.wesl).

## Render result

```json
{
  "type": "render_result",
  "id": "behind",
  "start_offset": "start_offset",
  "end_offset": "end_offset",
  "hide_original": "hide_original"
}
```

The two offsets reference `u32` properties constrained to `1..=30`. Offset `1`
means the layer immediately behind the owner. The range is inclusive, accepts
either order, and supplies transparency for out-of-range layers. Offsets inside
a scene are local to that scene. The `hide_original` bool controls whether those
layers also appear in the scene's normal output.

Without a `view`, the input retains the selected composition's complete logical
bounds. An optional `view` produces a camera image clipped to the scene viewport:

```json
{ "view": { "position": "position", "zoom": "zoom", "angle": "angle" } }
```

`position` is an `f32` pair in scene pixels, `zoom` an `f32` percentage with a
positive minimum, and `angle` an `f32` clockwise camera angle in degrees. These
properties support animation. A view can bring offscreen items into the capture,
including content inside nested scenes and their effects. Scene bounds are
retained until a camera view or final output clips the image. Nested camera views
keep their own viewport clipping; plain render results do not introduce a clip.
See [Render surfaces and bounds](rendering.md#render-surfaces-and-bounds) and the
bundled [Camera shader](../../plugins/zerium.builtin/items/camera/render.wesl).
