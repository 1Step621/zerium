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
values and bindings before committing; history records changes and advances
project revision. Animation gestures group their updates into one edit.
Selection, preview visibility, playhead, and history are session state.
Preview-only changes do not increment project revision or enter history.

Timeline positions, positive spans, and frame rates use `Frame`, `FrameDuration`,
and `FrameRate`. Items, layers, and effects have distinct ID types.
`PropertyPath` identifies a property, an optional stable array element ID, and
an optional tuple scalar index. Editing, animation, scene bindings, and
persistence share these paths, so reordering an array does not redirect its
animations or bindings.

Property resolution applies plugin defaults, scene arguments, animation, and
constraints through one pipeline. Stored reads use `source_items_in_scope`;
resolved reads use `property_value` or `evaluated_property_value`. Inspector,
rendering, and audio share this evaluation. Inspector controls come from
property schemas; editor extensions are declared separately in plugins.

Animation tracks address individual scalars. `AnimationClock` maps editable
pattern positions to timeline time for evaluation, editing, and synchronization,
including loop and ping-pong repeats. Beat guides affect editing only, without
retiming content. `ui::time_grid` shares frame-rounded guide positions and snap
rules between the timeline and curve editor.

## Sessions and persistence

Rendering and background work receive immutable `TimelineSnapshot`s and read
through `TimelineView`. `ProjectSession` tracks project generations so results
from a replaced project can be ignored. `ProjectController` owns file-operation
lifetimes and resets transient editor state when replacing a project. File
selection and I/O use one session operation from start to finish; dialogs and
workspace layout belong to `ui`.

Project files store property overrides relative to plugin defaults. Loading
applies validated overrides to current defaults. Project and clipboard input
is validated before constructing core state; filesystem access and atomic
replacement belong to `engine::project_io`.

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
scene boundaries and sample time, including for temporal effects. Item surfaces
use declared bounds; scenes composite into the viewport before applying effects.

Surface resampling preserves unchanged pixels, uses bilinear filtering for
magnification, and averages source texels by their overlap with the output-pixel
footprint for minification. Rotated footprints use an axis-aligned bounding box;
pixels outside the source surface are transparent.

Blend mode is a persisted setting shared by all timeline items, separate from
plugin properties. The inspector header edits it for its current item scope.
Each item's completed surface is blended into its parent scene after its
effects, using scene-linear colors and premultiplied alpha. Scene instances and
render-result captures composite their children against a transparent backdrop;
their completed image then participates in the containing scene's composition.

## Localization

Startup selects the shared `rust-i18n` locale. Core label accessors use it;
`LocalizedText::resolve_for` supports an explicit locale. Label declarations and
fallbacks are described in [Plugin API v1](plugin.md#manifest).

User-authored names are plain text. The UI supplies translated default names;
core owns uniqueness and document edits.
