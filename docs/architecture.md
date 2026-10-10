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
  generated WESL interfaces, linking, and shader validation.

Core and shader do not depend on GPUI, WGPU, FFmpeg, or application code.
Within the application, `app` composes the runtime, `ui` translates user input
into core commands, and `engine` handles media, rendering, export, and project
I/O. Engine code must not depend on UI entities.

## Editing and evaluation

Persistent changes go through `TimelineEditor`. Commands validate all affected
values and bindings before committing, then record history and advance project
revision. Each gesture or paste is one undoable edit. Selection, preview
visibility, playhead, and history are session state; preview-only changes do not
advance project revision or enter history.

Editing, animation, scene bindings, and persistence share `PropertyPath`.
Array elements have stable IDs, so reordering an array does not redirect its
animations or bindings. The inspector's displayed item is a local editing
target, independent of timeline selection and curve synchronization.

Property resolution applies plugin defaults, scene arguments, animation, and
constraints through one pipeline. Stored reads use `item`;
resolved reads use `property_value` or `evaluated_property_value`. Inspector,
rendering, and audio share this evaluation. Inspector controls come from
property schemas; editor extensions are declared separately in plugins.

Animation tracks address individual scalars. `AnimationClock` maps editable
pattern positions to timeline time, including repeats. Evaluation, editing, and
synchronization use this mapping. Beat guides affect editing only;
`ui::time_grid` shares their frame-rounded positions and snap rules between the
timeline and curve editor.

## Sessions and persistence

Rendering and background work receive immutable `TimelineSnapshot`s and read
through `TimelineView`. `ProjectSession` tracks project generations so results
from a replaced project can be ignored. `ProjectController` owns file-operation
lifetimes and resets transient editor state when replacing a project. Project
revisions are not reused when reopening a project, so old edit targets remain
invalid. File selection and I/O share one session operation from start to finish.

Project files store property overrides relative to plugin defaults. Loading
applies validated overrides to current defaults. Project and clipboard input
is validated before constructing core state; filesystem access and atomic
replacement belong to `engine::project_io`.

The system clipboard stores copied items. Effects and curve interpolation use
separate application-local buffers. Item and effect pastes assign fresh IDs;
all pastes use the usual command validation. Curve edits fix their synchronized
targets before mutation.

## Plugins and rendering

Core validates manifests and provides registry lookup. Shader loads bundles and
validates shader compatibility. The application embeds bundled plugins; plugin
CLI commands use the same shader implementation. See [Plugin API v1](plugin.md)
for the authoring contract.

FFmpeg handles probing, decoding, conversion, and encoding in process.
`MediaReaderRegistry` connects declared inputs to concrete readers. Visual and
audio playback use their own declared property references.

Preview and export share a `RenderRuntime` and compiled plugin shaders, with
independent render sessions. `RendererDevice` owns immutable GPU state;
`FrameRenderer` owns each session's resource pools. Timeline evaluation preserves
scene boundaries and sample time, including for temporal effects.

Blend mode is a persisted setting shared by all timeline items, separate from
plugin properties. Item surfaces use declared bounds; scenes composite into the
viewport before applying effects. Each item's completed surface is blended into
its parent scene after its effects, using scene-linear colors and premultiplied
alpha. Scene instances and render-result captures composite their children
against a transparent backdrop before participating in the parent scene.

## Localization

Startup selects the shared `rust-i18n` locale used by core label accessors and
the UI. User-authored names are plain text; the UI supplies translated defaults,
and core owns uniqueness and document edits. Plugin label declarations and
fallbacks are described in [Plugin API v1](plugin.md#manifest).

## Application updates

The app checks GitHub Releases and downloads updates in a detached background
task on launch. Velopack applies pending updates before normal startup on the
next launch, preserving launch arguments. Failures are logged and retried on a
later launch. Nix builds disable `self-update`, excluding the updater dependency;
feature selection stays at the app module boundary.
