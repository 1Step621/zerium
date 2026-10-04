# Plugin API v1

A plugin is one directory containing `plugin.json` and WESL modules. Each shader's
`module` is a module ID, resolved to `<module>.wesl` in the plugin root.
Zerium validates the complete bundle before registering it.
Missing shader or generated files, invalid schemas, and shader/API mismatches
are errors.

```text
com.example.plugin/
├── generated/       # generated interfaces and compatibility fingerprint
├── plugin.json
└── shape.wesl
```

Generate the packaged interfaces after changing the shader contract, then
validate the bundle before using it:

```sh
zerium plugin generate path/to/plugin
zerium plugin validate path/to/plugin
```

Both commands default to the current directory when the path is omitted. Include
`generated/` when distributing the plugin; do not edit its files manually.

## Manifest

The root contract is versioned independently from the plugin release:

```json
{
  "$schema": "https://raw.githubusercontent.com/1Step621/zerium/refs/heads/main/plugins/plugin.schema.json",
  "api_version": 1,
  "id": "com.example.plugin",
  "items": [{
    "id": "shape",
    "label": { "ja-JP": "図形", "en-US": "Shape" },
    "category": {
      "id": "example",
      "label": { "ja-JP": "サンプル", "en-US": "Example" }
    },
    "symbol": "■",
    "shader": { "module": "shape" },
    "output_bounds": {
      "min": ["viewport::min::x", "viewport::min::y"],
      "max": ["viewport::max::x", "viewport::max::y"]
    },
    "capabilities": []
  }]
}
```

`api_version` must be `1`. A plugin must define at least one item or effect.
Unknown JSON fields are rejected. The [JSON schema](../plugins/plugin.schema.json)
provides editor completion; host validation also checks IDs, property/default
compatibility, and cross-references. The [bundled plugin](../plugins/zerium.builtin/)
contains complete manifest and shader examples. JSON snippets below describe
individual declarations or fields unless stated otherwise.

All display labels are locale maps keyed by BCP 47 tags, including categories,
properties, tuple scalars, and enum variants. Zerium selects
`ZERIUM_LANGUAGE`, the system locale, or `en-US`, in that order. Label lookup
tries a case-insensitive exact locale, then `en-US`, then the first available
translation ordered by locale key. It does not fall back from a region tag to a
language-only tag. Category IDs are independent of labels; entries from different
plugins with the same category ID are grouped together.

## Item and effect inputs

An item declares its output `shader` at the top level. Its `capabilities` array
contains named inputs to that shader. An effect has the same array, and each
render, compute, or temporal pass can use those inputs. Array order fixes the
GPU binding order; each `id` is unique within its item or effect and becomes
the WESL symbol imported from `package::generated::<entity_id>`.

- `media` decodes a video or image file and exposes its pixels as a texture.
  Video inputs can reference source-clock properties in an optional `playback`
  block, independently for each input on either an item or an effect.
- `text` rasterizes text from referenced properties into a texture.
- `render_result` composites an inclusive range of layers behind the owner.

At most eight capabilities may be declared, with unique WGSL identifier IDs.
Their textures contain scene-linear, premultiplied color. Missing media frames
supply a transparent texture. File values belong to their item or effect properties and are saved with the project.

A media capability declaration:

```json
{
  "type": "media",
  "id": "source",
  "file": "source_file",
  "reader": "zerium.ffmpeg"
}
```

The `file` field references a file property; the capability ID names the shader
texture and may differ from the property ID. Multiple capabilities can consume
the same file property with different readers and playback settings.
Probed metadata is saved separately from property values so the decoded stream
information remains available when reopening a project. Paths and readers are resolved from
the property and capability declarations, rather than duplicated in metadata.

```json
{
  "id": "source_file",
  "label": { "ja-JP": "ソース", "en-US": "Source" },
  "type": {
    "file": {
      "extensions": ["mp4", "mov"]
    }
  },
  "default": { "file": null },
  "configurations": [{ "scene_bindable": false }]
}
```

File properties contain an optional file path. Readers belong to media and audio
inputs; each input keeps its own probed metadata. They require one configuration, an empty default, and no animation or
scene bindings. `editable` and `ui.visible` work like other properties. Files
cannot be tuple coordinates or array elements. Paths are saved relative to the
project file when possible. Files are host resources and do not occupy bytes or
fields in the shader property ABI; capabilities expose their decoded content.

A shared media shader can use `import package::generated::host::entity::{source,
capability_sampler};` and sample `source` with `capability_sampler`.
Every visual media input presents the reader's returned frame resolution to the
shader in scene-linear, premultiplied color. An input that fills a quad can
declare `"placement": {"position":"position", "size":"size"}`.
The host uses that placement and the output resolution to request enough source
pixels; fixed-resolution readers can return a smaller frame. Without placement,
the host requests the render target size. Placement properties must each be a
tuple of two `f32` values.

### Audio and editor roles

`audio` is a top-level item role, separate from shader capabilities.
It is an array of inputs. Each entry declares an `id` and `reader`, and references
properties through `file`, `volume`, `source_start`, `source_duration`,
`playback_speed`, `end_behavior`, and `preserve_pitch`. `file` names a file
property; `volume` names an `f32` linear gain property. Audio inputs read audio
streams independently of visual inputs and each other. A missing audio stream contributes silence. Inputs may
share property references to synchronize settings, or use different properties
to control each stream independently.

Items and effects can declare an `editor` array without adding shader inputs.
Each entry has a `type` and all property references consumed by that feature:

| Type | References | Behavior |
| --- | --- | --- |
| `timeline` | `source_start`, `source_duration`, `playback_speed` | Item trim/stretch; independent of readers and EOF policy |
| `position` | `property` | Preview position handle for a two-`f32` tuple |
| `size` | `property`, `position` | Preview size handles centered at its own declared position |
| `aspect_lock` | `property` | Ratio-lock toggle for a two-`f32` tuple; optional `default` is false |
| `points` | `property`, `position`, `size` | Vertex handles for an array of two-`f32` tuples in its own rectangle |
| `spline` | `points`, `position`, `size`, `tension`, `closed` | Preview curve; tension is `f32`, closed is `bool` |
| `label` | `property` | Item display label from a string property |

Position and size references are two-`f32` tuples. Points are percentages within
the declared rectangle: `[0, 0]` is top-left and `[100, 100]` is bottom-right.
Size, points, and spline editors do not inherit geometry from other entries.
A spline editor displays a curve. Declare a points editor as well to enable
dragging its vertices; each entry uses its own property references.
Each feature can be declared once. Timeline and label editors are supported
only on items. An omitted or empty array adds no editor features. The old object
form is not accepted.

For example:

```json
"editor": [
  { "type": "size", "property": "size", "position": "position" },
  { "type": "aspect_lock", "property": "size" }
]
```

This enables size handles and an initially unlocked ratio. Direct edits preserve
the retained ratio when locked; animation and scene bindings remain independent.
The inspector adds a lock toggle to the referenced tuple as an editor extension.
No additional property declaration is needed.

To display a spline curve with editable vertices, declare both features:

```json
"editor": [
  { "type": "spline", "points": "points", "position": "position", "size": "size", "tension": "tension", "closed": "closed" },
  { "type": "points", "property": "points", "position": "position", "size": "size" }
]
```

### Text

A text capability names every property consumed by the host rasterizer:

```json
{
  "type": "text",
  "id": "title",
  "size": "size",
  "text": "text",
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
```

The referenced properties must match the rasterizer's types: `size` is a two-`f32`
tuple, `font_family` is an array of strings, `text` is a string, font size and
outline width are `f32`, colors are `color`, and bold/italic are `bool`.
Alignment properties must be enums containing exactly `0`, `1`, and `2`.

### Render result

A `render_result` capability names two `u32` properties containing offsets
behind the owner's layer. Offset `1` is the layer immediately behind it. The
range is inclusive and may be entered in either order; out-of-range layers
contribute transparency. Both properties must be constrained to `1..=30`.
The referenced bool property `hide_original` determines whether those layers
remain in their scene's normal output. Inside a scene, offsets are local to
that scene.

```json
{
  "type": "render_result",
  "id": "behind",
  "start_offset": "start_offset",
  "end_offset": "end_offset",
  "hide_original": "hide_original"
}
```

## Properties

Every property has one identity and one value. Tuple coordinates can be edited
and animated independently, but are not modeled as separate property lanes.

The property inspector renders only declared properties, in declaration order,
and editor extensions. File selection is the editor for a file property.
For example, the `aspect_lock` editor declares the ratio-lock toggle
attached to its tuple. Playback controls are ordinary properties referenced by
a media input or the audio role. Undeclared metadata rows and hints are not added.
Scene schema authoring has its own pane; scene-instance argument values have a
separate editing dialog.

Each visual media input declares its optional source clock in `playback`.
Audio playback and timeline editing declare their own explicit references in
`audio` entries and `timeline` editor entries. No role falls back to another
role's references.
Referencing the same property IDs synchronizes these roles without duplicating
values or inspector controls; different IDs allow independent clocks. Each
input, including an effect's media input, uses its owner's property values.
Without `playback`, a video starts at source time zero, runs at 1× speed, and
stops at EOF. Images always provide a static texture; playback settings apply only when the
reader returns temporal media. There is no item-level `video` block.

```json
"capabilities": [{
  "type": "media",
  "id": "source",
  "file": "source_file",
  "reader": "zerium.ffmpeg",
  "playback": {
    "source_start": "source_start",
    "source_duration": "source_duration",
    "playback_speed": "rate",
    "end_behavior": "end_behavior"
  }
}],
"audio": [{
  "id": "sound",
  "file": "source_file",
  "reader": "zerium.ffmpeg",
  "volume": "volume",
  "source_start": "source_start",
  "source_duration": "source_duration",
  "playback_speed": "rate",
  "end_behavior": "end_behavior",
  "preserve_pitch": "preserve_pitch"
}],
"editor": [{
  "type": "timeline",
  "source_start": "source_start",
  "source_duration": "source_duration",
  "playback_speed": "rate"
}]
```

| Role key | Property type | Meaning |
| --- | --- | --- |
| `source_start` | `f32` | Source interval start in seconds |
| `source_duration` | `f32` | Source interval length in seconds |
| `playback_speed` | `f32` | Playback multiplier, 0.25–4 |
| `end_behavior` | Enum `[0, 1, 2]` | Stop, loop, hold |
| `preserve_pitch` (audio only) | `bool` | Keep audio pitch when changing speed |

When `playback` is present, all four references are required and distinct.
The same requirement applies to each `audio` entry, which also requires
`preserve_pitch`. Audio input IDs must be unique within an item.
The `timeline` editor requires only the three time mapping references. All referenced
time settings must set `scene_bindable: false` and cannot be animated because
time mappings are evaluated independently of property animation.

The timeline does not restrict an interval to a reader's actual stream length.
Each visual or audio input applies its own EOF policy: stop gives transparent
video or silent audio, loop wraps the stream, and hold gives a held video frame
or silent audio. Changing EOF policy does not modify the interval or clip length.
Without the `timeline` editor, source property edits do not change the timeline
length, and trim/stretch change only placement and animation timing.

Property IDs, labels, units, visibility, constraints, defaults, and numeric
steps use the usual property schema. For example, the property referenced by
`playback_speed` above is:

```json
{
  "id": "rate",
  "label": { "en-US": "Playback speed", "ja-JP": "再生速度" },
  "type": { "value": "f32" },
  "default": { "f32": 1 },
  "configurations": [{
    "scene_bindable": false,
    "constraints": { "min": 0.25, "max": 4 },
    "ui": { "unit": "×", "step": 0.01, "drag_step": 0.01 }
  }]
}
```

`ui.drag_step` optionally sets the numeric change per horizontal pixel; `ui.step` sets the step-button increment. Shift uses one tenth
of the normal adjustment. Dragging quantizes the adjustment relative to the
initial value, and step buttons add or subtract the configured increment;
values between increments retain their offset. Edits to the duration referenced
by the `timeline` editor keep speed fixed and update the timeline duration; edits to
its speed keep the source interval fixed and update the timeline duration.
On the first file import, a timeline mapping is initialized from the longest
temporal stream read from that file, at the current speed. Static streams do not
extend the clip. Further file imports and replacements preserve the edited time
mapping and placement. Without the `timeline` editor, file imports preserve the
existing timeline length. Timeline edits validate the declared property
constraints and clip placement atomically for the selection; there is no
media-length validation or clamping. Trim and stretch write only the three
properties referenced by the `timeline` editor.
The source end is computed as start + duration; there is no separately stored
end property or playback record. API and project format versions remain `1`.

```json
{
  "id": "position",
  "label": { "ja-JP": "位置", "en-US": "Position" },
  "type": {"value": ["f32", "f32"]},
  "default": {"tuple": [{"f32": 0}, {"f32": 0}]},
  "configurations": [
    {
      "animatable": true,
      "constraints": {"min": -1000000, "max": 1000000},
      "ui": {"label": {"ja-JP": "X", "en-US": "X"}, "unit": "px", "step": 1}
    },
    {
      "animatable": true,
      "constraints": {"min": -1000000, "max": 1000000},
      "ui": {"label": {"ja-JP": "Y", "en-US": "Y"}, "unit": "px", "step": 1}
    }
  ]
}
```

Scalar types are `f32`, `i32`, `u32`, `bool`, `color`, `string`, and finite
`enum` contracts. `type` and `default` each use one variant key: a scalar type
`{"value":"f32"}` has default `{"f32":0}`. Tuples contain 2–64 arbitrary
scalars; tuple and array defaults use `tuple` and `array` keys. Tuples cannot
contain tuples or arrays, and arrays cannot contain arrays.

`configurations` is required: one entry for a scalar, one per tuple scalar, or
one per scalar position in an array element. Each entry has these settings:

| Setting | Default | Purpose |
| --- | --- | --- |
| `editable` | `true` | Allow direct edits; false also disables animation editing. |
| `animatable` | `false` | Allow animation of numeric or color scalars. |
| `scene_bindable` | `true` | Allow the scalar to be exposed as a scene argument. |
| `constraints` | Unrestricted | Validate defaults, edits, loaded values, and animation endpoints. |
| `ui` | Scalar defaults | Presentation hints: label, unit, step, visibility, enum labels, multiline, editor. |

Numeric bounds belong in each scalar's `constraints`, not on the tuple.
Numeric units are the same in projects, shaders, and controls. Missing tuple
scalar labels use their one-based index; a tuple is visible if any scalar is
visible. Color is one scalar with a color picker, not four numeric coordinates.

### Arrays

String arrays use text inputs by default. Use `ui.editor: "font_family"` for a
system-font picker when the values represent fallback fonts:

```json
{
  "type": {"array": {"element_type": "string", "max_items": 1024}},
  "default": {"array": []},
  "append_default": {"string": ""},
  "configurations": [{
    "ui": { "editor": "font_family" }
  }]
}
```

Array configurations describe the scalar positions of one element template
and apply to every element. The property-level `default` holds the initial
elements; an empty list means the array starts empty. Each non-empty array
element has a stable positive `id` and a tagged `value`.
An array must declare `append_default` with a tagged element value, without an
`id`. The inspector uses that fixed value when adding an element, regardless of
the existing elements. The value must match the element type and scalar constraints.

Empty and duplicate strings are valid values. Arrays support scene
binding through individual elements, but an array itself is not a scene-argument
value. Plugins receive ordered values without the editor's stable element IDs.

Array types keep `element_type`, `min_items`, and `max_items` inside `array`.
`min_items` defaults to zero; `max_items` is required. For example, this property
starts with three points and appends the origin:

```json
{
  "id": "points",
  "label": {"en-US": "Points"},
  "type": {"array": {"element_type": ["f32", "f32"], "min_items": 3, "max_items": 1024}},
  "default": {"array": [
    {"id": 1, "value": {"tuple": [{"f32": 0}, {"f32": 0}]}},
    {"id": 2, "value": {"tuple": [{"f32": 100}, {"f32": 0}]}},
    {"id": 3, "value": {"tuple": [{"f32": 50}, {"f32": 100}]}}
  ]},
  "append_default": {"tuple": [{"f32": 0}, {"f32": 0}]},
  "configurations": [{}, {}]
}
```

### Enums and animation

Finite choices are declared in the type:

```json
{
  "type": { "value": { "enum": [0, 1] } },
  "default": { "enum": 0 },
  "configurations": [{
    "ui": { "enum_variants": { "0": {"en-US": "Outside"}, "1": {"en-US": "Inside"} } }
  }]
}
```

Each animation track addresses one scalar, including tuple and array-element
scalars. `f32`, `i32`, `u32`, and colors interpolate; bools, strings, enums,
and array structure remain static. Track positions, Bezier handles, easing,
and floating-point interpolation use f32. Colors share one curve across RGBA, and integer
interpolation rounds while retaining integer endpoints.

Enum membership is retained in runtime values and scene bindings. Its GPU
representation is `u32`. `ui.enum_variants` may be omitted to show numeric labels;
when provided it must label every member exactly once.

## Generated WESL API

Shader sources import the host API and the item/effect interface explicitly.
For an item with properties:

```wesl
import package::generated::host::item::{context, quad_corner};
import package::generated::shape::{ZeriumProps, props};
```

The `module` value is a single WGSL identifier. For example, `"module": "shape"`
loads `shape.wesl` as `package::shape`. Other root-level `.wesl` files may be
imported as helper modules; `generated` is reserved for host-provided modules.

`generated/<entity_id>.wesl` contains one visual item's or effect's full typed
property API and capability declarations. Every effect pass shares that file,
regardless of shader kind. A shader shared by multiple entities imports
`package::generated::host::entity`, which resolves to the current owner's
interface.

Visual item/effect IDs must be WGSL identifiers and distinct across both kinds.
Host APIs have a separate namespace, so an effect named `compute` imports its
properties from `package::generated::compute` and the compute helpers from
`package::generated::host::compute`. An entity named `host` is also valid.
Modules, fields, and helpers starting with `_` are private implementation details.

Regenerate when entity IDs or kinds, pass shader kinds, property IDs/types/order,
or capability IDs/order change, or when the host API is updated. The compatibility
fingerprint describes this shader contract rather than the raw JSON. Formatting,
labels, defaults, enum choices, and array length limits do not require regeneration.
Generated declarations are packaged files; they are not generated at runtime.
Zerium links WESL to in-memory WGSL when loading and shares it between preview and
export. Plugins distribute WESL and generated interfaces, not WGSL.

A module with properties exposes a typed property struct; a module without
properties does not generate `ZeriumProps` or `props`. All shader kinds use the
same loader name:

```wesl
let properties = props(instance_index); // item shader
let properties = props();               // effect pass
```

Tuple fields are generated structs with fields `v0`, `v1`, and so on. Arrays use
`properties.<id>_len` and `get_<id>(properties, index)`. Strings use `ZeriumStr`:
`value.byte_len` is the UTF-8 byte length, and `str_byte(value, index)` returns a
byte or zero outside the string. Import `str_byte` from the entity interface
alongside `props`. Use these typed accessors; raw buffers and ABI
offsets are private.

Every shader receives `ZeriumContext` through `context`. It contains the
physical surface `output_size`, fixed `composition_size`, logical
`surface_size`, and pixel density `composition_scale`. Item shaders pass their
instance index; effect passes do not. Composition coordinates are centered,
with positive X right and positive Y down.

Procedural and media item shaders also receive quad and coordinate helpers such
as `quad_corner` and `quad_position`. Shared rotation and color helpers live in
`package::generated::host::util`, including `rotate`, `srgb`, and `scene_color`.

## Render surfaces and bounds

An item effect receives the item's image on a surface described by a logical
rectangle and a pixel density. This rectangle can lie outside the viewport.
Each effect produces its own rectangle; the renderer maps its input image into
that rectangle without clipping it to the viewport first. A scene composites
its children into the viewport, then runs scene effects on that viewport-sized
image. The scene boundary is therefore the point where offscreen content is
clipped.

Every item and effect declares `output_bounds` with four
expressions. `min` and `max` contain X and Y edges in composition pixels.
Expressions run on the CPU for each evaluated frame, before the output texture
is allocated. An item starts with the viewport as its `input` rectangle; an
effect starts with its incoming image rectangle. Scene effects always render
inside the viewport, regardless of their declared output rectangle.

Expressions can use arithmetic, parentheses, and functions such as `min`,
`max`, `math::abs`, `math::sin`, and `math::cos`. For each `input` and `viewport`
rectangle, `{prefix}::{min,max,center,size}::{x,y}` variables are available
(e.g. `input::min::x`). An `f32` property `radius` is `p::radius`;
an `f32` pair `position` exposes `p::position::v0` and `p::position::v1`.
Three- and four-component `f32` tuples also expose `::v2` and `::v3`.
The declaration must include all four edges:

```json
{
  "output_bounds": {
    "min": ["input::min::x - p::radius", "input::min::y - p::radius"],
    "max": ["input::max::x + p::radius", "input::max::y + p::radius"]
  }
}
```

For a full-frame item, use `viewport::*` edges as in the manifest example. To
preserve an effect's input rectangle, use `input::*`. Expressions may chain
`;`-separated assignments to reuse local calculations. The bundled plugin
contains rotated and projected bounds examples.

Invalid, reversed, or non-finite results fail the render rather than producing
an unbounded texture allocation. Media `placement` independently controls the
reader's requested raster resolution. The visual shader remains responsible
for drawing pixels inside the declared rectangle.

`input_space` separately controls which rectangle an effect shader receives in
`effect_input`. With `"output"`, the renderer first composites the input into
the output rectangle, so the shader can sample it with output UVs. With
`"source"`, the input keeps its own rectangle; shaders can use
`uv_to_position` and `position_to_uv` to map between them. The default is
`"output"`; source-space effects must have exactly one render pass.

For spatial effects, `uv_to_position` converts an output UV to a composition
position, while `position_to_uv` converts a composition position to the input
texture's UV. `input_uv_to_position` converts an input UV to a composition
position. A `render_result` capability is scene-sized; use `viewport_uv`
to sample it from an item-local effect pass.

## Effects and passes

An effect is always an ordered, non-empty list of explicit passes. There is no
top-level shader and no implicit render pass.

```json
{
  "id": "blur",
  "label": {"en-US": "Blur"},
  "category": {"id": "blur", "label": {"en-US": "Blur"}},
  "output_bounds": {
    "min": ["input::min::x - p::radius * 4", "input::min::y - p::radius * 4"],
    "max": ["input::max::x + p::radius * 4", "input::max::y + p::radius * 4"]
  },
  "properties": [{
    "id": "radius",
    "label": {"en-US": "Radius"},
    "type": { "value": "f32" },
    "default": { "f32": 8 },
    "configurations": [{
      "animatable": true
    }]
  }],
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
```

Pass constants use `f32`, `i32`, `u32`, or `bool` values and are injected through
WESL's `constants` virtual module. Import them explicitly in the shader, for example
`import constants::{direction_x, direction_y};`. They are compiled separately for
each pass and never enter the property buffer or inspector state. Compute
workgroup size is read from the shader's `@workgroup_size`; the manifest only controls
dispatch dimensions.

Effect shaders process the output surface declared by `output_bounds`. Shader
coordinates and `context().output_size` refer to that surface for item effects,
and to the viewport for scene effects. The renderer does not scan input alpha
to determine effect bounds.

Render and compute passes receive `effect_input`, the current pipeline
input, and `effect_source`, the image captured at the start of the current
regular pass chain. They also receive `effect_sampler`. Capability inputs are
imported by ID from `package::generated::<entity_id>` (or the `host::entity` alias) with
`capability_sampler`. Modules without capability inputs do not generate a
capability sampler. Compute passes write with `store(position, color)`.

A temporal pass must be first and may occur at most once. Its `sampling`
declaration maps public properties to host-controlled subframe sampling, while
its reducer owns the weighting algorithm:

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

`range` samples evenly between start and end offsets, measured in frames.
The sample-count property must have explicit constraints within `1..=32`.
Alternatively, `{"type":"offsets","offsets":"sample_offsets"}` reads an
array of 1–32 explicit frame offsets from a property. Negative offsets sample
the past, positive offsets the future, and zero samples the current frame.

Reducer WESL receives `temporal_sample`,
`temporal_accumulation`, `temporal_sampler`, and
`info() -> ZeriumTemporalInfo`. The public info contains `sample_index`,
`sample_count`, `frame_offset` (in frames), and `sample_progress` (0–1).
Surface dimensions and scale come from `context()`. Later render or compute
passes consume the reduced texture normally.

Render entry points default to `vertex_main` and `fragment_main`; compute
defaults to `compute_main`.
