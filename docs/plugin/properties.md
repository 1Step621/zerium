# Properties and editor features

[Plugin API v1](README.md) · **Properties** · [Rendering](rendering.md) · [Inputs](inputs.md)

## Properties

The inspector displays properties in declaration order. Inputs and editor features
reference these declarations. Their shader representation is described in
[Property access](rendering.md#property-access).

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
same offset; see [Render surfaces and bounds](rendering.md#render-surfaces-and-bounds).

A `timeline` feature uses the three [time-mapping references](inputs.md#playback-and-audio)
to derive clip duration from source duration and speed. Changing duration keeps
speed fixed; changing speed keeps the source interval fixed. Trim/stretch update
these three properties and validate their constraints and clip placement.
Without this feature, source-property edits do not change clip length, and
timeline trim/stretch affect placement and animation timing only.
