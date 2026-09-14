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

## Timeline

`TimelineEditor` is the only owner of live editing state. It coordinates:

- `TimelineDocument`: persistent items, layers, parameters, effects, scenes,
  and IDs;
- selection, preview visibility, and edit history: session-only state.

Commands that change persistent state go through `TimelineEditor`, which keeps
revision and history bookkeeping consistent. Background work receives an
immutable `TimelineSnapshot`/`TimelineView`, never a live editor or UI entity.

`TimelineDocument` owns indexes and document invariants. `TimelineItem` owns its
plugin or scene identity, parameters, effects, and geometry. Scene resolution
and argument binding live in `timeline::scene`; runtime evaluation produces a
detached hierarchical scene graph.

Timeline positions use `Frame`, positive spans use `FrameDuration`, and frame
rates use `FrameRate`. Items, layers, and effect instances use distinct ID
types. Array values use stable `ArrayElementId`s so animations and bindings do
not follow a neighboring element after deletion or reordering.

## Parameters and animation

`domain::parameter` owns parameter types, checked values, constraints, schema
metadata, and plugin JSON adapters. Plugin manifests describe these contracts;
they do not own runtime parameter mutation.

`ParameterValuePath` identifies a value inside one parameter using an optional
stable array element ID and an optional tuple element. Scene bindings and
animation addresses share this path. `ParameterAddress` adds the parameter ID
to the path and identifies one scalar animation track.

`domain::animation` owns scalar tracks, ordered value stops, interpolation,
and animation evaluation. A track contains one interpolation per adjacent stop
pair. Parameter validation determines whether a scalar can be animated; the
animation module owns interpolation and track rules.

## Plugins and persistence

The plugin boundary has three responsibilities:

- `manifest` parses and validates plugin documents;
- `bundle` resolves referenced shader assets and constructs validated bundles;
- `registry` provides deterministic lookup of completed bundles.

`plugin::validation` rejects contradictory schema and capability definitions.
The plugin ABI exposes ordered values and generated shader data; internal stable
array IDs are not exposed to plugins.

`persistence::project` and `persistence::clipboard` deserialize untrusted data,
validate it against the current domain contracts, and then construct domain
state. Filesystem access and atomic replacement stay in `engine::project_io`.

## Media, rendering, and export

FFmpeg libraries are used in process for probing, decoding, conversion, and
encoding. `MediaReaderRegistry` is the boundary between timeline/rendering code
and the concrete media adapter.

Rendering depends on the read-only `TimelineView`, not editing commands.
Preview and export use independent render sessions while sharing immutable
pipelines and the GPU device. Timeline evaluation preserves scene composition
boundaries and carries sample time through media and temporal passes.

## Change rules

- Invalid external data returns a typed error; user-controlled input must not
  cause a panic.
- Preview-only changes do not increment project revision or enter history.
- New asynchronous work accepts immutable snapshots and is cancelled or
  ignored after a project replacement.
- Domain modules remain crate-private until Zerium intentionally becomes a
  library; the public Rust API currently exposes only `zerium::run`.
