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

This item draws six vertices covering the composition viewport. Run the generate
and validate commands above to check the complete plugin.

## Manifest rules

`api_version` must be `1`, and a plugin must define at least one item or effect.
Validation rejects unknown fields, invalid defaults, unresolved references, and
shader mismatches. Examples in the detailed guides are declaration fragments.

Display labels are locale maps keyed by BCP 47 tags. Zerium selects
the `language` override in `config.json`, the system locale, or `en-US`, in that
order. Lookup tries a case-insensitive exact match, `en-US`, then the first
translation ordered by locale key; region tags do not fall back to language-only
tags.

Items select `shape`, `text`, `media`, `composite`, or `other`. Effects select
`transform`, `color`, `style`, `filter`, `composite`, or `other`.
Categories are host-defined and group entries across plugins.

## Authoring guides

| Guide | Contents |
| --- | --- |
| [Properties and editor features](plugin/properties.md) | Scalar and collection types, animation settings, and preview/timeline editing |
| [Rendering and shaders](plugin/rendering.md) | Generated WESL API, image coordinates, output bounds, and effect passes |
| [Visual and audio inputs](plugin/inputs.md) | Media readers, playback, text, numbers, render results, and camera views |

Use the JSON schema for field definitions and the guides for how those fields
work together. For runtime implementation and ownership, see
[Architecture](architecture.md#plugins).
