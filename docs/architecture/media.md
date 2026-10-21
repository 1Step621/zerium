# Media, preview, and export

[Architecture](README.md) · [Editing](editing.md) · [Rendering](rendering.md) · **Media and export** · [Runtime](runtime.md)

| Module | Responsibility |
| --- | --- |
| [`engine/media`](../../crates/zerium/src/engine/media/) | Reader contracts, image/video/audio readers, and audio timelines |
| [`engine/frame`](../../crates/zerium/src/engine/frame/) | Shared visual frames, pixel planes, and color metadata |
| [`engine/gpu.rs`](../../crates/zerium/src/engine/gpu.rs) | GPU device/queue ownership and video-capable device creation |
| [`engine/gstreamer`](../../crates/zerium/src/engine/gstreamer/) | Discovery, demuxing, software decoding, and MP4 output |
| [`engine/preview`](../../crates/zerium/src/engine/preview/) | Decode scheduling and retained frames for interactive playback |
| [`engine/export`](../../crates/zerium/src/engine/export/) | Snapshot evaluation, ordered frame processing, and encoding |

## Reader contracts

Scene evaluation requests media by asset, source time, and raster size. Preview
and export own separate evaluators and decode sessions. Video selection uses
presentation timestamps; images are rasterized at the requested size.
Filesystem revision reads belong to the media runtime; core's `FileRevision`
stores only the observed metadata. `RasterSize` owns fitting source dimensions
within a requested image size.

Metadata discovery reads `MediaDependency` values from the snapshot, resolving
only file properties through nested scene arguments. It includes media inputs
on scene-instance effects without evaluating visual or gain calculations.
Background discovery includes unused scene definitions and default file values;
export discovery follows occupied root placements and their intersected clips.
Both feed the same reader refresh path.

H.264 decoding uses `gpu-video` when available, with a GStreamer software fallback.
GPU-decoded NV12 planes are converted to RGB for rendering. Audio readers feed a
shared timeline mixer; preview sends its output to CPAL and export to the encoder.
The mixer reads `AudioTimelineSnapshot` with the same audio plan and frame-based
gain evaluator for playback, export, and the meter. Preview captures the current
editing scope and visibility; export captures the root timeline. The plan expands
scene placements once and resolves only static decoder inputs. Only inputs with
a loaded audio stream request gains. Gain calculations run at integer video
frames; the mixer linearly interpolates their results between floor-rounded
sample boundaries. The final occupied frame holds its gain until the clip ends.
Frame gains are shared across blocks and calculation results are reused while
their inputs stay constant. The playback worker receives updated immutable
snapshots between blocks. The mixer owns source-change detection, decoder
reconciliation, and frame-cache invalidation; the UI and meter submit snapshots
without deciding which internal resources to replace. The worker retains
existing decoders by clip ID and asset identity, updates placement and playback
configuration in place, and opens only new or replaced assets before committing
the edit. The output stream and its clock continue through source changes.
Frame control ramps are interpolated directly while mixing decoded samples,
without allocating a sample-sized gain buffer for every clip. Calculations use
the render cursor rather than a UI-sampled gain. Preview retains authored gains
for failed calculations; export reports diagnostics from contributing frames.
Device failure signals the render worker to stop and invalidates its audio
clock immediately, allowing transport to continue with its wall clock.
Playback started at a video frame uses its exact rational, floor-rounded sample
boundary without converting through floating-point seconds. A deferred start
joins the continuous wall-clock position, also rounded down to a sample.

## Preview

`PreviewFrameScheduler` tracks demand from the current scene, including temporal
samples. Each input has at most one outstanding decode. Preview displays the
frame covering the requested time, or a retained candidate while waiting.
Changed candidates trigger redraws; unchanged frames skip scene construction and
GPU rendering. Texture preparation and drawing share a GPU queue to preserve
submission order without blocking the UI on GPU completion.

## Export

The UI controller uses `ExportState` to distinguish idle, destination selection,
and export execution. Each active state owns its operation and task; running
exports also own progress and cancellation. A project change cancels the export
and drops its state. The chosen output path persists independently within the
current project session.

Export freezes the timeline and processes every output frame in order. Evaluation,
decoding, rendering, and encoding overlap through bounded queues. Rendered output
is converted to limited-range BT.709 NV12. Encoding uses Vulkan Video on Linux and
Windows or VideoToolbox on macOS, with GStreamer's x264 encoder as the fallback
when a GPU encoder cannot be initialized. Later encoder failures are reported.

Each video backend owns its resources and a queue of its concrete pending-frame
type. `ExportVideoPipeline` dispatches submission and completion. Scene rendering
and NV12 color conversion are shared in `ColorConversion`.

GStreamer combines H.264 video and AAC audio into MP4. The completed temporary
file is published atomically.

The evaluated scene and GPU resource model are described in
[Rendering](rendering.md). Plugin authors declare sources through
[Visual and audio inputs](../plugin/inputs.md).
