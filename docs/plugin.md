# Plugin API v1

A plugin is one directory containing `plugin.json` and every WESL shader file that the
manifest references. Zerium validates the complete bundle before registering
it. Missing shader or generated files, invalid schemas, and shader/API mismatches
are errors.

```text
com.example.plugin/
├── generated/
│   └── …
├── plugin.json
├── shape.wesl
├── blur.wesl
└── wesl.toml
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
    "shader": { "source": "shape.wesl" },
    "capabilities": []
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

The checked-in [schema](../plugins/plugin.schema.json)
provides editor completion. Rust validation remains authoritative for rules
that JSON Schema cannot express, such as unique IDs, text-item requirements,
property/default compatibility, and cross-references.

## Rust interface boundary

Manifest-backed structs expose behavior and immutable views rather than public
storage fields. Callers use `ItemSchema::shader`, `capabilities`, `files`, `file`,
`audio`, and `properties`. Effects expose `capabilities`, `files`, `properties`,
`passes`, and `render_scale`. The Rust API remains crate-private where possible.
`PropertySchema` is read-only outside `domain`: UI and rendering code use its
accessors, while timeline code retains the narrower internal access needed to
project plugin properties into editable scene arguments. Item and effect
schemas implement `PluginCatalogEntry`, giving catalog UIs one shared metadata
interface and one consistent search-term policy.

Shared property contracts live in [`../src/domain/property/`](../src/domain/property/), independently
of plugin loading: `types` owns the scalar/tuple/array algebra, `value` owns
checked values and collections, `schema` exposes the read-only model,
`schema` and `constraints` enforce contracts, and `ui`/`numeric` provide editor
views. The plugin layer retains shader identifier validation and the runtime ABI
packer. Property errors are converted to plugin errors at this boundary.
Animation eligibility and interpolation are explicit operations in
[`../src/domain/animation/`](../src/domain/animation/), rather than methods added to property types
from another module.

Public visibility is reserved for plugin loading and immutable schema
inspection. Editor-derived choices, host capabilities, value projection, and
other Zerium runtime adapters are crate-private. The
plugin module denies unreachable public items so private implementation helpers
cannot accidentally become part of the Rust API.

## Item and effect inputs

An item declares its output `shader` at the top level. Its `capabilities` array
contains named inputs to that shader. An effect has the same array, and each
render, compute, or temporal pass can use those inputs. Array order fixes the
GPU binding order; each `id` is unique within its item or effect and becomes
the WESL symbol imported from `package::generated::capability_input`.

- `shader` renders a separate item-like input from a WESL source. It sees the
  owner's properties and does not receive the owner's capability array.
- `media` decodes a video or image file and exposes its pixels as a texture.
- `text` rasterizes text from referenced properties into a texture.
- `render_result` composites an inclusive range of layers behind the owner.

These input types use the same rendering rules for items and effects. A missing
or unavailable media frame supplies a transparent texture in either case.
Item and effect shader IDs retain separate namespaces so identically named
schemas and capabilities cannot collide.

Up to eight capabilities may be declared. Their IDs must be valid WGSL
identifiers. The IDs are used directly, without a `slot_` prefix. All
capability textures contain scene-linear, premultiplied color. For example:

```json
{
  "id": "video",
  "label": "Video",
  "category": "Media",
  "symbol": "▶",
  "shader": { "source": "media.wesl" },
  "capabilities": [{
    "type": "media",
    "id": "source",
    "label": "Source",
    "media_type": "video",
    "reader": "zerium.ffmpeg",
    "extensions": ["mp4", "mov"]
  }],
  "audio": { "inputs": ["source"], "volume": "volume" }
}
```

The item shader can use `import package::generated::capability_input::{source,
capability_sampler};` and sample `source` with `capability_sampler`.
Effects use the same import path. Their passes also receive `effect_input`
(the current image) and `effect_source` (the image captured at the start of a
regular pass chain). Each effect instance owns its imported media assets; they
are saved with the project.

`audio` and `editor` remain top-level item roles. They do not add shader inputs.
`audio.inputs` refers to video capabilities or to audio files declared in
`audio.files`; the host mixer reads the `f32` gain property named by `volume`.
A media file property referenced by `editor.size` can use the host's aspect-ratio
lock, which stays in editor state and outside the shader ABI.

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

The referenced properties must have the expected types. Alignment properties
must be enums containing exactly `0`, `1`, and `2`.

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

The bundled [layer-mask effect](../plugins/zerium.builtin/plugin.json) samples
`behind` and multiplies it by the owner's alpha. Its `invert_mask` setting
uses the transparent part of the owner instead; it is off by default. Shader
and media capabilities can be combined with it in one effect.

## Properties

Every property has one identity and one value. Tuple coordinates can be edited
and animated independently, but are not modeled as separate property lanes.

```json
{
  "id": "position",
  "label": "Position",
  "type": {"value": ["f32", "f32"]},
  "default": {"tuple": [{"f32": 0}, {"f32": 0}]},
  "configurations": [
    {
      "animatable": true,
      "constraints": {"min": -1000000, "max": 1000000},
      "ui": {"label": "X", "unit": "px", "step": 1}
    },
    {
      "animatable": true,
      "constraints": {"min": -1000000, "max": 1000000},
      "ui": {"label": "Y", "unit": "px", "step": 1}
    }
  ]
}
```

`type` and `default` each use a single key for their variant. A scalar type is
`{"value":"f32"}` and its default is `{"f32":0}`; tuples and arrays use
`tuple` and `array` default values. Each `configurations` entry
describes one scalar position instead: its `scene_bindable` permission,
`editable`/`animatable` flags, `constraints`, and `ui` hints.
`scene_bindable` defaults to true and is checked per scalar, so tuple
scalars are published and connected independently.

`label` and `default` belong to the property contract, while each scalar's
`scene_bindable` permission and numeric `constraints` belong to its
`configurations` entry. `ui` only
contains presentation hints: `label`, `unit`, `step`,
`visible`, `enum_variants`, `multiline`, and `editor`. Numeric values use the same canonical
unit in projects, shaders, and editor controls.
Constraints are enforced for defaults, direct edits, array elements, loaded
projects, and animation endpoints.

Scalar types are `f32`, `i32`, `u32`, `bool`, `color`, `string`, and finite `enum` contracts. Color is one scalar, edited with a color picker even inside tuples and arrays. Numeric inputs address numeric scalars by their tuple index; RGBA components are not flattened into numeric input indices.
Item-generic editor behaviors reference properties from the item `editor` block, for
example `"editor": { "position": "position", "size": "size", "points":
"points", "label": "text" }`. Position and size references require two-`f32`
tuples. The points reference requires an array of two-`f32` tuples and also
requires position and size references; each point is expressed as a percentage
within the item's size, with `[0, 0]` at the top-left and `[100, 100]` at the
bottom-right. The label reference requires a string. This follows the same
property-ID wiring used by text rasterization, audio gain, and temporal
sampling. Effects may still use presentation hints such as `multiline`.

An array of strings uses the ordinary array inspector with a text input for each
element by default.
Use the specialized font-family editor when the values represent fallback
fonts:

```json
{
  "type": {"array": {"element_type": "string", "max_items": 1024}},
  "default": {"array": []},
  "configurations": [{
    "ui": { "editor": "font_family" }
  }]
}
```

Array configurations describe the scalar positions of one element template
and apply to every element. The property-level `default` holds the initial
elements; an empty list means the array starts empty. Each non-empty array
element has a stable positive `id` and a tagged `value`.

Without `ui.editor`, entries can be added, edited, reordered, and removed as
ordinary strings. Empty and duplicate strings are valid list values. With
`"editor": "font_family"`, the same stored type uses the system-font picker.
Like every other array, it uses the declared `min_items`/`max_items` bounds and
supports scene binding through its individual elements. Arrays are not themselves
scene-argument values, and nested arrays are not part of the type algebra.

Tuples contain 2–64 arbitrary scalars, including colors, strings, and enums. Their structure prevents
nested tuples and arrays:

```json
{
  "type": {"value": ["f32", "bool", "string", {"enum": [2, 7]}]},
  "default": {"tuple": [{"f32": 0}, {"bool": true}, {"string": "Label"}, {"enum": 7}]}
}
```

Array definitions keep `element_type`, `min_items`, and `max_items` inside the `array`
object. `min_items` defaults to zero; `max_items` is required.
Arrays contain a scalar or tuple value type, so arrays of tuples are valid
without allowing nested arrays:

```json
{
  "type": {"array": {"element_type": ["f32", "f32"], "min_items": 3, "max_items": 1024}},
  "default": {"array": [
    {"id": 1, "value": {"tuple": [{"f32": 0}, {"f32": 0}]}},
    {"id": 2, "value": {"tuple": [{"f32": 100}, {"f32": 0}]}},
    {"id": 3, "value": {"tuple": [{"f32": 50}, {"f32": 100}]}}
  ]}
}
```

Finite choices are types, not UI options:

```json
{
  "type": { "value": { "enum": [0, 1] } },
  "default": { "enum": 0 },
  "configurations": [{
    "ui": { "enum_variants": { "0": "Outside", "1": "Inside" } }
  }]
}
```

`editable` defaults to true. Set it to `false` for plugin properties whose values are produced by the
plugin and must be displayed without allowing direct edits. Tuple properties specify one object per scalar
in `configurations`; each object has its own `scene_bindable`,
`editable`, and `animatable` settings. Disabling `editable` also disables animation
editing for the property or scalar. `scene_bindable` defaults to true and remains independent, so a
scalar can still be exposed through a scene binding when the plugin uses that as its input path.
Scene arguments themselves are scalars, preserving enum membership;
tuple scalars are published and connected independently.

### Tuple metadata and animation

`configurations` specifies metadata for each tuple scalar, including inside an array
element. The list must match the tuple length. Each scalar object contains its
`scene_bindable`, `editable`, `animatable`, `constraints`, and `ui`
settings; UI properties
(`label`, `unit`, `step`, `visible`, `enum_variants`, and `multiline`) stay
inside that scalar's `ui`. Missing labels use the one-based scalar index, and
omitted UI metadata uses the scalar defaults. A tuple is visible if any scalar
is visible. Numeric bounds belong inside each scalar's `constraints`; tuple-level
`min`/`max` are rejected. The `font_family` editor remains an
array-of-strings setting.
`scene_bindable` and `animatable` are scalar permissions: standalone scalar
properties use one `configurations` entry, while tuple properties use one
entry per tuple scalar. The same scalar metadata applies to every element in
an array of tuples.

```json
{
  "id": "entry",
  "label": "Entry",
  "type": {"value": ["f32", {"enum": [2, 7]}, "string", "color"]},
  "default": {"tuple": [
    {"f32": 1}, {"enum": 2}, {"string": "Caption"}, {"color": [1, 1, 1, 1]}
  ]},
  "configurations": [
    {
      "animatable": true,
      "constraints": {"min": 0, "max": 10},
      "ui": {"label": "Width", "unit": "px"}
    },
    {"ui": {"label": "Mode", "enum_variants": {"2": "Low", "7": "High"}}},
    {"ui": {"label": "Caption", "multiline": true}},
    {
      "animatable": true,
      "constraints": {"min": 0, "max": 1},
      "ui": {"label": "Tint"}
    }
  ]
}
```

Every animation track addresses one scalar: a standalone scalar, a tuple scalar,
or a scalar within an array element. Array elements have editor-side stable IDs,
but plugins receive only their ordered values. Numeric scalars and colors interpolate;
bools, strings, and enums remain static. A mixed tuple can animate its eligible
scalars independently. An animatable scalar must be numeric or a color;
boolean, string, and enum scalars remain static.
Colors share one curve across RGBA. Integer endpoints retain their integer type
and interpolate with rounding, without conversion of endpoints to `f32`.
Array structure is not animated. Tracks store a structural target and typed
endpoints with a curve; there is no separate animation layout.

Enum membership is retained in runtime values and scene bindings. Its GPU
representation is `u32`. `ui.enum_variants` may be omitted to show numeric labels;
when provided it must label every member exactly once.

## Generated WESL API

Shader sources are WESL modules. The host API is imported explicitly so editor
tools can resolve it without seeing Zerium's Rust-side source concatenation:

```wesl
import package::generated::item::{context, quad_corner};
import package::generated::properties_shape::{ZeriumProps, props};
```

The path without `.wesl` is also the module path used by runtime compilation;
each path segment must be a valid WGSL identifier. For example,
`shapes/ellipse.wesl` is compiled as `package::shapes::ellipse`.

`properties_<shader>.wesl` is generated from the manifest properties. Run
`zerium plugin generate` in a plugin directory whenever its manifest or shader
contract changes. The command writes the host interface modules, property
modules, and per-source capability interfaces under `generated/`. Shader imports are authored in
the shader source and are not rewritten by the generator.
These generated modules are packaged with the plugin. At runtime,
`package::generated::capability_input` is selected from the interface for the
shader being compiled. Zerium links plugin WESL to in-memory WGSL once
when loading the application and shares that result between preview and export;
plugin authors do not generate or distribute WGSL. Zerium rejects generated
files whose manifest fingerprint is stale. A source shared by several items or passes gets
the property fields whose type and ABI location agree in every use. The source
must use one shader kind and one capability-input layout.

```sh
zerium plugin generate
# or: zerium plugin generate path/to/plugin
```

Before opening the application, a plugin can be checked with the same WESL
linking and WGSL validation used by the renderer:

```sh
zerium plugin validate path/to/plugin
```

The generated property module exposes a typed struct for each shader source.
All shader kinds use the same loader name:

```wesl
let properties = props(instance_index); // item shader
let properties = props();               // effect pass
```

Tuple fields are generated structs with fields `v0`, `v1`, and so on. Arrays use
`properties.<id>_len` plus `get_<id>(properties, index)`, including arrays
of strings. A string is represented by `ZeriumStr`; its byte length is
`value.byte_len`, and `str_byte(properties._raw, value, index)` reads one
UTF-8 byte. The descriptor layout and backing-buffer offsets remain host-private.

Host types use the `Zerium` namespace; resource and function names are concise
and unprefixed. Plugin WESL must not redeclare imported host names.

Every shader receives `ZeriumContext` through `context`.
It contains `output_size`, the fixed `composition_size`, and
`composition_scale`. Item shaders pass their instance index; effect passes do
not. Composition coordinates are centered, with positive X right and positive
Y down.

Procedural and media item shaders also receive quad and coordinate helpers such
as `quad_corner`, `quad_position`, and
`rotate`.

## Effects and passes

An effect is always an ordered, non-empty list of explicit passes. There is no
top-level shader and no implicit render pass.

```json
{
  "id": "blur",
  "label": "Blur",
  "category": "Blur",
  "properties": [{
    "id": "radius",
    "label": "Radius",
    "type": { "value": "f32" },
    "default": { "f32": 8 },
    "configurations": [{
      "animatable": true
    }]
  }],
  "passes": [{
    "type": "compute",
    "shader": { "source": "blur.wesl" },
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

Render and compute passes receive `effect_input`, the current pipeline
input, and `effect_source`, the image captured at the start of the current
regular pass chain. They also receive `effect_sampler`. Capability inputs are
imported by ID from `package::generated::capability_input` with
`capability_sampler`. Compute passes write with `store(position, color)`.

A temporal pass must be first and may occur at most once. Its `sampling`
declaration maps public properties to host-controlled subframe sampling, while
its reducer owns the weighting algorithm:

```json
{
  "type": "temporal",
  "sampling": {
    "sample_count": "samples",
    "angle": "shutter_angle",
    "phase": "phase"
  },
  "reducer": { "source": "motion_blur_accumulate.wesl" }
}
```

The sample-count property must have explicit constraints within `1..=32`, the
angle must have a non-negative minimum, and phase (when present) must be bounded
within `-1..=1`.

Reducer WESL receives `temporal_sample`,
`temporal_accumulation`, `temporal_sampler`, and
`info()`. Later render or compute passes consume the reduced
texture normally.

Shader identities are derived internally from plugin ID, item/effect ID, and
pass index. Plugins declare source paths and entry points, not global pipeline
IDs. Render entry points default to `vertex_main` and `fragment_main`; compute
defaults to `compute_main`.
