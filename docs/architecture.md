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
identity. Persistent changes go through `TimelineEditor` to keep validation,
revision, and history consistent. Selection, preview visibility, playhead, and
history are session state. Preview-only changes do not increment project revision
or enter history.

Rendering and background work receive immutable `TimelineSnapshot`s and read
through `TimelineView`. `ProjectSession` tracks project generations so results
from a replaced project can be ignored. `ProjectRuntime` groups the handles that
reset together on project replacement.

Timeline positions, positive spans, and frame rates use `Frame`, `FrameDuration`,
and `FrameRate`. Items, layers, and effects have distinct ID types.
`PropertyPath` identifies a property, an optional stable array element ID, and
an optional tuple scalar index. Editing, animation, scene bindings, and
persistence share these paths, so reordering an array does not redirect its
animations or bindings.

Property schemas define types, defaults, constraints, and scalar permissions.
Commands validate all affected values and bindings before committing a multi-owner edit.
Animation tracks address individual scalars; interpolation belongs to the
animation module. Invalid external data returns an error rather than panicking.

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
`MediaReaderRegistry` connects media inputs to concrete readers.

Preview and export share a `RenderRuntime` and compiled plugin shaders, with
independent render sessions. Timeline evaluation preserves scene boundaries and
sample time, including for temporal effects. Item surfaces use declared bounds;
scenes composite into the viewport before applying scene effects.

## Localization

Startup selects the shared `rust-i18n` locale. Core label accessors use it;
`LocalizedText::resolve_for` supports an explicit locale. Label declarations and
fallbacks are described in [Plugin API v1](plugin.md#manifest).

User-authored names are plain text. The UI supplies translated default names;
core owns uniqueness and document edits.
