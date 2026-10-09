# Plugin API v1

A plugin is a directory containing `plugin.json`, WESL modules, and generated
interfaces:

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

Both commands default to the current directory. Distribute `generated/` with
the plugin and do not edit its files manually. See the
[bundled plugin](../plugins/zerium.builtin/) for complete examples and the
[JSON schema](../plugins/plugin.schema.json) for all declaration fields.

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
Unknown fields, invalid defaults, unresolved references, and shader mismatches
are rejected. The examples below describe individual declarations unless stated
otherwise.

Display labels are locale maps keyed by BCP 47 tags. Zerium selects
`ZERIUM_LANGUAGE`, the system locale, or `en-US`, in that order. Lookup tries a
case-insensitive exact match, `en-US`, then the first translation ordered by
locale key; region tags do not fall back to language-only tags.

Items select `shape`, `text`, `media`, `composite`, or `other`. Effects select
`transform`, `color`, `style`, `filter`, `composite`, or `other`.
These are separate fixed lists owned by Zerium. Category labels and translations
come from Zerium; plugins cannot define new categories or override their labels.
Entries in the same category are grouped together across plugins.

## Properties

The inspector displays declared properties in declaration order, with extensions
from `editor`. Capabilities reference these properties rather than adding their
own controls.

Scalar types are `f32`, `i32`, `u32`, `bool`, `color`, `string`, `file`, and finite
`enum` contracts. Each scalar declares its type, plain default value, and optional
editing settings together: `"type": "f32", "default": 0`. Tuples contain 2–64
scalar declarations; arrays contain scalars or tuples. Nested tuples and nested
arrays are unsupported. Project values retain type tags and stable array-element
IDs; these are not part of plugin authoring.

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

Element defaults and array defaults must satisfy their types and constraints.
The host assigns initial element IDs in declaration order, starting at one, and
preserves IDs when editing and saving projects. Elements can be bound to scene
arguments individually; a whole array cannot. Shaders receive ordered values
without element IDs.

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

Values must be unique. Each label is optional and falls back to its numeric
value; displayed choices must remain distinct. Labels affect presentation, not
type compatibility. Enums are represented as `u32` in shaders.

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

A file value is an optional path, saved relative to the project when possible.
`ui.extensions` suggests file-selection and import extensions without restricting
values or scene bindings. Paths can be unset or unavailable; assigning a path
does not require successful decoding.

Files can appear in tuples and arrays with per-scalar configuration. Media and
audio inputs reference standalone file properties; file tuples and arrays do not
automatically create inputs. Readers are declared on the inputs.

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
  "reader": "zerium.ffmpeg",
  "placement": { "position": "position", "size": "size", "origin": "origin" },
  "playback": {
    "source_start": "source_start",
    "source_duration": "source_duration",
    "playback_speed": "rate",
    "end_behavior": "end_behavior"
  }
}
```

`file` references a file property; `reader` selects the decoder. Multiple inputs
can share the file with different readers or playback settings. Missing frames
produce a transparent texture.

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
    "reader": "zerium.ffmpeg",
    "volume": "volume",
    "preserve_pitch": "preserve_pitch",
    "playback": {
      "source_start": "source_start",
      "source_duration": "source_duration",
      "playback_speed": "rate",
      "end_behavior": "end_behavior"
    }
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

Visual and audio inputs use the same `playback` block with the first four
references. Audio additionally requires `preserve_pitch` outside that block.
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

Formatting happens after animation evaluation and includes trailing zeros:
`12.3` with two places displays as `12.30`. It does not change the stored value
or its animation. See the bundled [Number declaration](../plugins/zerium.builtin/plugin.json)
and [shader](../plugins/zerium.builtin/number.wesl).

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
"view": { "position": "position", "zoom": "zoom", "angle": "angle" }
```

`position` references an f32 pair in scene pixels, with `(0, 0)` at the scene
center. `zoom` references an f32 percentage with a positive minimum (`100` is
unchanged). `angle` references an f32 clockwise camera angle in degrees. Moving
the camera right moves the captured image left; rotating the camera clockwise
rotates the image counterclockwise. These properties use ordinary animation.

Each captured item's own bounds are rendered before the view is applied, so
objects outside the scene viewport can enter the shot. A captured scene or
render-result output is already clipped to its own viewport. Nested captures
therefore transform the completed inner image. The bundled Camera item uses
this view with `hide_original` enabled by default and offsets `1..30`.

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
the declared rectangle: `[0, 0]` is top-left and `[100, 100]` is bottom-right.
Spline displays the curve; add a points feature to edit its vertices:

```json
{
  "editor": [
    { "type": "spline", "points": "points", "position": "position", "size": "size", "tension": "tension", "closed": "closed" },
    { "type": "points", "property": "points", "position": "position", "size": "size" }
  ]
}
```

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

A timeline feature uses the three [time-mapping references](#playback-and-audio):

```json
{
  "editor": [{
    "type": "timeline",
    "source_start": "source_start",
    "source_duration": "source_duration",
    "playback_speed": "rate"
  }]
}
```

It derives clip duration from source duration and speed. Changing duration keeps
speed fixed; changing speed keeps the source interval fixed. Trim/stretch update
these three properties and validate their constraints and clip placement.
Without this feature, source-property edits do not change clip length, and
timeline trim/stretch affect placement and animation timing only.

## Generated WESL API

Shader sources import the host and entity interfaces explicitly:

```wesl
import package::generated::host::item::{context, quad_corner};
import package::generated::shape::{ZeriumProps, props};
```

Host modules are `item`, `effect`, `compute`, `temporal`, and `util` under
`package::generated::host`.

A shader `module` is a WGSL identifier resolved to `<module>.wesl` in the plugin
root. Other root-level WESL files can be imported as helpers; `generated` is
reserved for the host interface.

`generated/<entity_id>.wesl` contains an entity's typed properties and texture
inputs and is shared by all its effect passes. Shared shaders can import
`package::generated::host::entity` to use the current owner's interface.
Visual item and effect IDs must be WGSL identifiers and distinct across both
kinds. Names beginning with `_` are private.

Regenerate when entity IDs, property IDs/types/order, input
IDs/order, or the host API change. Labels, defaults, enum choices, and array
length limits do not require regeneration. Distribute WESL and generated
interfaces; the host links them to WGSL when loading.

A shader with no shader-visible properties has no `ZeriumProps` or `props`.
Property loaders use these signatures:

```wesl
let properties = props(instance_index); // item shader
let properties = props();               // effect pass
```

Tuple fields are structs with `v0`, `v1`, and subsequent scalar fields. Arrays
use `properties.<id>_len` and `get_<id>(properties, index)`. Strings use `ZeriumStr`:
`byte_len` gives UTF-8 length and `str_byte(value, index)` gives a byte or zero
outside the string. Import these accessors from the entity interface.

Files have no shader fields. Mixed tuples retain their other scalars' original
`vN` indices; properties containing only files are omitted. Capabilities expose
the decoded file content as textures instead.

`context` provides `ZeriumContext`: physical `output_size`, fixed
`composition_size`, logical `surface_size`, and pixel density `composition_scale`.
Item shaders pass an instance index; effect passes do not. Composition
coordinates are centered, with positive X right and positive Y down.

Item helpers include `quad_corner` and `quad_position`. Rotation and color
helpers, including `rotate`, `srgb`, and `scene_color`, live in
`package::generated::host::util`. Media and text textures are sampled with
`capability_sampler` from the entity interface.

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
Numeric properties use `p::<id>`; tuple components use `p::<id>::v0`, `::v1`,
`::v2`, and `::v3` for two-, three-, and four-component numeric tuples.

### Placement and projection

`placement_center(position, size, origin)` converts an origin coordinate to its
center on one axis, matching WESL's `item::placement_center`. Use the unpadded
local size and an origin of `0` (start), `1` (center), or `2` (end):

```text
placement_center(p::position::v0, p::size::v0, p::origin::v0) - math::abs(p::size::v0) / 2
```

Use `v1` for the vertical axis. Arguments may be integers or floating-point
numbers; origin must be an integer in the declared range.

For perspective bounds, `projected_rect_min_x`, `projected_rect_min_y`,
`projected_rect_max_x`, and `projected_rect_max_y` return individual rectangle
edges using the same six arguments:

```text
projected_rect_min_x(
  (input::min::x, input::min::y),
  (input::max::x, input::max::y),
  (p::rotation::v0, p::rotation::v1, p::rotation::v2),
  (p::center::v0, p::center::v1),
  p::perspective,
  0.1
)
```

Arguments are minimum corner, maximum corner, XYZ rotation in degrees, rotation
center, focal length, and near clip ratio. Rotation applies in X/Y/Z order to a
flat source at z=0; near depth is focal length × near clip ratio. Bounds are
clipped before perspective division; an entirely clipped rectangle returns the
rotation center. Inputs must be finite, corners ordered, and focal length and
near depth positive. The shader must use the same projection and near depth.
See the bundled `rotate_3d` effect for a matching implementation.

### Effect input coordinates

`render.input_space` defaults to `"output"`: the input is composited into the output
rectangle and can be sampled with output UVs. With `"source"`, the input keeps
its own rectangle; this requires exactly one render pass.

`uv_to_position` maps output UVs to composition positions; `position_to_uv` maps
composition positions to input UVs. `input_uv_to_position` maps input UVs to
composition positions. A `render_result` texture is scene-sized; sample it with
`viewport_uv` when used in an item-local effect.

## Effects and passes

Effects declare an ordered, non-empty `render.passes` array:

```json
{
  "render": {
    "passes": [{
      "type": "compute",
      "shader": { "module": "blur" },
      "dispatch": ["width", "height", "one"],
      "constants": [
        { "id": "direction_x", "value": { "f32": 1 } },
        { "id": "direction_y", "value": { "f32": 0 } }
      ]
    }]
  }
}
```

Render entry points default to `vertex_main` and `fragment_main`; compute defaults
to `compute_main`. Compute workgroup size comes from `@workgroup_size`; the
manifest controls dispatch dimensions. Pass constants accept `f32`, `i32`, `u32`,
or `bool` and are imported from WESL's virtual `constants` module, for example
`import constants::{direction_x, direction_y};`.

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
