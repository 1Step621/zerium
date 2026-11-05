# Plugin API v1

[Documentation](../README.md) · **Plugin API v1** · [Architecture](../architecture/README.md)

A plugin contains `plugin.json`, WESL sources, and generated interfaces:

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
plugin and regenerate it rather than editing it manually.

## Minimal plugin

```json
{
  "$schema": "https://raw.githubusercontent.com/1Step621/zerium/refs/heads/main/plugins/plugin.schema.json",
  "api_version": 1,
  "id": "com.example.plugin",
  "items": [{
    "id": "shape",
    "label": { "en-US": "Shape", "ja-JP": "図形" },
    "category": "shape",
    "icon": "square",
    "render": {
      "properties": [],
      "shader": { "module": "shape" },
      "bounds": {
        "script": "#{ min: [viewport.min.x, viewport.min.y], max: [viewport.max.x, viewport.max.y] }"
      }
    }
  }]
}
```

Save the declaration as `plugin.json`, then create `shape.wesl`:

```wesl
import package::generated::host::item::quad_corner;
import package::generated::support::util::scene_color;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    return vec4(quad_corner(vertex_index), 0.0, 1.0);
}

@fragment
fn fragment_main() -> @location(0) vec4<f32> {
    return scene_color(vec4(1.0, 0.3, 0.1, 1.0));
}
```

This draws a viewport-sized quad. Generate and validate the bundle with the
commands above.

## Manifest rules

`api_version` must be `1`. Define at least one item or effect. Validation rejects
unknown fields, invalid defaults, unresolved references, and shader mismatches.
Detailed-guide examples are declaration fragments.

Labels are BCP 47 locale maps. Lookup uses a case-insensitive exact match,
`en-US`, then the first translation by locale key; regional tags do not fall back
to language-only tags. Categories are host-defined and group entries across
plugins. Use the [JSON schema](../../plugins/plugin.schema.json) for allowed
categories and field definitions.

Items and effects require an `icon` ID from the application's bundled UI icons,
such as `square`, `media-video`, or `color-wheel`. The ID is the SVG filename
without `icons/` or `.svg`, and is case-sensitive. The application checks that the
icon exists when loading plugins and through `zerium plugin validate`. The same
icon is used in the addition picker and, for items, on the timeline.

## Catalog categories

Categories describe the operation's purpose. Their order is shared by every plugin
in the addition picker. Item categories are `shape`, `text`, `media`, `scene`
(cameras and render-result references), and `other`.

| Effect category | Purpose | Built-in examples |
| --- | --- | --- |
| `transform` | Placement, size, orientation, and arrangement | Translate, rotation, follow spline, tile layers |
| `distortion` | Warp image geometry | Wave warp, fisheye, corner pin, displacement map |
| `color` | Adjust or remap colors and tones | Exposure, levels, gradient map, posterize |
| `fill` | Apply a fill or outline | Color gradients, noise fill, outline |
| `blur` | Blur image details | Gaussian blur, motion blur, lens blur |
| `light` | Add light, shading, or shadows | Glow, bloom, light rays, drop shadow, vignette |
| `texture` | Change image detail or simulate a visual texture | Pixelate, dither, sharpen, halftone, glitch, grunge |
| `mask` | Remove image regions or change transparency | Crop, rounded corners, opacity, layer mask, chroma key |
| `transition` | Reveal or hide the image for an entrance or exit | Wipe, noise dissolve |
| `time` | Change which source time is sampled | Time remap |
| `other` | Operations outside the categories above | Custom plugin operations |

## Authoring guides

| Guide | Contents |
| --- | --- |
| [Properties and editor features](properties.md) | Scalar and collection types, animation settings, and preview/timeline editing |
| [Rendering and shaders](rendering.md) | Generated WESL API, image coordinates, output bounds, and effect passes |
| [Visual and audio inputs](inputs.md) | Media readers, playback, text, numbers, render results, and camera views |

The [bundled plugin](../../plugins/zerium.builtin/) provides complete examples.
For runtime ownership, see [Plugin loading and validation](../architecture/runtime.md#plugin-loading-and-validation).
