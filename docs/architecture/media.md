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

H.264 decoding uses `gpu-video` when available, with a GStreamer software fallback.
GPU-decoded NV12 planes are converted to RGB for rendering. Audio readers feed a
shared timeline mixer; preview sends its output to CPAL and export to the encoder.

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
