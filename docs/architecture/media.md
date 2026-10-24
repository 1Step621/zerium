# Media, preview, and export

[Architecture](README.md) · [Editing](editing.md) · [Rendering](rendering.md) · **Media and export** · [Runtime](runtime.md)

## Reader contracts

[`engine/media`](../../crates/zerium/src/engine/media/) owns readers and timeline
mixing. Scene evaluation requests media by asset, source time, and raster size.
Preview and export own separate evaluators and decode sessions. Video selection
uses presentation timestamps; images are rasterized at the requested size.
Decoded frames retain the same `Arc<VisualFrame>` identity when reusing a
presentation interval. Raster images, SVG, and text share a byte-budgeted cache
implementation; keys include raster size and, for text, content and style.
Live consumers retain their pixel versions, while unused entries can be evicted.

H.264 uses `gpu-video` when available, with a GStreamer software fallback.
`VideoDecodeSession` owns decoder lifetime and the fallback policy;
`PlaybackCursor` selects presentation intervals, while `DecodeBackend` supplies
frames and keyframe seeking. Encoded GPU input preserves the keyframe headers and
reference-picture preroll needed to decode before selecting a visible frame.
GPU decoder teardown retains synchronization resources until outstanding consumer
queue dependencies have completed.

Metadata discovery resolves only file properties through nested scene arguments,
including inputs on scene-instance effects. Background discovery includes unused
scene definitions and defaults. Export discovers visual files throughout the
definitions reachable from root instances, including child sources beyond a
parent's output clip that a remapped capture can consume. Audio discovery follows
occupied, intersected clips because visual sampling does not retime audio.
Neither evaluates unrelated visual or gain calculations.
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

[`PreviewFrameScheduler`](../../crates/zerium/src/engine/preview/) tracks scene
demand, including temporal samples. `FrameDemand` owns frame selection, cache
retention and failure reporting. The scheduler selects requests and reconciles
replies by generation; `DecodeWorker` owns its thread, request channel and
cancellation. Each decode stream has at most one outstanding request.

Preview displays the requested frame or retains a candidate while waiting.
Unchanged images skip scene construction and rendering. Workers share a
`VideoFrameUploader` that serializes conversion scratch access and submits
uploads before publication. Drawing uses the same GPU queue ordering. Worker
cancellation releases resources on the worker thread without blocking UI teardown;
unexpected termination is reported against the pending request.

`MediaInputId` identifies a placed source input. `DecodeStreamKey` adds its sampling
consumer branch, so original and retimed images can keep independent decoder
positions. All samples of one temporal reduction share a branch and are requested
in source-time order in both preview and export. Decode sessions are released when
their streams leave current demand.

The ready-frame cache shares images by asset, raster size and presentation
interval across streams. Presentation state belongs to the full request, including
its decoder stream, and redraw compares image identity. Decode failures remain
associated with current demand; removing a failed source removes its error status.

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
