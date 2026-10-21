# Visual and audio inputs

[Plugin API v1](README.md) · [Properties](properties.md) · [Rendering](rendering.md) · **Inputs**

Items and effects declare up to eight visual inputs in `render.inputs`. Order
fixes GPU binding order; IDs are unique WGSL identifiers. Inputs use scene-linear,
premultiplied color through generated [sampling functions](rendering.md#image-sampling).

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

`file` references a standalone file property. Built-in readers are `zerium.video`,
`zerium.image` (raster images), `zerium.svg`, and `zerium.audio` (including audio in
video files). Multiple inputs can share a file with different readers or playback
settings. Missing visual frames produce transparency.

Optional `placement` uses position and size pairs plus the
[origin contract](properties.md#editor-features). It guides source resolution;
without it, the host requests the render target size. The reader determines the
returned resolution. Optional `playback` affects temporal media; without it,
video starts at zero at 1× and stops at EOF. Images stay static.

## Playback and audio

Items declare audio separately in their top-level `audio` array:

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

Audio IDs are unique within the item. `volume` references an `f32` linear gain;
a missing stream contributes silence. Gain sampling is described under
[Audio](../architecture/media.md#audio).

Audio accepts the same optional `playback` block as visual media, with the
required `preserve_pitch` reference outside it. Visual playback, audio, and
timeline editing use explicit references; share property IDs to synchronize them.

| Reference | Property type | Meaning |
| --- | --- | --- |
| `source_start` | `f32` | Nonnegative source start, seconds |
| `source_duration` | `f32` | Positive source duration, seconds |
| `playback_speed` | `f32` | Multiplier, 0.25–4 |
| `end_behavior` | Enum `[0, 1, 2]` | Stop, loop, hold |
| `preserve_pitch` | `bool` | Keep audio pitch while changing speed |

The three time-mapping references must be distinct. All properties in this table
must disable animation and set `scene_bindable: false`.

Playback reads from `source_start` to `source_start + source_duration` at the
specified speed. Intervals may exceed the stream. At EOF, stop produces
transparent video/silent audio; loop wraps; hold retains the last video frame
and produces silent audio. EOF behavior does not change clip length.

## Text

```json
{
  "type": "text",
  "id": "title",
  "text": "text",
  "style": {
    "size": "size",
    "font_family": "font_family",
    "font_size": "font_size",
    "letter_spacing": "letter_spacing",
    "line_spacing": "line_spacing",
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

`text` is a string. Style references use:

| References | Type |
| --- | --- |
| `size` | Two-`f32` tuple |
| `font_family` | String array |
| `font_size`, `outline_width`, `letter_spacing`, `line_spacing` | `f32`, composition pixels |
| `color`, `outline_color` | `color` |
| `bold`, `italic` | `bool` |
| `horizontal_alignment`, `vertical_alignment` | Enum exactly `[0, 1, 2]` |

Alignment means left/center/right or top/center/bottom; vertical alignment uses
line boxes. Letter spacing adjusts glyph advances. Line spacing adds to the
default line height of 1.2 times font size, with a minimum of one render pixel.
Negative spacing tightens layout; zero preserves defaults.

## Number

A `number` input shares text's `style`, replacing `text` with `value` (`f32`,
`i32`, or `u32`) and `decimal_places` (`u32` constrained to `0..=10`). Formatting
follows evaluation: `12.3` at two places displays `12.30`. See the bundled
[Number shader](../../plugins/zerium.builtin/items/number/render.wesl).

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

Offsets are `u32` properties constrained to `1..=30`, relative to the owning layer
within its scene. `1` is immediately behind it. The inclusive range accepts
either order; out-of-range layers are transparent. The `hide_original` bool
removes captured layers from normal output, without removing other captures'
dependencies.

Without `view`, the input keeps complete composition bounds. An optional camera
clips to the scene viewport:

```json
{ "view": { "position": "position", "zoom": "zoom", "angle": "angle" } }
```

`position` is an `f32` pair in scene pixels, `zoom` an `f32` percentage with a
positive minimum, and `angle` an `f32` clockwise angle in degrees. All support
animation. Views can capture offscreen content in nested scenes and effects;
nested cameras retain their own clips. See [Bounds](rendering.md#render-surfaces-and-bounds)
and the bundled [Camera shader](../../plugins/zerium.builtin/items/camera/render.wesl).
