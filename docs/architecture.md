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
detached hierarchical scene graph.

Timeline positions use `Frame`, positive spans use `FrameDuration`, and frame
rates use `FrameRate`. Items, layers, and effect instances use distinct ID
types. Array values use stable `PropertyElementId`s so animations and bindings do
not follow a neighboring element after deletion or reordering.

## Properties and animation

`domain::property` owns property types, checked values, constraints, schema
metadata, and plugin JSON adapters. Plugin manifests describe these contracts;
they do not own runtime property mutation.

Scene bindings and animation lookups address a property with an optional stable
array element ID and an optional tuple scalar index. Animation tracks are stored
hierarchically under property ID, array element ID, and tuple scalar index, so
each layer resolves only the identifier belonging to that layer. Scope values and
animation collections resolve property IDs; property values resolve array
elements; element values and animation groups resolve tuple scalar indices.

`domain::animation` owns scalar tracks, ordered value stops, interpolation,
and animation evaluation. A track contains one interpolation per adjacent stop
pair. Property validation determines whether a scalar can be animated; the
animation module owns interpolation and track rules.

`PropertyAddress` identifies an item, effect, property, array element, and
scalar independently of inspector widget identity. `InspectorPath` remains a
PropertyInspector-only key for row state; the inspector keeps editable scalar
coordinates separately. `ui::property_presentation` resolves numeric display
rules, while `ui::animation_presentation` resolves animation labels and source
addresses from a `PropertyAddress`. Neither shared module depends on an
inspector or curve component.

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
`shader`, `media`, `text`, and `render_result` can each produce a scene-linear
texture, and both item shaders and effect passes import them by ID from the
same generated capability module. The item's output shader is separate from
its inputs. Each effect instance owns its imported file assets. Item-only
`audio` and `editor` declarations stay outside shader capabilities. Rendering
resolves capability nodes before the owner shader or pass and binds them in
manifest order.
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
creates the dedicated export device; each consumer uses an independent render
session while sharing compiled plugin shaders. Timeline evaluation preserves
scene composition boundaries and carries sample time through media and
temporal passes.

## Change rules

- Invalid external data returns a typed error; user-controlled input must not
  cause a panic.
- Preview-only changes do not increment project revision or enter history.
- New asynchronous work accepts immutable snapshots and is cancelled or
  ignored after a project replacement.
- Domain modules remain crate-private until Zerium intentionally becomes a
  library; the public Rust API currently exposes only `zerium::run`.
