# Media, preview, and export

[Architecture](README.md) · [Editing](editing.md) · [Rendering](rendering.md) · **Media and export** · [Runtime](runtime.md)

## Reader contracts

[`engine/media`](../../crates/zerium/src/engine/media/) owns readers and timeline
mixing. Scene evaluation requests media by asset, source time, and raster size.
Preview and export own separate evaluators and decode sessions. Video selection
uses presentation timestamps; images are rasterized at the requested size.
Decoded frames own an `Arc<VisualFrame>`. Reusing a presentation interval keeps
the same image identity through preview and export, rather than wrapping the
same decoded pixels in a new allocation and invalidating GPU result caches.
Raster images, SVG, and text use the same byte-budgeted frame cache. Image
decoders key results by raster dimensions; text keys them by content and style
as well. Each cache retains at most 128 MiB of pixel data, evicting unused
versions in LRU order. Live graphs pin individual versions. If no room remains,
the caller can still use a new result without adding it to cache retention.
H.264 uses `gpu-video` when available, with a GStreamer software fallback.
`VideoDecodeSession` owns an explicit unopened/open state and the one-way fallback
policy. An open state owns a `PlaybackCursor`, which selects presentation intervals
and maintains the last and lookahead frames. `DecodeBackend` supplies decoded
frames and keyframe seeking from either GPU or CPU input. Replacing the session
state drops codec resources and presentation history together; frame selection
does not depend on an earlier configure call or a separate GPU preference flag.
An interrupted interval read retains its unconsumed picture, so cancellation
does not advance presentation when the same session resumes.
Cursor position distinguishes a known starting time, a completed frame interval,
and an unknown position after a failed seek. Cancellation before the first interval
therefore preserves the seek origin; requesting a different time, including zero,
positions the backend again instead of assuming an unopened stream.
The encoded input parser inserts SPS/PPS before every IDR picture, so a new
GPU decode session can start at a seeked keyframe without prior stream headers.
Encoded input also retains frames outside the requested GStreamer segment:
preroll IDRs and reference pictures must reach the decoder before selecting a
visible frame. Segment clipping belongs after decoding, in `PlaybackCursor`.
GPU input timestamps use the full segment-to-stream conversion, including
preroll before the segment start. A separate sample ID associates asynchronous
outputs with timing metadata, so repeated or negative presentation timestamps
do not change that identity. Frames before the media origin supply references
and are discarded only after decoding.
The GPU decoder also owns the consumer queue connection. On teardown it submits
and waits for outstanding queue dependencies while its synchronization resources
are still alive. Decoded pictures can be discarded without ever being rendered;
their imported semaphore waits must still finish before the decoder is released.
Normal frame decoding and publication do not wait for GPU completion.

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

[`PreviewFrameScheduler`](../../crates/zerium/src/engine/preview/) tracks current
scene demand, including temporal samples. `FrameDemand` owns frame selection,
cache retention, failure suppression and publication without worker I/O. The
scheduler selects requests and reconciles replies by generation. `DecodeWorker`
owns thread creation, its request channel, and cancellation; the scheduler never
constructs those transport details. Each decode
stream has at most one outstanding decode. Preview displays the requested frame or
retains a candidate while waiting; unchanged frames skip scene construction and
rendering. Texture preparation and
drawing share queue ordering without blocking the UI on GPU completion.
Each worker owns its decoder and shares a `VideoFrameUploader` with the other
workers. The uploader serializes conversion scratch access and submits uploads
before returning, without locking the UI's render session. Workers do not manage
GPU queue ordering or own scene-rendering resources. Cancellation releases their references
without joining a live thread on UI teardown. Unexpected
worker termination is reported against its pending request. A reply guard retains
that request until publication and emits one terminal result, including on panic;
an unfinished request does not contain a placeholder failure result.

`MediaInputId` identifies a placed source input. `DecodeStreamKey` adds a stable
sampling-consumer branch without changing that source identity. Every sampling
pass starts a branch, regardless of whether its coordinates use progress or frame
offsets. All samples of one temporal reduction share a branch and are scheduled
in source-time order in both preview and export, rather than opening one decoder
per sample. Original and
retimed images therefore retain independent decoder positions.

Preview's ready-frame cache shares images by asset, raster size and presentation
interval across streams. Both CPU and GPU image identities describe pixel
dependencies without decoder branches. A separate consumer-dependency DAG retains
each branch when image requests coincide, so their decoder positions remain
independent when clocks later diverge. Image-cache hits reuse that demand DAG
instead of rebuilding the image. Planned source requests acquire their decoder
stream during demand traversal, before frame binding. Preview and export release
decode sessions when their streams leave the current demand.

Preview tracks the presented candidate by the full request, including asset,
source time, raster size and decoder stream. Requests at the same time but with
different resolutions or animated assets therefore cannot overwrite one another's
redraw state. The ready cache stores `DecodedFrame` directly, and presentation
history stores only the consumed image's `Arc`. Redraw compares pixel identity;
timestamp and sequence metadata are not duplicated into a second frame type.

Decode failures belong to source requests. The displayed error is derived from
failures still present in current demand: a different source succeeding cannot
clear it, and removing the failed source removes its status. A decoded timestamp
gap suppresses repeated requests without becoming a decoder-error message.
Preview finalizes demand even if scene planning fails, so unused workers and
their decoder resources are released on that path too.

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
