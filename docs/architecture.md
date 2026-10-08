# Architecture

## Workspace

```text
zerium (application)
 ├──> zerium-shader (plugin assets and shader compilation)
 │     └──> zerium-core
 └────────> zerium-core (models and editing)
```

- [`zerium`](../crates/zerium/src/) owns startup, CLI dispatch, GPUI entities,
  device and filesystem adapters, and embedded resources.
- [`zerium-core`](../crates/zerium-core/src/) owns project, property, animation,
  and plugin models, validation, and editing rules.
- [`zerium-shader`](../crates/zerium-shader/src/) owns plugin asset loading,
  generated WESL interfaces, linking, and shader validation. The application and
  plugin CLI use the same implementation.

Core and shader do not depend on GPUI, WGPU, FFmpeg, or application code.
Within the application, `app` composes the runtime, `ui` translates user input
into core commands, and `engine` handles media, audio, rendering, export, and
project I/O. Engine code must not depend on UI entities.

## Project state

`TimelineProject` holds persistent documents, scenes, resolution, and project
identity. `timeline::document` owns item storage and layer indexes; its
`placement` and `editing` modules handle interval operations and value
transactions without exposing those indexes. Persistent changes go through
`TimelineEditor`; the edit boundary in `timeline::history` records changed
commands and advances revision. Commands prepare fallible multi-owner changes
before committing them. Animation gestures keep their own starting snapshot
and grouping across updates. Selection, preview visibility, playhead, and
history are session state. Preview-only changes do not increment project revision
or enter history. Grouping inside a scene preserves argument connections
through inputs on the new nested scene, with constraints still applied by the
original targets.

Rendering and background work receive immutable `TimelineSnapshot`s and read
through `TimelineView`. `ProjectSession` tracks project generations so results
from a replaced project can be ignored. `ProjectController` owns file-operation
lifetimes and resets transient editor state when replacing a project. File
selection and I/O use one session operation from start to finish; dialogs and
workspace layout belong to `ui`.

Inspector controls are derived from schemas. Cached inputs own their event
subscriptions and use stable array element IDs, so reordering preserves focus
and deleting controls releases their state. Preview layout and interaction live
in `ui::preview::render`; frame preparation and presentation live in
`ui::preview::frame`.

Timeline positions, positive spans, and frame rates use `Frame`, `FrameDuration`,
and `FrameRate`. Items, layers, and effects have distinct ID types.
`PropertyPath` identifies a property, an optional stable array element ID, and
an optional tuple scalar index. Editing, animation, scene bindings, and
persistence share these paths, so reordering an array does not redirect its
animations or bindings.

Property schemas define types, defaults, constraints, and scalar permissions.
Stored items are read through `source_items_in_scope`; resolved reads use
`property_value` or `evaluated_property_value`. Whole-item resolution builds
each property through the same pipeline. `timeline::properties` resolves defaults
and scene arguments, then delegates per-property animation and constraints to the
animation module. Scene argument resolution accepts either definition defaults
or placed instance values. Baking arguments into detached items uses the same
resolver and stores only connected properties, preserving inheritance for
unbound scene inputs. Inspector, rendering, and audio use this evaluation path.
Owner and array-element matching uses resolved items so inherited values
participate consistently. Commands validate all affected values and bindings
before committing a multi-owner edit.
Animation tracks address individual scalars; interpolation belongs to the
animation module. A track may repeat its editable pattern with a frame period
and cycle phase. `AnimationClock` maps between pattern positions and timeline
time for evaluation, editing, and synchronization. Loop periods measure a full
cycle; ping-pong periods include both directions. Trim advances phase without
rewriting repeating patterns, while stretch scales their periods. Nonrepeating
tracks retain their first-to-last-frame timing. Preview position, size, and point
controls edit each scalar independently using authored stop values. Their short
markers use the current item geometry, with full axis guides only on hover or
drag. Unanimated axes edit their property values. Motion paths sample
complete playback intervals using the same property constraints as playback
and connect ordinary playback samples as a guide,
including loop and Hold jumps. Drawing fewer intervals preserves short-period
motion and leaves omitted intervals disconnected.
Invalid external data returns an error rather than panicking.

## Plugins and persistence

Core parses and validates manifests and provides registry lookup. Shader handles
bundle loading and shader compatibility. Embedding bundled plugins belongs to
the application. See [Plugin API v1](plugin.md) for the authoring contract.

Project files store property overrides relative to plugin defaults. Loading
applies validated overrides to the current defaults. Project and clipboard
input is validated before constructing core state; filesystem access and atomic
replacement belong to `engine::project_io`.

## Rendering

FFmpeg handles probing, decoding, conversion, and encoding in process.
`MediaReaderRegistry` connects media inputs to concrete readers. File import
preparation and metadata refresh belong to the metadata module.

Preview and export share a `RenderRuntime` and compiled plugin shaders, with
independent render sessions. `RendererDevice` owns immutable GPU state;
`FrameRenderer` owns each session's resource pools. Timeline evaluation preserves scene boundaries and
sample time, including for temporal effects. Item surfaces use declared bounds;
scenes composite into the viewport before applying scene effects.

## Localization

Startup selects the shared `rust-i18n` locale. Core label accessors use it;
`LocalizedText::resolve_for` supports an explicit locale. Label declarations and
fallbacks are described in [Plugin API v1](plugin.md#manifest).

User-authored names are plain text. The UI supplies translated default names;
core owns uniqueness and document edits.
