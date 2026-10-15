# Plugin API v1

A plugin is a directory containing `plugin.json`, WESL modules, and generated
interfaces. For example:

```text
com.example.plugin/
├── generated/
├── plugin.json
└── shape.wesl
```

Generate the interfaces after changing the shader contract, then validate the
complete bundle:

```sh
zerium plugin generate path/to/plugin
zerium plugin validate path/to/plugin
```

Both commands default to the current directory. Distribute `generated/` with the
plugin; do not edit generated files manually. The [JSON schema](../plugins/plugin.schema.json)
is the field reference, and the [bundled plugin](../plugins/zerium.builtin/)
provides working examples.

## Manifest

```json
{
  "$schema": "https://raw.githubusercontent.com/1Step621/zerium/refs/heads/main/plugins/plugin.schema.json",
  "api_version": 1,
  "id": "com.example.plugin",
  "items": [{
    "id": "shape",
    "label": { "ja-JP": "図形", "en-US": "Shape" },
    "category": "shape",
    "symbol": "■",
    "render": {
      "shader": { "module": "shape" },
      "bounds": {
        "min": ["viewport::min::x", "viewport::min::y"],
        "max": ["viewport::max::x", "viewport::max::y"]
      }
    }
  }]
}
```

`api_version` must be `1`, and a plugin must define at least one item or effect.
Validation rejects unknown fields, invalid defaults, unresolved references, and
shader mismatches. Later examples are declaration fragments.

Display labels are locale maps keyed by BCP 47 tags. Zerium selects
the `language` override in `config.json`, the system locale, or `en-US`, in that
order. Lookup tries a case-insensitive exact match, `en-US`, then the first
translation ordered by locale key; region tags do not fall back to language-only
tags.

Items select `shape`, `text`, `media`, `composite`, or `other`. Effects select
`transform`, `color`, `style`, `filter`, `composite`, or `other`.
Categories are host-defined and group entries across plugins.

## Properties

The inspector displays properties in declaration order. Inputs and editor features
reference these declarations.

Scalar types are `f32`, `i32`, `u32`, `bool`, `color`, `string`, `file`, and finite
`enum` contracts. Each scalar declares its type, plain default value, and optional
editing settings together: `"type": "f32", "default": 0`. Tuples contain 2–64
scalar declarations; arrays contain scalars or tuples. Nested tuples and nested
arrays are unsupported.

For example, a position property declares two independently editable scalars:

```json
{
  "id": "position",
  "label": { "en-US": "Position" },
  "type": "tuple",
  "elements": [
    { "type": "f32", "default": 0, "animatable": true, "ui": { "label": { "en-US": "X" }, "unit": "px", "step": 1 } },
    { "type": "f32", "default": 0, "animatable": true, "ui": { "label": { "en-US": "Y" }, "unit": "px", "step": 1 } }
  ]
}
```

Settings belong on each scalar declaration, including the scalars inside tuple
and array element declarations.

| Setting | Default | Purpose |
| --- | --- | --- |
| `editable` | `true` | Allow direct edits and animation editing |
| `animatable` | `false` | Allow numeric or color animation |
| `scene_bindable` | `true` | Allow exposure as a scene argument |
| `constraints` | Unrestricted | Validate defaults, edits, loaded values, and animation endpoints |
| `ui` | Scalar defaults | Labels, units, visibility, numeric steps, and editor hints |

Numeric bounds belong in each scalar's `constraints`. Units are the same in
projects, shaders, and controls. `ui.step` sets the step-button increment;
`ui.drag_step` optionally sets the change per horizontal pixel. Missing tuple
scalar labels use their one-based index. A tuple is visible if any scalar is
visible; color is a single scalar.

### Arrays

Array declarations require `element`, `max_items`, and a plain `default` array;
`min_items` defaults to zero. The element declaration provides the default and
settings for newly added elements:

```json
{
  "id": "points",
  "label": { "en-US": "Points" },
  "type": "array",
  "element": {
    "type": "tuple",
    "elements": [
      { "type": "f32", "default": 0 },
      { "type": "f32", "default": 0 }
    ]
  },
  "min_items": 3,
  "max_items": 1024,
  "default": [[0, 0], [100, 0], [50, 100]]
}
```

Element and array defaults must satisfy their types and constraints. Elements
can be bound to scene arguments individually; a whole array cannot. Shaders
receive values in array order.

String arrays use text inputs. Set `ui.editor: "font_family"` for a system-font
picker when the array represents fallback fonts.

### Enums and animation

Finite choices are declared in the type:

```json
{
  "id": "mode",
  "label": { "en-US": "Mode" },
  "type": "enum",
  "default": 0,
  "variants": [
    { "value": 0, "label": { "en-US": "Outside" } },
    { "value": 1, "label": { "en-US": "Inside" } }
  ]
}
```

Values and displayed choices must be unique. Missing labels use the numeric value.
Enums are represented as `u32` in shaders.

Animation tracks address individual scalars, including tuple and array-element
scalars. `f32`, `i32`, `u32`, and colors interpolate; other scalar types and array
structure remain static. Colors share one curve across RGBA; integer
interpolation rounds to integer values.

### Files

```json
{
  "id": "source_file",
  "label": { "en-US": "Source" },
  "type": "file",
  "default": null,
  "ui": { "extensions": ["mp4", "mov"] }
}
```

Files are optional paths, saved relative to the project when possible.
`ui.extensions` suggests selection and import extensions without restricting values.
Unset or unavailable files are allowed.

Files can appear in tuples and arrays, but media and audio inputs must reference
standalone file properties. Declare the reader on each input.

## Generated WESL API

Shader sources import the host and entity interfaces explicitly:

```wesl
import package::generated::host::item::{context, quad_corner};
import package::generated::shape::{ZeriumProps, props};
```

Host modules are `item`, `effect`, `compute`, `temporal`, and `util` under
`package::generated::host`.

Shader modules use relative WESL paths: `filters::blur` resolves to
`filters/blur.wesl`, and a helper at `math/color.wesl` is imported as
`package::math::color`. Authors choose the directory layout; the top-level
`generated` namespace is reserved for the host.

Generated interfaces are ordinary WESL files with explicit imports:

| Consumer | Generated file | Import module |
| --- | --- | --- |
| Item or render pass | `generated/<entity_id>.wesl` | `package::generated::<entity_id>` |
| Compute pass | `generated/<entity_id>/compute.wesl` | `package::generated::<entity_id>::compute` |
| Temporal reducer | `generated/<entity_id>/temporal.wesl` | `package::generated::<entity_id>::temporal` |

Each interface contains the owner's typed properties and texture inputs. Only
interfaces needed by the declared pass kinds are generated. For example:

```wesl
import package::generated::gaussian_blur::compute::props;
```

Entry modules import their owner's interface explicitly; shared helpers receive
values or textures as arguments. Visual item and effect IDs must be WGSL
identifiers and distinct across both kinds. Host names beginning with `_` are private.

Regenerate after changing entity IDs, property IDs/types/order, input IDs/order,
pass kinds, or the host API. Labels, defaults, enum choices, and array length
limits do not require regeneration.

A shader with no shader-visible properties has no `ZeriumProps` or `props`.
Property loaders use these signatures:

```wesl
let properties = props(instance_index); // item shader
let properties = props();               // effect pass
```

Tuple fields are structs with `v0`, `v1`, and subsequent scalar fields. Arrays
use `properties.<id>_len` and `get_<id>(properties, index)`. Import these accessors
from the entity interface.

Strings and files are CPU-only values and have no shader fields. Mixed tuples
retain their other scalars' original `vN` indices; properties and arrays containing
only CPU values are omitted. Text and media capabilities expose rendered text
and decoded file content as textures instead.

`context` provides `ZeriumContext`: physical `output_size`, fixed
`composition_size`, logical `surface_size`, and pixel density `composition_scale`.
Item shaders pass an instance index; effect passes do not. Composition
coordinates are centered, with positive X right and positive Y down.

Item helpers include `placement_center`, `quad_corner`, and `quad_position`.
Rotation and color helpers (`rotate`, `srgb`, `scene_color`) live in
`package::generated::host::util`. Sample input textures with `capability_sampler`
from the entity interface.

## Render surfaces and bounds

Visual items and effects group drawing declarations in `render`: `bounds`,
`inputs`, and either an item `shader` or effect `passes`. Items may omit `render`
when they only provide audio or editor behavior. Item `vertex_count` defaults to
six; effect `scale` defaults to one.

`render.bounds` is a rectangle in composition pixels. The host evaluates its
four edges before allocating the surface. Item effects
can preserve content outside the viewport; a scene composites its children into
the viewport before applying scene effects, clipping offscreen content there.
Scene effects render inside the viewport regardless of their declared bounds.

```json
{
  "render": {
    "bounds": {
      "min": ["input::min::x - p::radius", "input::min::y - p::radius"],
      "max": ["input::max::x + p::radius", "input::max::y + p::radius"]
    }
  }
}
```

For an item, `input` starts as the viewport; for an effect, it is the incoming
image rectangle. Use `viewport::*` edges for a full-frame item and `input::*` to
preserve an effect's input. Invalid, reversed, or non-finite results fail rendering.
The shader is responsible for drawing within its declared bounds.

Expressions support arithmetic, parentheses, `min`, `max`, and functions such
as `math::abs`, `math::sin`, and `math::cos`. Local assignments can be separated
with `;`. Rectangle variables use `{input,viewport}::{min,max,center,size}::{x,y}`.
Numeric and enum properties use `p::<id>`; tuple scalars use `p::<id>::v0`,
`::v1`, and subsequent zero-based component indices.

### Placement and projection

`placement_center(position, size, origin)` converts an origin coordinate to its
center on one axis, matching WESL's `item::placement_center`. Use the unpadded
local size and an origin of `0` (start), `1` (center), or `2` (end):

```text
placement_center(p::position::v0, p::size::v0, p::origin::v0) - math::abs(p::size::v0) / 2
```

Use `v1` for the vertical axis. Arguments may be integers or floating-point
numbers; origin must be an integer in the declared range.

Perspective bounds use `projected_rect_min_x`, `projected_rect_min_y`,
`projected_rect_max_x`, and `projected_rect_max_y`. Each takes the minimum corner,
maximum corner, XYZ rotation in degrees, rotation center, focal length, and near
clip ratio. Match the shader's projection and near depth; see the bundled
[rotation declaration](../plugins/zerium.builtin/plugin.json) and
[3D rotation shader](../plugins/zerium.builtin/effects/rotate_3d/render.wesl).

### Effect input coordinates

`render.input_space` defaults to `"output"`: the input is composited into the output
rectangle and can be sampled with output UVs. With `"source"`, the input keeps
its own rectangle; this requires exactly one render pass.

`uv_to_position` maps output UVs to composition positions; `position_to_uv` maps
composition positions to input UVs. `input_uv_to_position` maps input UVs to
composition positions. A `render_result` texture is scene-sized; sample it with
`viewport_uv` when used in an item-local effect.

## Visual and audio inputs

Items and effects declare visual inputs in `render.inputs`. Array order fixes GPU
binding order; IDs must be unique WGSL identifiers. At most eight inputs are
allowed, and `capability_sampler` is reserved. Input textures contain scene-linear,
premultiplied color and are imported by ID from the generated entity interface.

### Media

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
origin uses the [origin contract](#editor-features). The host uses placement to
request sufficient source resolution. Without placement it requests the render
target size. The shader receives the resolution returned by the reader.

`playback` is optional and applies to temporal media. Without it, video starts at
zero, runs at 1× speed, and stops at EOF. Images provide a static texture.

### Playback and audio

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

### Text

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

### Number

A `number` capability uses the same `style` and rasterizer as [text](#text),
with `value` and `decimal_places` references instead of `text`.

| Reference | Property type | Meaning |
| --- | --- | --- |
| `value` | `f32`, `i32`, or `u32` | Number to display |
| `decimal_places` | `u32` constrained to `0..=10` | Fixed decimal precision |

Formatting follows animation evaluation: `12.3` with two places displays as `12.30`.
See the bundled [Number shader](../plugins/zerium.builtin/items/number/render.wesl).

### Render result

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

An optional `view` maps the capture through a camera before it is clipped to the
scene viewport:

```json
{ "view": { "position": "position", "zoom": "zoom", "angle": "angle" } }
```

`position` is an `f32` pair in scene pixels, `zoom` an `f32` percentage with a
positive minimum, and `angle` an `f32` clockwise camera angle in degrees. These
properties support animation. A view can bring offscreen items into the capture;
nested scenes and captures are clipped to their own viewports first. See the
bundled [Camera shader](../plugins/zerium.builtin/items/camera/render.wesl).

## Effects and passes

Effects declare an ordered, non-empty `render.passes` array:

```json
{
  "render": {
    "passes": [{
      "type": "compute",
      "shader": { "module": "blur" },
      "dispatch": ["width", "height", "one"],
      "overrides": { "direction_x": 1, "direction_y": 0 }
    }]
  }
}
```

Render entry points default to `vertex_main` and `fragment_main`; compute defaults
to `compute_main`. Compute workgroup size comes from `@workgroup_size`; the
manifest controls dispatch dimensions. Declare pipeline-overridable values with
standard WGSL `override` declarations in the entry module:

```wesl
override direction_x: f32 = 1.0;
override direction_y: f32 = 0.0;
```

The optional `overrides` object sets declarations by name or by the decimal
string of an explicit `@id`. Values convert to the declaration's scalar type;
booleans use `0` or `1`. Omitted overrides use shader defaults. Dispatch dimensions
are `width`, `height`, `max_dimension`, or `one`; the host rounds each extent up
to a whole number of specialized workgroups.

Render and compute passes receive `effect_input` (current input), `effect_source`
(the image at the start of the regular pass chain), and `effect_sampler`. Compute
passes write with `store(position, color)`. Texture inputs use the generated
entity interface and `capability_sampler`, which is generated only when needed.

### Temporal passes

A temporal pass must be first and can occur only once:

```json
{
  "type": "temporal",
  "sampling": {
    "type": "range",
    "sample_count": "samples",
    "start_offset": "start_offset",
    "end_offset": "end_offset"
  },
  "reducer": { "module": "motion_blur_accumulate" }
}
```

`range` samples evenly between `f32` start and end offsets measured in frames.
The `u32` sample-count property must have constraints within `1..=32`.
Alternatively, `{"type":"offsets","offsets":"sample_offsets"}` reads an array
of 1–32 `f32` frame offsets. Negative offsets sample the past, positive offsets
the future, and zero the current frame.

Reducers receive `temporal_sample`, `temporal_accumulation`, `temporal_sampler`,
and `info() -> ZeriumTemporalInfo`. Info contains `sample_index`, `sample_count`,
`frame_offset`, and `sample_progress` (0–1); surface dimensions come from `context()`.
Later render or compute passes consume the reduced texture.

## Editor features

Items and effects can declare an `editor` array. Each feature supplies its own
property references and can be declared once; timeline and label features are
item-only.

| Type | References | Behavior |
| --- | --- | --- |
| `timeline` | `source_start`, `source_duration`, `playback_speed` | Source trim/stretch |
| `position` | `property` | Preview position handle for a two-`f32` tuple |
| `size` | `property`, `position` | Preview size handles |
| `aspect_lock` | `property` | Ratio lock for a two-`f32` tuple; optional `default` is false |
| `points` | `property`, `position`, `size` | Vertex handles for an array of two-`f32` tuples |
| `spline` | `points`, `position`, `size`, `tension`, `closed` | Preview curve; tension is `f32`, closed is `bool` |
| `label` | `property` | Item label from a string property |

Position and size references are two-`f32` tuples. Points are percentages within
the rectangle: `[0, 0]` is top-left and `[100, 100]` is bottom-right. Combine
`spline` with `points` to display a curve and edit its vertices.

`aspect_lock` adds a toggle to the referenced tuple without requiring another
property. Locked direct edits retain the ratio; animation and bindings remain
independent.

Size, points, spline, and media placement can reference an optional `origin`:
a tuple of two enums containing exactly `0`, `1`, and `2`. The axes select
left/center/right and top/center/bottom; omission means center. Position is the
chosen origin's coordinate. Changing origin retains position, and resizing keeps
that coordinate fixed.

Use `item::placement_center(position, size, origin)` in the shader to compute the
quad center from the unpadded size and a `vec2<u32>` origin. Bounds must apply the
same offset; see [Render surfaces and bounds](#render-surfaces-and-bounds).

A `timeline` feature uses the three [time-mapping references](#playback-and-audio)
to derive clip duration from source duration and speed. Changing duration keeps
speed fixed; changing speed keeps the source interval fixed. Trim/stretch update
these three properties and validate their constraints and clip placement.
Without this feature, source-property edits do not change clip length, and
timeline trim/stretch affect placement and animation timing only.
