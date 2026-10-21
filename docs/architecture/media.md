# Media, preview, and export

[Architecture](README.md) · [Editing](editing.md) · [Rendering](rendering.md) · **Media and export** · [Runtime](runtime.md)

## Reader contracts

[`engine/media`](../../crates/zerium/src/engine/media/) owns readers and timeline
mixing. Scene evaluation requests media by asset, source time, and raster size.
Preview and export own separate evaluators and decode sessions. Video selection
uses presentation timestamps; images are rasterized at the requested size.
H.264 uses `gpu-video` when available, with a GStreamer software fallback.

Metadata discovery resolves only file properties through nested scene arguments,
including inputs on scene-instance effects. Background discovery includes unused
scene definitions and defaults; export follows occupied root placements and
intersected clips. Neither evaluates unrelated visual or gain calculations.
Filesystem revision reads belong to the runtime; core stores their observed
metadata.

## Audio

Playback, meters, and export share `AudioTimelineSnapshot` and one timeline mixer.
Preview captures the edited scene and visibility; export captures the root
timeline. The immutable plan expands placements and resolves static decoder
inputs. Only sources with loaded audio streams request gain evaluation.

Gain animations and calculations run at integer video frames. The mixer linearly
interpolates adjacent results between rational, floor-rounded sample boundaries,
including hold segments. Changes entirely between video frames are not
represented. The final occupied frame holds its gain until the clip ends.
Mixing is independent of block size. Preview retains authored gains on calculation
failure; export reports diagnostics from contributing frames.

Playback workers accept new snapshots at block boundaries. The mixer invalidates
gain caches and reconciles sources, preserving decoders for unchanged clip IDs
and assets. Replacements open before an update commits; placement changes retain
the output stream and clock.

CPAL plays the mix; export passes it to the encoder. Device failure stops the
worker and invalidates the audio clock immediately, letting transport continue
on its wall clock. Frame-based starts use exact rational sample boundaries;
starts during playback use the continuous position, rounded down to a sample.
Device discovery, decoder opening and initial buffering run in a preparation
worker. Transport holds the requested position until preparation finishes, then
switches to the audio clock or anchors the wall clock there. Cancellation removes
the preparation handle and signals its worker, so late results cannot start a
stream after stop or project replacement. An unchanged failed snapshot is not
retried on each UI tick. CPAL streams remain on their owning UI thread.

## Preview

[`PreviewFrameScheduler`](../../crates/zerium/src/engine/preview/) tracks current
scene demand, including temporal samples. `FrameDemand` owns frame selection,
cache retention, failure suppression and publication without worker I/O. The
scheduler owns worker channels, cancellation and generation checks. Each input
has at most one outstanding decode. Preview displays the requested frame or
retains a candidate while waiting; unchanged frames skip scene construction and
rendering. Texture preparation and
drawing share queue ordering without blocking the UI on GPU completion.
Each worker owns its decoder and upload renderer; cancellation releases those
resources on the worker without joining a live thread on UI teardown. Unexpected
worker termination is reported against its pending request.

## Export

The UI owns destination selection, progress, cancellation, and the background
task. Project replacement cancels active work. The output path persists within
the project session.

Export preserves media, audio timeline and visual calculation errors as distinct
typed causes. It freezes the timeline and processes output frames in order.
Evaluation, decoding, rendering, and encoding overlap through bounded queues. Output is
converted to limited-range BT.709 NV12. Encoding uses Vulkan Video on Linux and
Windows or VideoToolbox on macOS, with GStreamer's x264 fallback if a GPU encoder
cannot initialize. Later failures are reported.

`ExportVideoPipeline` dispatches backend submission and completion; rendering and
color conversion are shared. GStreamer combines H.264 video and AAC audio into
MP4. The completed temporary file is published atomically. A typed input-closure
result preserves the upstream failure that caused it; cancellation and writer
failures take precedence over resulting channel errors.

See [Rendering](rendering.md) for GPU ownership and
[Visual and audio inputs](../plugin/inputs.md) for plugin declarations.
