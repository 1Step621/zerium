# Rendering and shaders

[Plugin API v1](README.md) · [Properties](properties.md) · **Rendering** · [Inputs](inputs.md)

## Visual property contract

Each item's `render.properties` lists the property IDs used by its shaders,
bounds, and visual inputs, in shader ABI order. Use `[]` for none. Missing or
duplicate IDs and visual input references outside the list are rejected.

Only these properties are evaluated for visuals and exposed to shaders. Keep
purely audio controls outside the list; shared audio/visual properties are
evaluated independently at each consumer's sample times. Effect passes consume the
effect's full property list.

## Generated WESL API

Generate interfaces with `zerium plugin generate path/to/plugin`. Sources import
host functions and owner interfaces explicitly:

```wesl
import package::generated::host::item::{context, quad_corner};
import package::generated::shape::{ZeriumProps, props};
```

| Host module | Purpose |
| --- | --- |
| `package::generated::host::item` | Item context, placement, and quad geometry |
| `package::generated::host::effect` | Effect context, coordinates, images, and color helpers |
| `package::generated::host::render` | Fullscreen vertex type and helper |
| `package::generated::host::compute` | Storage texture output via `store` |
| `package::generated::host::temporal` | Sample information via `info` |

Common math and color helpers live in `package::generated::support::util`.
The `_internal` namespace is private. Each owner has one property module at
`package::generated::<entity_id>` and an image module per named input at
`package::generated::<entity_id>::inputs::<input_id>`.

Module paths follow the source directory: `filters::blur` resolves to
`filters/blur.wesl`; imports within the bundle start with `package::`.
The top-level `generated` namespace is reserved. Item and effect IDs must be
distinct across both kinds; entity IDs, input IDs, and module components must be
non-reserved WGSL identifiers.

Regenerate after changing IDs, property types, the order or membership of
`render.properties`, input order, owner kind, or the host API. Labels, defaults,
enum choices, and array length limits do not require regeneration.

### Property access

```wesl
let properties = props(instance_index); // item
let properties = props();               // effect
```

A shader without shader-visible properties has no `ZeriumProps` or `props`.
Tuples use fields `v0`, `v1`, and so on. Arrays use `properties.<id>_len` and
`get_<id>(properties, index)`, imported from the owner's module.

Files and strings have no shader fields; input capabilities turn their content
into textures. Mixed tuples preserve the other scalars' original `vN` indices.
Properties and arrays containing only CPU values are omitted.

### Context and coordinates

`context()` returns physical `output_size`, fixed `composition_size`, logical
`surface_size`, and pixel density `composition_scale`. Import it from `host::item`
or `host::effect` for the current pass. Composition coordinates are centered:
positive X points right and positive Y down.

Item geometry helpers are `placement_center`, `quad_corner`, and `quad_position`.
Rotation and color helpers (`rotate`, `srgb`, `scene_color`) live in
`support::util`. Use generated sampling functions for images; plugin code does
not bind textures or samplers.

## Render surfaces and bounds

Items declare `render.shader` and `render.bounds`; effects declare ordered
`render.passes` with bounds per pass. Both may declare `render.inputs`. Items
without visual output can omit `render`. Item `vertex_count` defaults to six;
effect `render.scale` defaults to one.

Bounds describe logical content in composition pixels, independently of texture
resolution. Scene bounds are the union of children; scene boundaries do not clip
content. Shader passes receive complete images and render their declared output
extent. Camera views and final output clip to their viewports; plain render
results keep offscreen content. Each intermediate image must fit the GPU texture
limit at its sampling density, including effects on widely separated children.

```json
{
  "type": "render",
  "shader": { "module": "expand" },
  "bounds": {
    "script": "#{ min: [input.min.x - p.radius, input.min.y - p.radius], max: [input.max.x + p.radius, input.max.y + p.radius] }"
  }
}
```

`bounds.script` is a Rhai script returning `#{ min: [x, y], max: [x, y] }`.
An item's initial `input` is the viewport; an effect's is the incoming image.
The shader must draw within its bounds. Invalid, reversed, non-finite, or failed
results stop rendering with a diagnostic identifying the owner/pass.

| Script input | Contents |
| --- | --- |
| `input`, `source`, `viewport` | Rectangles with `min`, `max`, `center`, and `size`; each point has `x` and `y` |
| `p.<id>` | Property value; tuple and array components use zero-based indices |
| `images.<id>` | Named image rectangle |
| `o.<id>` | Numeric pass override; use `o["0"]` for a numeric ID |

Booleans remain booleans, integers and enums remain integers, colors are
four-component arrays, and text is a string. Inputs are read-only and every
execution starts with a fresh scope. Scripts support local bindings, functions,
conditionals, arrays, maps, and loops, plus math such as `abs`, `sin`, `cos`,
`min`, `max`, `exp2`, `pow`, `lerp`, and `clamp`. Execution is bounded to 50,000
operations and 32 call levels, with no imports or dynamic `eval`.

For ordinary passes, `source` is the image entering the current effect. A temporal
pass uses the union of available samples for its bounds inputs; later passes use
the reduced image as `source`. See [Temporal passes](#temporal-passes).

### Placement and projection

`placement_center(position, size, origin)` matches the WESL item helper. It uses
unpadded size and origin `0` (start), `1` (center), or `2` (end):

```text
placement_center(p.position[0], p.size[0], p.origin[0]) - abs(p.size[0]) / 2.0
```

Use `[1]` for the vertical axis. Numbers may be integers or floats; origin must
be an integer in range.

`projected_rect(min, max, rotation, center, focal, near_ratio)` returns the same
bounds map. Pass arrays for the corners, XYZ rotation in degrees, and rotation
center, then focal length and near clip ratio. Match the shader's projection and
near depth; see the bundled [3D rotation shader](../../plugins/zerium.builtin/effects/rotate_3d/render.wesl)
and its [declaration](../../plugins/zerium.builtin/plugin.json).

## Image sampling

Images pair a texture with metadata: logical `bounds_min`/`bounds_size`, raster
coverage `raster_min`/`raster_size`, and pixels per composition unit in `density`.
Outside pixels are transparent, including bilinear filtering at raster edges.

Named image modules expose:

```wesl
import package::generated::image::inputs::source::{info, position, sample, sample_uv, load};

let descriptor = info();
let center = position(vec2(0.5)); // normalized logical image coordinates
let color = sample(center);     // composition coordinates, bilinear
let normalized = sample_uv(uv); // logical image UVs, bilinear
let exact = load(center);        // composition coordinates, nearest texel
```

Effect passes access the previous pass as `input` and the original image entering
the effect as `source`, through `package::generated::host::effect`:

| Named-image function | Previous pass | Original source |
| --- | --- | --- |
| `info` | `input_info` | `source_info` |
| `position` | `input_position` | `source_position` |
| `sample` | `sample_input` | `sample_source` |
| `sample_uv` | `sample_input_uv` | `sample_source_uv` |
| `load` | `load_input` | `load_source` |

Named inputs have separate modules, so they can also be called `input` or
`source`. WESL import aliases distinguish multiple images. Media and text use
centered native pixel bounds; render results already use scene coordinates.

### Output coordinates

`host::effect` provides `uv_to_position`, `pixel_position`, and `reference_uv`.
The first two map output UVs or integer pixels to composition coordinates;
`reference_uv` maps a position into the full logical output bounds. Item shaders
use `item::pixel_position(fragment_position.xy)`.

`host::render` provides `ZeriumVertex` and `vertex` for fullscreen passes.
Compute passes write through `host::compute::store(pixel, color)`.
Color helpers `straight`, `over`, and `premul` live in `host::effect`.

## Effects and passes

Effects declare a non-empty `render.passes` array:

```json
{
  "render": {
    "passes": [{
      "type": "compute",
      "shader": { "module": "blur" },
      "dispatch": ["width", "height", "one"],
      "bounds": {
        "script": "#{ min: [input.min.x - 10, input.min.y], max: [input.max.x + 10, input.max.y] }"
      },
      "overrides": { "direction_x": 1, "direction_y": 0 }
    }]
  }
}
```

Render entry points default to `vertex_main` and `fragment_main`; compute defaults
to `compute_main`. Workgroup size comes from `@workgroup_size`. Dispatch dimensions
are `width`, `height`, `max_dimension`, or `one`, rounded up to workgroups.

Pass `overrides` set WGSL `override` declarations by name or the decimal string
of an explicit `@id`. Values convert to the declared scalar type; booleans use
`0` or `1`. Omitted values use shader defaults:

```wesl
override direction_x: f32 = 1.0;
override direction_y: f32 = 0.0;
```

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
  "reducer": { "module": "motion_blur_accumulate" },
  "bounds": {
    "script": "#{ min: [input.min.x, input.min.y], max: [input.max.x, input.max.y] }"
  }
}
```

`range` samples evenly between `f32` frame offsets; its `u32` count property must
have constraints within `1..=32`. Alternatively,
`{"type":"offsets","offsets":"sample_offsets"}` reads an array of 1–32 `f32`
frame offsets. Negative samples look backward, positive forward, and zero at the
current frame. Samples evaluate the image before the current effect.

Bounds use the union of available samples, and all reduction steps share that
output extent. Each step's `input` is its sample and `source` is the previous
accumulation, initially transparent. Later render or compute passes consume the
reduced image, which also becomes their `source`.

`host::temporal::info()` returns `sample_index`, `sample_count`, `frame_offset`,
and `sample_progress` (0–1) in `ZeriumTemporalInfo`. Surface dimensions come from
`context()`.
