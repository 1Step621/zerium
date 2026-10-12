# Architecture

## Workspace

```text
zerium (application)
 ├──> zerium-shader (plugin assets and shader compilation)
 │     └──> zerium-core
 └────────> zerium-core (models and editing)
```

- [`zerium`](../crates/zerium/src/) owns startup, GPUI entities, platform adapters,
  and embedded resources.
- [`zerium-core`](../crates/zerium-core/src/) owns project, property, animation,
  and plugin models, validation, and editing rules.
- [`zerium-shader`](../crates/zerium-shader/src/) loads plugin assets, generates
  WESL interfaces, and validates and compiles shaders.

Core and shader do not depend on GPUI, wgpu, GStreamer, or application code.
Within the application, `app` composes the runtime, `ui` translates user input
into core commands, and `engine` handles media, rendering, export, and project
I/O. Engine code must not depend on UI entities.

## Editing and evaluation

Persistent changes go through `TimelineEditor`. Commands validate values and
bindings before committing, record history, and advance project revision.
Each gesture or paste is one undoable edit. Selection, preview visibility,
playhead, and history are session state; preview-only changes do not advance
project revision or enter history.

Editing, animation, scene bindings, and persistence share `PropertyPath`.
Array elements have stable IDs, so reordering preserves their animations and
bindings. The inspector's editing target is independent of timeline selection.

Property evaluation applies plugin defaults, scene arguments, animation, and
constraints through one pipeline shared by the inspector, rendering, and audio.
Animation tracks address individual scalars; `AnimationClock` maps pattern
positions to timeline time, including repeats. Beat guides affect editing only.

## Sessions and persistence

Rendering reads through `TimelineView`; background work receives immutable
`TimelineSnapshot`s. `ProjectSession` identifies project generations so results
from a replaced project can be ignored. `ProjectController` owns file-operation
lifetimes and resets transient state when replacing a project. Reopening a
project invalidates old edit targets.

Project files store property overrides relative to plugin defaults. Loading
validates input and applies overrides to current defaults. Filesystem access
and atomic replacement belong to `engine::project_io`.

Clipboard input uses the same validation as project input. Item and effect
pastes assign fresh IDs and go through the usual editing commands.

## Plugins

Core validates manifests and provides registry lookup. Shader loads bundles
and validates shader compatibility. The application embeds bundled plugins;
plugin CLI commands use the same implementation. See [Plugin API v1](plugin.md)
for the authoring contract.

## Media

| Module | Responsibility |
| --- | --- |
| [`engine/media`](../crates/zerium/src/engine/media/) | Reader contracts, image/video/audio readers, metadata, and audio timelines |
| [`engine/gstreamer/input`](../crates/zerium/src/engine/gstreamer/input/) | Audio/video discovery and persistent decoder pipelines |
| [`engine/gstreamer/gpu`](../crates/zerium/src/engine/gstreamer/gpu/) | Native GPU surface import and conversion into wgpu textures |
| [`engine/gstreamer/encoding`](../crates/zerium/src/engine/gstreamer/encoding/) | GPU/CPU encoding, audio encoding, and MP4 muxing |

Scene construction resolves media inputs into `MediaFrameRequest`s containing
an asset, source time, and raster size. Preview and export use the same
`VisualDecoder` implementation with independent sessions. Each session reuses
its decoder while the asset remains unchanged.

Video decodes at source resolution and selects frames by actual presentation
timestamps. Nearby forward requests continue decoding; backward and distant
requests seek to a preceding keyframe. Image readers honor requested raster sizes.

Decoder sessions receive the consumer's GPU device and validate native import
before publishing frames; unsupported surfaces select CPU output for the session.
Linux negotiates the VA driver's actual DMA-BUF modifier and checks the consumer's
support before import. Each uploaded video frame retains an owned GPU texture
so decoder surfaces can be released. Input transfer and export encoding select
their paths independently.

Audio readers feed a shared timeline mixer. Preview sends mixed audio to CPAL;
export sends it to the audio encoder.

## Rendering

[`engine/rendering`](../crates/zerium/src/engine/rendering/) builds scenes and
composites them with wgpu. Preview and export share compiled plugin shaders and
the rendering implementation. `RendererDevice` owns immutable GPU state;
`FrameRenderer` owns each consumer's independent resource pools.

Timeline evaluation preserves scene boundaries and temporal effect samples.
Effects and blending operate in scene-linear light with premultiplied alpha.
Each item's completed surface is blended into its parent after its effects;
scene-instance effects run after their children are composited.

## Preview

[`engine/video_playback.rs`](../crates/zerium/src/engine/video_playback.rs)
schedules only the frames needed by the current scene during playback,
scrubbing, and stopping. Each input has at most one outstanding decode. When
demand changes, that decode finishes and its result is cached before the next
request is selected from the latest demand.

A cache managed by a byte budget retains decoded frames. Preview displays the
frame covering the requested time, or the closest cached frame while waiting.
Workers prepare video textures before publishing results to the UI. The UI
updates demand and redraws when the displayed candidate changes; unchanged
frames skip scene construction and GPU rendering.

## Export

[`engine/export.rs`](../crates/zerium/src/engine/export.rs) freezes the timeline
at export start and processes every output frame in order. Scene evaluation
and decoding, GPU rendering, and encoding overlap through bounded queues.
Export owns its decoder and rendering sessions independently of preview.

The GPU converts rendered output to limited-range BT.709 NV12. Linux and Windows
use `gpu-video` for Vulkan Video encoding; macOS uses VideoToolbox. Unsupported
GPUs or encoder initialization failures select CPU readback and GStreamer's
x264 encoder. The backend stays fixed for the export; subsequent failures
are reported.

GStreamer combines compressed H.264 video and AAC audio into MP4. Audio timing
is adjusted to account for encoder priming. Output is written to a temporary
file and published atomically only after successful completion.

## Localization

Startup selects a shared `rust-i18n` locale for core labels and the UI.
User-authored names are plain text; translated defaults come from the UI.
Plugin label lookup is described in [Plugin API v1](plugin.md#manifest).

## Application updates

The app checks GitHub Releases in the background. Velopack applies pending
updates before startup on the next launch, preserving launch arguments.
Nix builds disable the `self-update` feature.
