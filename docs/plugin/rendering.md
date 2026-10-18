# Rendering and shaders

[Plugin API v1](README.md) · [Properties](properties.md) · **Rendering** · [Inputs](inputs.md)

The manifest declares what an item or effect produces. Generated WESL modules
provide its properties, image access, and host operations. This page covers the
contract shared by item rendering and all effect pass kinds.

- [Generated WESL API](#generated-wesl-api)
- [Render surfaces and bounds](#render-surfaces-and-bounds)
- [Image sampling](#image-sampling)
- [Effects and passes](#effects-and-passes)

## Generated WESL API

Shader sources import the host and entity interfaces explicitly. For an item
with shader-visible properties:

```wesl
import package::generated::host::item::{context, quad_corner};
import package::generated::shape::{ZeriumProps, props};
```

`package::generated::host` contains five modules. Import the parts used by a pass:

| Module | Purpose |
| --- | --- |
| `item` | Item context, placement, and quad geometry |
| `effect` | Shared effect context, coordinates, input images, and color helpers |
| `render` | Fullscreen vertex type and vertex helper |
| `compute` | Storage texture output via `store` |
| `temporal` | Time sample information via `info` |

For example, a fullscreen effect imports `render` and `effect`; a compute pass
imports `compute` and `effect`. Temporal reduction also imports `temporal`.
Common math and color utilities live in `package::generated::support::util`.
`package::generated::_internal` contains implementation details and is not an
authoring interface.

Shader modules use relative WESL paths: `filters::blur` resolves to
`filters/blur.wesl`, and a helper at `math/color.wesl` is imported as
`package::math::color`. Authors choose the directory layout; the top-level
`generated` namespace is reserved for the host.

Generated interfaces are ordinary WESL files with explicit imports:

| Interface | Generated file | Import module |
| --- | --- | --- |
| Owner properties (all pass kinds) | `generated/<entity_id>.wesl` | `package::generated::<entity_id>` |
| Named image | `generated/<entity_id>/inputs/<input_id>.wesl` | `package::generated::<entity_id>::inputs::<input_id>` |
| Previous pass image | `generated/host/effect.wesl` | `package::generated::host::effect::{sample_input, input_info, ...}` |
| Original source image | `generated/host/effect.wesl` | `package::generated::host::effect::{sample_source, source_info, ...}` |

Each visual item or effect has one property interface shared by all of its passes.
Each named input has its own image module, independent of the property module.
For example:

```wesl
import package::generated::image::props;
import package::generated::image::inputs::source::sample_uv;
```

Entry modules import their owner's interface explicitly; shared helpers receive
owner-specific values as arguments. Item and effect IDs must be distinct across
both kinds. Module path components, entity IDs, and input IDs must be WGSL
identifiers; WGSL reserved words cannot be used.

Regenerate after changing entity IDs, property IDs/types/order, input IDs/order,
item/effect ownership, or the host API. Labels, defaults, enum choices, and array
length limits do not require regeneration.

### Property access

See [Properties](properties.md#properties) for manifest declarations.

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

### Context and coordinates

`context()` provides `ZeriumContext`: physical `output_size`, fixed
`composition_size`, logical `surface_size`, and pixel density `composition_scale`.
Item shaders import `context()` from `host::item`; effect passes import it from
`host::effect`. Both describe the current render surface. Item property loaders
continue to use an instance index. Composition coordinates are centered, with
positive X right and positive Y down.

Item helpers include `placement_center`, `quad_corner`, and `quad_position`.
Rotation and color helpers (`rotate`, `srgb`, `scene_color`) live in
`package::generated::support::util`. Image access uses the generated sampling
functions in [Image sampling](#image-sampling); plugin code does not bind textures
or samplers.

## Render surfaces and bounds

Items declare `render.shader` and `render.bounds`; effects provide an ordered
`render.passes` array, with independent `bounds` for every pass. Both can declare
named `render.inputs`. Items may omit `render` when they only provide audio or
editor behavior. Item `vertex_count` defaults to six; effect `render.scale`
defaults to one.

`bounds` declares logical content bounds in composition pixels, independent
of texture resolution. A scene's bounds are the union of its children's bounds;
empty scenes have empty bounds. Each pass evaluates its output bounds from the
previous image's logical bounds, the original source, and its named images.
The final pass defines the effect's output. Item and scene effects use the same
rules; scene boundaries do not clip content.

Shader nodes render their full logical bounds before being composited into a
parent. Each pass receives complete input images and renders its own output
extent; input texel indices need not match output texel indices. Composition and
camera nodes select the children needed for their destination, and camera views
map sampling density back into the source scene. Final output and explicit
camera views clip to their own viewports. A plain render-result input retains
the composition's logical bounds, including content outside the viewport.

Complete intermediate images must fit the GPU texture limit at their sampling
density. In particular, applying an effect to a group with widely separated
children requires an image covering the space between them.

```json
{
  "type": "render",
  "shader": {
    "module": "expand"
  },
  "bounds": {
    "min": [
      "input::min::x - p::radius",
      "input::min::y - p::radius"
    ],
    "max": [
      "input::max::x + p::radius",
      "input::max::y + p::radius"
    ]
  }
}
```

For an item, `input` starts as the viewport; for an effect, it is the incoming
image rectangle. Use `viewport::*` edges for a full-frame item and `input::*` to
preserve an effect's input. Invalid, reversed, or non-finite results fail rendering.
The shader is responsible for drawing within its declared bounds.

Expressions support arithmetic, parentheses, `min`, `max`, and functions such
as `math::abs`, `math::sin`, and `math::cos`. Local assignments can be separated
with `;`. Rectangle variables use `{input,source,viewport}::{min,max,center,size}::{x,y}`.
Numeric, boolean (`0`/`1`), and enum properties use `p::<id>`; tuple scalars use
`p::<id>::v0`, `::v1`, and subsequent zero-based component indices. Named image
bounds use `images::<id>::{min,max,center,size}::{x,y}`. `o::<id>` provides pass
overrides.

For ordinary effect passes, `source::*` describes the image entering the current
effect. After a temporal pass, it describes the reduced image. For the
temporal pass's bounds program, both `input::*` and `source::*` describe the union
of available sample bounds. The reducer's sampled `source` is its accumulation;
see [Temporal passes](#temporal-passes).

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
[rotation declaration](../../plugins/zerium.builtin/plugin.json) and
[3D rotation shader](../../plugins/zerium.builtin/effects/rotate_3d/render.wesl).

## Image sampling

Every image has its own paired texture and metadata. The generated functions use
that pair directly; shaders do not supply image indices or binding numbers.

Every image carries logical `bounds_min`/`bounds_size`, raster coverage
`raster_min`/`raster_size`, and physical pixels per composition unit in `density`.
Logical bounds are independent of camera position and sampling density. Pixels
outside an image sample as transparent, with transparent bilinear filtering at
raster edges.

Effect passes access the previous pass and original source through
`package::generated::host::effect`:

```wesl
import package::generated::host::effect::{
    sample_input,
    load_input,
    input_position,
    input_info,
    sample_source
};

let color = sample_input(position); // composition coordinates, bilinear
let exact = load_input(position);   // composition coordinates, nearest texel
let original = sample_source(position);
let center = input_position(vec2(0.5)); // normalized logical input coordinates
let image = input_info();
```

Named inputs expose the same functions from their own modules:

```wesl
import package::generated::image::inputs::source::{
    info,
    position as image_position,
    sample,
    sample_uv,
    load
};

let descriptor = info();
let center = image_position(vec2(0.5));
let color = sample(position); // composition coordinates
let normalized = sample_uv(uv); // logical image UVs
let exact = load(position);
```

Decoded media and text use their centered native pixel bounds, so normalized
sampling is convenient for item quads. Render results already use scene
coordinates. Imports can use aliases when accessing several inputs:

```wesl
import package::generated::my_effect::inputs::foo::{sample as sample_foo};
import package::generated::my_effect::inputs::foo_uv::{sample as sample_foo_uv};
```

### Output coordinates

The shared `package::generated::host::effect` module provides `context`,
`uv_to_position`, `pixel_position`, and `reference_uv` for all effect pass kinds.
`package::generated::host::render` provides `ZeriumVertex` and `vertex` for
fullscreen render and temporal passes.
`uv_to_position(uv)` and `pixel_position(pixel)` map output UVs or integer pixel
indices to composition coordinates. Item shaders use
`item::pixel_position(fragment_position.xy)` instead.
`reference_uv(position)` maps positions into the full logical output bounds for
procedural patterns. Compute passes import `store(pixel, color)` from
`package::generated::host::compute`; color helpers such as `straight`, `over`,
and `premul` remain in `package::generated::host::effect`.

Effect passes receive `input` (the previous pass) and `source` (the image entering
the current effect), plus any named images. Named inputs use separate
modules, so their IDs may also be `input` or `source`. All input images are supplied
in their own complete bounds.

## Effects and passes

Effects declare an ordered, non-empty `render.passes` array:

```json
{
  "render": {
    "passes": [
      {
        "type": "compute",
        "shader": {
          "module": "blur"
        },
        "dispatch": [
          "width",
          "height",
          "one"
        ],
        "bounds": {
          "min": [
            "input::min::x - 10",
            "input::min::y"
          ],
          "max": [
            "input::max::x + 10",
            "input::max::y"
          ]
        },
        "overrides": {
          "direction_x": 1,
          "direction_y": 0
        }
      }
    ]
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
  "reducer": {
    "module": "motion_blur_accumulate"
  },
  "bounds": {
    "min": [
      "input::min::x",
      "input::min::y"
    ],
    "max": [
      "input::max::x",
      "input::max::y"
    ]
  }
}
```

`range` samples evenly between `f32` start and end offsets measured in frames.
The `u32` sample-count property must have constraints within `1..=32`.
Alternatively, `{"type":"offsets","offsets":"sample_offsets"}` reads an array
of 1–32 `f32` frame offsets. Negative offsets sample the past, positive offsets
the future, and zero the current frame.

The temporal pass's output bounds use the union of all available samples as
`input`; every reduction step uses that same logical output extent.
Each reduction step is a pass node: `input` is the current temporal sample and
`source` is the previous accumulation, initially transparent.
`package::generated::host::temporal` provides `info() -> ZeriumTemporalInfo`, with
`sample_index`, `sample_count`, `frame_offset`, and `sample_progress` (0–1);
surface dimensions come from `context()`. Later render or compute passes consume
the reduced image, which also becomes their original `source`.
