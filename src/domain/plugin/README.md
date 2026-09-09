# Plugin API v1

A plugin is one directory containing `plugin.json` and every WGSL file that the
manifest references. Zerium validates the complete bundle before registering
it. Missing shader files, unreferenced files supplied to `PluginBundle`, invalid
schemas, and shader/API mismatches are errors.

```text
com.example.plugin/
├── plugin.json
├── shape.wgsl
└── blur.wgsl
```

The root contract is versioned independently from the plugin release:

```json
{
  "$schema": "https://raw.githubusercontent.com/1Step621/zerium/refs/heads/main/plugins/plugin.schema.json",
  "api_version": 1,
  "id": "com.example.plugin",
  "items": [{
    "id": "shape",
    "label": "Shape",
    "category": "Example",
    "symbol": "■",
    "capabilities": {
      "visual": {
        "type": "procedural",
        "shader": { "source": "shape.wgsl" }
      }
    }
  }]
}
```

`api_version` must be `1` during the current development cycle.
Each plugin must define at least one item or effect.
The root envelope is deserialized privately. `ItemSchema` and `EffectSchema`
each validate themselves at their deserialization boundary; the root then
checks the API version and collection-wide uniqueness before returning a
`PluginManifest`. Runtime code therefore never observes a partly validated
manifest or a deserialized top-level schema. Unknown JSON fields are rejected.

The checked-in [schema](../../../plugins/plugin.schema.json)
provides editor completion. Rust validation remains authoritative for rules
that JSON Schema cannot express, such as unique IDs, text-item requirements,
parameter/default compatibility, and cross-references.

## Rust interface boundary

Manifest-backed structs expose behavior and immutable views rather than public
storage fields. Callers use `ItemSchema::files`, `file`, `visual`, `audio`,
`parameters`, and the visual-kind helpers instead of walking the serialized
`capabilities` shape. Effects similarly expose `parameters`, `passes`, and
`render_scale`. Shader source and entry-point fields are read through accessors.
This keeps JSON layout changes inside the plugin domain.

`ParameterSchema` is read-only outside `domain`: UI and rendering code use its
accessors, while timeline code retains the narrower internal access needed to
project plugin parameters into editable scene arguments. Item and effect
schemas implement `PluginCatalogEntry`, giving catalog UIs one shared metadata
interface and one consistent search-term policy.

Shared parameter contracts live in [`../parameter/`](../parameter/), independently
of plugin loading: `types` owns the scalar/tuple/array algebra, `value` owns
checked values and collections, `schema` exposes the read-only model,
`compatibility` and `constraints` enforce contracts, and `ui`/`numeric`
provide editor views. JSON conversion lives in `parameter/wire/`.
The plugin layer retains WGSL identifier and generated-name validation and the
ABI compiler. Parameter errors are converted to plugin errors at this boundary.
Animation eligibility and interpolation are explicit operations in
[`../animation/`](../animation/), rather than methods added to parameter types
from another module.

Public visibility is reserved for plugin loading and immutable schema
inspection. Editor-derived choices, host capabilities, value projection, generated
WGSL interfaces, and other Zerium runtime adapters are crate-private. The
plugin module denies unreachable public items so private implementation helpers
cannot accidentally become part of the Rust API.

## Items

An item composes file, visual, and audio capabilities. The visual kinds describe
what supplies pixels:

- `procedural`: WGSL generates the item directly.
- `media`: WGSL displays one or more decoded video/image inputs.
- `text`: Zerium rasterizes text and supplies it as a generated texture input.

```json
{
  "id": "video",
  "label": "Video",
  "category": "Media",
  "tags": ["movie", "clip"],
  "symbol": "▶",
  "capabilities": {
    "files": [{
      "id": "source",
      "label": "Source",
      "media_type": "video",
      "reader": "zerium.ffmpeg",
      "extensions": ["mp4", "mov"]
    }],
    "visual": {
      "type": "media",
      "shader": { "source": "video.wgsl" }
    },
    "audio": { "inputs": ["source"], "volume": "volume" }
  },
  "parameters": [{
    "id": "volume",
    "label": "Volume",
    "type": "f32",
    "default": 1,
    "constraints": { "min": 0 }
  }]
}
```

Audio explicitly names the file inputs consumed by the host mixer. Each ID must
refer to a video or audio input. Like a temporal pass `sampling` block, the
audio capability also names the `f32` item parameter read as linear gain in
`volume`. A media visual likewise requires at least one
video/image input. Every item whose `capabilities.editor.size` references a
parameter gets the host's aspect-ratio lock control. The lock is editor state, starts
disabled for new items, and is not part of the shader parameter ABI.

A text visual names every item parameter consumed by the host rasterizer:

```json
"visual": {
  "type": "text",
  "shader": { "source": "text.wgsl" },
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

The rasterizer resolves values through these references, so text parameters can
use any IDs as long as each referenced parameter has the expected storage
type. Alignment references must be enums containing exactly `0`, `1`, and `2`.

For each visual file input `<id>`, media WGSL receives
`zerium_media_<id>`, `zerium_media_<id>_size()`, and the shared
`zerium_media_sampler`. Text receives `zerium_media_text`.

## Parameters

Every parameter has one identity and one value. Tuple coordinates can be edited
and animated independently, but are not modeled as separate parameter lanes.

```json
{
  "id": "position",
  "label": "Position",
  "type": {"tuple": ["f32", "f32"]},
  "default": [0, 0],
  "animatable": true,
  "constraints": {"elements": [
    {"min": -1000000, "max": 1000000},
    {"min": -1000000, "max": 1000000}
  ]},
  "ui": {
    "elements": [
      {"label": "X", "unit": "px", "step": 1},
      {"label": "Y", "unit": "px", "step": 1}
    ]
  }
}
```

`label` and numeric `constraints` belong to the parameter contract. `ui` only
contains presentation hints: `elements` (including each element’s `label`), `unit`, `step`,
`visible`, `enum_variants`, `multiline`, and `editor`. Numeric values use the same canonical
unit in projects, shaders, and editor controls.
Constraints are enforced for defaults, direct edits, array elements, loaded
projects, and animation endpoints.

Scalar types are `f32`, `i32`, `u32`, `bool`, `color`, `string`, and finite `enum` contracts. Color is one scalar, edited with a color picker even inside tuples and arrays. Numeric inputs address numeric scalars by their tuple index; RGBA components are not flattened into numeric input indices.
Item-generic editor behaviors reference parameters from the item
capability, for example `"editor": { "size": "size", "label": "text" }`.
The size reference requires a two-`f32` tuple and the label reference requires a
string. This follows the same parameter-ID wiring used by text rasterization,
audio gain, and temporal sampling. Effects may still use presentation hints such
as `multiline`.

An array of strings uses the ordinary array inspector with a text input for each
element by default.
Use the specialized font-family editor when the values represent fallback
fonts:

```json
{
  "type": {"array": {"type": "string", "max_items": 1024}},
  "default": [],
  "ui": { "editor": "font_family" }
}
```

Without `ui.editor`, entries can be added, edited, reordered, and removed as
ordinary strings. Empty and duplicate strings are valid list values. With
`"editor": "font_family"`, the same stored type uses the system-font picker.
Like every other array, it uses the declared `min_items`/`max_items` bounds and
supports scene binding through its individual elements. Arrays are not themselves
scene-argument values, and nested arrays are not part of the type algebra.

Tuples contain 2–64 arbitrary scalars, including colors, strings, and enums. Their structure prevents
nested tuples and arrays:

```json
{ "type": { "tuple": ["f32", "bool", "string", {"enum": [2, 7]}] }, "default": [0, true, "Label", 7] }
```

Array definitions keep `type`, `min_items`, and `max_items` inside the `array`
object. `min_items` defaults to zero; `max_items` is required.
Arrays contain a scalar or tuple value type, so arrays of tuples are valid
without allowing nested arrays:

```json
{
  "type": {"array": {"type": {"tuple": ["f32", "f32"]}, "min_items": 3, "max_items": 1024}},
  "default": [[0, 0], [100, 0], [50, 100]]
}
```

Finite choices are types, not UI options:

```json
{
  "type": { "enum": [0, 1] },
  "default": 0,
  "ui": { "enum_variants": { "0": "Outside", "1": "Inside" } }
}
```

`scene_bindable` defaults to true. Scene arguments themselves are scalars, preserving enum membership;
tuple elements are published and connected independently.

### Tuple metadata and animation

`ui.elements` and `constraints.elements` specify metadata for each tuple scalar,
including inside an array. When present, each list must match the tuple length.
Tuple UI properties (`label`, `unit`, `step`, `visible`,
`enum_variants`, and `multiline`) belong inside `ui.elements`. Parent UI hints
are not inherited. Missing labels use the one-based element index, and omitted
UI metadata uses the scalar defaults. A tuple is visible if any element is visible.
Numeric bounds belong inside `constraints.elements`; tuple-level `min`/`max`
are rejected. Use `{}` for an unconstrained element. Omitting either metadata
object leaves every element at its defaults. These rules also apply to arrays
of tuples. The `font_family` editor remains an array-of-strings setting.
`animatable` and `scene_bindable` remain parameter-level permissions.

```json
{
  "id": "entry",
  "label": "Entry",
  "type": {"tuple": ["f32", {"enum": [2, 7]}, "string", "color"]},
  "default": [1, 2, "Caption", [1, 1, 1, 1]],
  "animatable": true,
  "constraints": {"elements": [{"min": 0, "max": 10}, {}, {}, {"min": 0, "max": 1}]},
  "ui": {
    "elements": [
      {"label": "Width", "unit": "px"},
      {"label": "Mode", "enum_variants": {"2": "Low", "7": "High"}},
      {"label": "Caption", "multiline": true},
      {"label": "Tint"}
    ]
  }
}
```

Every animation track addresses one scalar: a standalone scalar, a tuple element,
or a scalar within an array element. Numeric scalars and colors interpolate;
bools, strings, and enums remain static. A mixed tuple can animate its eligible
elements independently. `animatable: true` requires at least one eligible scalar.
Colors share one curve across RGBA. Integer endpoints retain their integer type
and interpolate with rounding, without conversion of endpoints to `f32`.
Array structure is not animated. Tracks store a structural target and typed
endpoints with a curve; there is no separate animation layout.

Enum membership is retained in runtime values and scene bindings. Its GPU
representation is `u32`. `ui.enum_variants` may be omitted to show numeric labels;
when provided it must label every member exactly once.

## Generated WGSL API

Zerium prepends a typed parameter struct to every pass. All shader kinds use
the same public loader name:

```wgsl
let params = zerium_load_parameters(instance_index); // item shader
let params = zerium_load_parameters();               // effect pass
```

Tuple fields are generated structs with fields `v0`, `v1`, and so on. Arrays use
`params.<id>_len` plus `zerium_parameter_<id>_get(params, index)`, including
arrays of strings. A string is represented by `ZeriumString`; its byte length is
`value.byte_len`, and `zerium_string_byte(params._raw, value, index)` reads one
UTF-8 byte. The descriptor layout and backing-buffer offsets remain host-private.

All host declarations use the `zerium_`/`Zerium` namespace. Plugin WGSL must
not declare names in that namespace. Media file IDs `inputs` and `sampler` are
reserved because they would collide after generated-name expansion.

Every shader receives `ZeriumRenderContext` through `zerium_render_context`.
It contains `output_size`, the fixed `composition_size`, and
`composition_scale`. Item shaders pass their instance index; effect passes do
not. Composition coordinates are centered, with positive X right and positive
Y down.

Procedural and media item shaders also receive quad and coordinate helpers such
as `zerium_item_quad_corner`, `zerium_item_quad_clip_position`, and
`zerium_rotate_2d`.

## Effects and passes

An effect is always an ordered, non-empty list of explicit passes. There is no
top-level shader and no implicit render pass.

```json
{
  "id": "blur",
  "label": "Blur",
  "category": "Blur",
  "parameters": [{
    "id": "radius",
    "label": "Radius",
    "type": "f32",
    "default": 8,
    "animatable": true
  }],
  "passes": [{
    "type": "compute",
    "shader": { "source": "blur.wgsl" },
    "dispatch": ["width", "height", "one"],
    "constants": [{
      "id": "direction",
      "type": { "tuple": ["f32", "f32"] },
      "value": [1, 0]
    }]
  }]
}
```

Pass constants use fixed-size scalar or tuple value types. Strings, including strings inside tuples, are rejected for pass constants. They join the generated
parameter struct for that pass but never become inspector state. Compute
workgroup size is read from WGSL's `@workgroup_size`; the manifest only controls
dispatch dimensions.

Render and compute passes receive `zerium_effect_input`, the current pipeline
input; `zerium_effect_source`, the image captured at the start of the current
regular pass chain; and
`zerium_effect_sampler`. Compute passes write with
`zerium_store_output(position, color)`.

A temporal pass must be first and may occur at most once. Its `sampling`
declaration maps public parameters to host-controlled subframe sampling, while
its reducer owns the weighting algorithm:

```json
{
  "type": "temporal",
  "sampling": {
    "type": "shutter",
    "sample_count": "samples",
    "angle": "shutter_angle",
    "phase": "phase"
  },
  "reducer": { "source": "motion_blur_accumulate.wgsl" }
}
```

The sample-count parameter must have explicit constraints within `1..=32`, the
angle must have a non-negative minimum, and phase (when present) must be bounded
within `-1..=1`.

Reducer WGSL receives `zerium_temporal_sample`,
`zerium_temporal_accumulation`, `zerium_temporal_sampler`, and
`zerium_temporal_info()`. Later render or compute passes consume the reduced
texture normally.

Shader identities are derived internally from plugin ID, item/effect ID, and
pass index. Plugins declare source paths and entry points, not global pipeline
IDs. Render entry points default to `vertex_main` and `fragment_main`; compute
defaults to `compute_main`.
