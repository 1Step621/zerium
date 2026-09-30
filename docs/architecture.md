# Architecture

Zerium is a layered application. Dependencies point inward:

```text
app ──────> ui ──────> engine ──────> domain
 │          │                         ^
 └──────────┴─────────────────────────┘
```

`app` is the composition root. `ui` owns GPUI entities and translates user
input into domain commands. `engine` owns stateful adapters such as media,
audio, rendering, export, and project I/O. `domain` owns validated models and
business rules and must not depend on GPUI, WGPU, FFmpeg processes, or audio
devices. Engine code may read domain models, but must not depend on UI entities.
Application construction lives in `app::bootstrap`; `Workspace` composes the
top-level UI and forwards actions. UI components may share domain addresses and
application/runtime dependencies, but do not call sibling components for their
presentation or behavior.

`project_session::ProjectSession` is plain Rust state at the crate root, shared
by app and UI without reversing their dependency direction. It tracks project
generations and pending operations so stale load, save, import,
and export work can be ignored after a project change. GPUI entities notify
observers when they mutate the session. `app::ProjectRuntime` groups the editor,
transport, animation selection, and session handles that must be reset together
when a project is replaced. `app::ProjectController` handles project I/O and
settings and delegates that replacement reset to the runtime group.

## Timeline

`TimelineProject` is the persistent project state: its document, reusable
scenes, resolution, and project identity. The project is stored in
`domain::timeline::project` and is changed only through `TimelineEditor`.
`TimelineEditor` owns the editing session around that project and coordinates:

- `TimelineDocument`: persistent items, layers, properties, effects, scenes,
  and IDs;
- selection, preview visibility, and edit history: session-only state.

Commands that change persistent state go through `TimelineEditor`, which keeps
revision and history bookkeeping consistent. Selection, preview visibility,
playhead, and edit history remain editor session state. Background work
receives an immutable `TimelineSnapshot`; rendering and export read its
`TimelineView`, never a live editor or UI entity. Timeline editor mutations are
grouped in `domain::timeline::commands` by scene, session, and item concerns.

`TimelineDocument` owns indexes and document invariants. `TimelineItem` owns its
plugin or scene identity, properties, effects, and geometry. Scene resolution
and argument binding live in `timeline::scene`; runtime evaluation produces a
detached hierarchical scene graph. Each evaluated node retains its timeline item
and children; its item kind is the sole source of plugin/scene identity. Scene
instance spans are independent of the source scene duration; source times beyond
its end produce no content. Editing a source duration does not resize other
instances or parent scenes.

Timeline positions use `Frame`, positive spans use `FrameDuration`, and frame
rates use `FrameRate`. Items, layers, and effect instances use distinct ID
types. Array values use stable `PropertyElementId`s so animations and bindings do
not follow a neighboring element after deletion or reordering.

## Properties and animation

`domain::property` owns property types, checked values, constraints, schema
metadata, and plugin JSON adapters. Plugin manifests describe these contracts;
they do not own runtime property mutation.

`PropertyPath` identifies a property with an optional stable array element ID
and an optional tuple scalar index. Animation tracks use a flat map keyed by
this path, and scene binding targets store the same path. Shared value traversal
and schema resolution validate the scalar and preserve neighboring values when
editing it. Inspector lookups, animation evaluation, binding projection, and
persistence validation use these common operations.

`domain::animation` owns scalar tracks, ordered value stops, interpolation,
and animation evaluation. A track contains one interpolation per adjacent stop
pair. Custom handles belong to `CubicBezier`, rather than optional operations
on other interpolation modes. Property validation determines whether a scalar can
be animated; the animation module owns interpolation and track rules.

`PropertyAddress` identifies an item, effect, property, array element, and
scalar independently of inspector widget identity. `InspectorPath` remains a
PropertyInspector-only key for row state; the inspector keeps editable scalar
coordinates separately. `ui::numeric_property` resolves numeric display
rules, while `ui::animation_curve::presentation` resolves animation labels from
a `PropertyAddress`. Both use the same numeric presentation metadata. Inspector
text inputs and color pickers are stored by their concrete widget types.
Aspect-ratio locking constrains direct size edits only; animation tracks and scene
arguments keep their own values.

## Plugins and persistence

The plugin boundary has three responsibilities:

- `manifest` parses and validates plugin documents;
- `bundle` resolves referenced shader assets and constructs validated bundles;
- `registry` provides deterministic lookup of completed bundles.

`plugin::validation` rejects contradictory schema and capability definitions.
The plugin ABI exposes ordered values and generated shader data; internal stable
array IDs are not exposed to plugins.

Plugin item and effect schemas own the property contracts. `PropertyValues`
stores only current values; edits and loading validate them against the owning
schema. Project files record
property values that differ from the plugin defaults; loading starts from the
current validated defaults and applies those overrides. Scene instances keep
their sparse argument overrides. The same checked item representation is used
for project files and the timeline clipboard.

Item and effect schemas share an ordered array of named shader inputs.
`media`, `text`, and `render_result` can each produce a scene-linear
texture, and both item shaders and effect passes import them by ID from the
same generated capability module. The item's output shader is separate from
its inputs. Each effect instance owns its imported file assets. Item-only
`audio` and `editor` declarations stay outside shader capabilities. Rendering
resolves capability nodes before the owner shader or pass and binds them in
manifest order.
Scene arguments contain property contracts and bindings; evaluation applies
their instance values without expression evaluation.
`persistence::project` and `persistence::clipboard` deserialize untrusted data,
validate it against the current domain contracts, and then construct domain
state. Filesystem access and atomic replacement stay in `engine::project_io`.

## Media, rendering, and export

FFmpeg libraries are used in process for probing, decoding, conversion, and
encoding. `MediaReaderRegistry` is the boundary between timeline/rendering code
and the concrete media adapter.

Rendering depends on the read-only `TimelineView`, not editing commands.
`engine::rendering::RenderRuntime` is constructed by the app composition root
and shared by Preview and Export. It owns the preview renderer and lazily
creates the dedicated export device. Preview initialization is retained as one
result, keeping success and error states mutually exclusive. Each consumer uses
an independent render session while sharing compiled plugin shaders. Timeline
evaluation preserves scene composition boundaries and carries sample time through
media and temporal passes.
The encoder assigns every node a logical output rectangle. Item shaders begin
with their declared bounds; each item effect transforms those bounds. A scene
composite and every scene effect use the viewport rectangle. The GPU renderer
returns a texture and rectangle for each node, then maps child rectangles into
the scene composite. Compositing into the viewport clips item pixels there.
Temporal passes select source frames through either a uniform range or an
explicit array of frame offsets; their reducer shader combines the samples.
`SceneBuilder` constructs an arena of nodes referenced by IDs. Normal drawing,
captures, and temporal samples reuse those IDs; GPU encoding lowers each node
once. Item and scene effects use the same temporal sampling pipeline, with
shared depth and sample limits. Evaluation and composition plans are cached by
sample time and scene scope.
`SceneCompositionPlan` resolves normal visibility and `render_result` capture
membership once per evaluated scene scope, including scopes evaluated at a
temporal sample time.

## Change rules

- Invalid external data returns a typed error; user-controlled input must not
  cause a panic.
- Preview-only changes do not increment project revision or enter history.
- New asynchronous work accepts immutable snapshots and is cancelled or
  ignored after a project replacement.
- Domain modules remain crate-private until Zerium intentionally becomes a
  library; the public Rust API currently exposes only `zerium::run`.
