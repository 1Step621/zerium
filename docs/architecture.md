# Architecture

## Workspace and dependencies

The root is a virtual Cargo workspace. `cargo build` and `cargo run` select the
application by default; assets, locales, plugins, and packaging files stay at the
repository root.

```text
zerium (application, UI, media, audio, GPU rendering)
 ├──> zerium-shader (generation, linking, validation, plugin asset loading)
 │     └──> zerium-core
 └────────> zerium-core (models, editing, persistence representations)
```

- [`zerium`](../crates/zerium/src/) owns application construction, GPUI entities,
  device and filesystem adapters, and embedded resources.
- [`zerium-core`](../crates/zerium-core/src/) owns validated project, property,
  animation, and plugin models and their editing rules.
- [`zerium-shader`](../crates/zerium-shader/src/) owns shader contracts, packaged
  WESL generation, plugin asset loading, WESL linking, and WGSL validation. The
  application and plugin CLI use the same implementation.

Core and shader do not depend on GPUI, WGPU, FFmpeg, audio devices, or application
code. They are unpublished workspace libraries, not a stable SDK. Public items
serve cross-crate use; implementation helpers stay private or crate-visible.
The application is a binary-only crate; `main.rs` owns startup and CLI dispatch.

Within the application, `app` composes the runtime, `ui` translates user input
into core commands, and `engine` handles media, audio, rendering, export, and
project I/O. Engine code must not depend on UI entities.

## Project state and editing

`TimelineProject` holds persistent documents, reusable scenes, resolution, and
project identity. Persistent changes go through `TimelineEditor` to keep
validation, revision, and history consistent. Selection, preview visibility,
playhead, and history are session state, not part of saved projects.

Rendering and background work receive immutable `TimelineSnapshot`s and read
through `TimelineView`, never through live editors or UI entities.
`ProjectSession` tracks project generations and pending operations so results
from an old project can be ignored. `ProjectRuntime` groups the session handles
that must reset together when replacing a project.

Timeline positions, positive spans, and frame rates use `Frame`, `FrameDuration`,
and `FrameRate`. Items, layers, and effect instances have distinct ID types.
`PropertyPath` identifies a property, an optional stable array element ID, and
an optional tuple scalar index. Editing, animation, scene bindings, and
persistence use the same paths, so deleting or reordering an array element does
not redirect its animation to a neighbor.

Property schemas define types, defaults, constraints, and scalar permissions.
Commands validate all affected values and bindings before committing a
multi-owner edit. Animation tracks address individual scalars; interpolation
rules belong to the animation module.

## Plugins and persistence

Core parses and validates manifests, represents loaded bundles, and provides
registry lookup. Shader handles asset loading, generated-interface compatibility,
and executable shader validation. Embedding bundled plugins belongs to the
application. See [Plugin API v1](plugin.md) for manifest and shader contracts.

Project files store property overrides relative to plugin defaults. Loading
starts from the current validated defaults and applies those overrides. Project
and clipboard decoding validate external data before constructing core state.
Filesystem access and atomic replacement belong to `engine::project_io`.

## Rendering and export

FFmpeg handles probing, decoding, conversion, and encoding in process.
`MediaReaderRegistry` separates rendering from concrete media readers.

The application creates a shared `RenderRuntime` for preview and export. Each
consumer has an independent render session; compiled plugin shaders are shared.
Timeline evaluation preserves scene boundaries and sample time, including for
media and temporal effects. Item surfaces use declared bounds; scenes composite
into the viewport before applying scene effects.

## Localization

The application selects a BCP 47 locale from `ZERIUM_LANGUAGE`, then the system
locale, then `en-US`, and sets the shared `rust-i18n` locale at startup. Core label
accessors read that setting without locale arguments or application dependencies.
Explicit `LocalizedText::resolve_for(locale)` is used when a particular
translation is needed, including validation of all enum labels independently of
the display language. User-authored names remain plain text.

The UI supplies translated defaults for scene and argument names. Core owns
uniqueness, history, and the resulting document changes.

## Change rules

- Invalid external data returns a typed error; user-controlled input must not
  cause a panic.
- Preview-only changes do not increment project revision or enter history.
- Asynchronous work uses immutable snapshots and is cancelled or ignored after
  a project replacement.
