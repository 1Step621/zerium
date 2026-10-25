# Properties and editor features

[Plugin API v1](README.md) · **Properties** · [Rendering](rendering.md) · [Inputs](inputs.md)

## Properties

Properties appear in declaration order. Inputs and editor features reference
their IDs; shaders use the generated [property interface](rendering.md#property-access).

Scalar types are `f32`, `i32`, `u32`, `bool`, `color`, `string`, `file`, and finite
`enum` choices. A scalar declares its type, plain default, and editing settings.
Tuples contain 2–64 scalars; arrays contain scalars or tuples. Collections do not
nest further.

```json
{
  "id": "position",
  "label": { "en-US": "Position" },
  "type": "tuple",
  "elements": [
    { "type": "f32", "default": 0, "animatable": true, "ui": { "label": { "en-US": "X" }, "unit": "px" } },
    { "type": "f32", "default": 0, "animatable": true, "ui": { "label": { "en-US": "Y" }, "unit": "px" } }
  ]
}
```

Settings belong on each scalar, including tuple and array-element scalars:

| Setting | Default | Purpose |
| --- | --- | --- |
| `editable` | `true` | Allow direct and animation edits |
| `animatable` | `false` | Allow numeric or color animation |
| `scene_bindable` | `true` | Allow connections to scene variables |
| `constraints` | Unrestricted | Bounds for defaults, edits, loaded values, and animation keys |
| `ui` | Scalar defaults | Labels, units, visibility, steps, and editor hints |

Units are shared by projects, shaders, and controls. `ui.step` sets the button
increment; `ui.drag_step` sets the change per horizontal pixel. Missing tuple
scalar labels use one-based indices. A tuple is visible if any scalar is visible;
color is one scalar.

### Arrays

Arrays require `element`, `max_items`, and a plain `default` array. `min_items`
defaults to zero. The element declaration supplies defaults for added rows:

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

All defaults must satisfy their types and constraints. Scene connections target
individual scalars, not whole arrays. Shaders receive values in array order.
String arrays use text inputs; `ui.editor: "font_family"` selects a system-font
picker for fallback fonts.

### Enums and animation

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

Values and displayed choices must be unique. Unlabeled choices use the numeric
value. Shaders represent enums as `u32`.

Animation targets individual scalars. Numbers and colors interpolate; other
types and array structure stay static. Integers round after interpolation;
colors share one curve across RGBA.

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

Files are optional paths saved relative to the project when possible.
`ui.extensions` suggests selection/import extensions without restricting values.
`ui.directory: true` uses a folder picker instead; it cannot be combined with
`ui.extensions`. Directory properties retain the same optional path value,
project-relative persistence, scene bindings, and Undo behavior as files.
Unset or unavailable files are allowed. Files may occur in collections, but
media and audio inputs reference standalone file properties and declare their
own readers.

## Editor features

Items and effects declare features in `editor`. Each kind can occur once;
`label` is item-only.

| Type | References | Behavior |
| --- | --- | --- |
| `position` | `property` | Preview position handle |
| `size` | `property`, `position` | Preview size handles |
| `aspect_lock` | `property` | Ratio-lock toggle; optional `default` is false |
| `points` | `property`, `position`, `size` | Vertex handles |
| `spline` | `points`, `position`, `size`, `tension`, `closed` | Preview curve |
| `label` | `property` | Item label from a string |

Position, size, and aspect-lock properties are two-`f32` tuples. Points are an
array of such tuples expressed as percentages: `[0, 0]` is top-left and
`[100, 100]` bottom-right. Spline tension is `f32`, closed is `bool`; combine
`spline` with `points` to edit its vertices. Ratio locking affects direct edits;
animation and bindings remain independent.

Size, points, spline, and media placement accept an optional `origin`: two enums
containing exactly `0`, `1`, and `2` for start/center/end on each axis. Omission
means center. Position identifies that origin and stays fixed when origin or
size changes. Use `item::placement_center(position, size, origin)` in shaders and
the matching [bounds helper](rendering.md#placement-and-projection).

Playback is declared once on the item; see [Playback and audio](inputs.md#playback-and-audio).
Its source-start, source-duration, speed, and EOF fields reference ordinary item
properties. They appear in the Inspector through their property declarations.
Playback items derive their frame duration from these values. Trimming changes
the source range; stretching changes speed. Items without playback retain a fixed
frame duration. Editor tools do not declare or own playback settings.
