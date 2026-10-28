# Media, preview, and export

[Architecture](README.md) · [Editing](editing.md) · [Rendering](rendering.md) · **Media and export** · [Runtime](runtime.md)

## Reader contracts

[`engine/input`](../../crates/zerium/src/engine/input/) owns source discovery and
audio/visual reader contracts. Native-library adapters live with their readers.
Scene evaluation requests media by asset, source time, and raster size.
Reader IDs resolve directly to the bundled implementations. Selection is stateless;
only metadata observations and decoder sessions retain consumer state.
Preview and export own separate evaluators and decode sessions. Video selection
uses presentation timestamps; images are rasterized at the requested size.
Decoded frames retain the same `Arc<VisualFrame>` identity when reusing a
presentation interval. Raster images, SVG, and text share a byte-budgeted cache
implementation; keys include raster size and, for text, content and style.
Live consumers retain their pixel versions, while unused entries can be evicted.

Image sequences share raster decoding and caching with still images. Their source
FPS is authored independently of project cadence; frame selection uses rational
presentation intervals. Directory revisions track image names, sizes, and
modification times without changing authored intervals. See
[Image sequences](../plugin/inputs.md#image-sequences) for ordering and input rules.

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
Each reader observes its source revision; metadata refreshes when that revision
changes. Revisions belong to each reader/target observation,
so one interpretation cannot invalidate another interpretation of the same path.
An unreadable replacement invalidates only its failed interpretation; missing
sources retain their previous observation. Failures are retried when the source
changes or its file property is selected again. Background results are committed
only while their baseline observation is current, so they cannot overwrite a
newer import. Core stores observations without filesystem access.
Observations and revisions exist only in runtime snapshots and caches. Project
files and clipboard payloads carry file references and authored timing without
metadata; loading and pasting trigger discovery through the same dependency path.

Import discovery has the same failure policy for every source. The first media
selection on an item initializes its size and temporal range through the same
document operation as a file drop. Picker presentation does not determine import
semantics.

## Audio

[`engine/processing/audio`](../../crates/zerium/src/engine/processing/audio/) owns
the timeline mixer and metering; [`engine/output/audio`](../../crates/zerium/src/engine/output/audio/)
owns live device playback. Playback, meters, and export share
`AudioTimelineSnapshot` and one timeline mixer.
Preview captures the edited scene; video export captures Main. Both respect
the saved visibility of every scene, including nested items and layers. The
immutable plan expands placements and resolves static decoder inputs. Only
sources with loaded audio streams request gain evaluation.

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
Output callbacks fill temporary buffer shortages with silence; only decoder,
device, and worker failures produce playback notifications.

The playback engine owns transitions between idle, preparation, and playback.
A session owns its device stream and mixer worker; device callbacks and the
worker share one output state for clock, levels, cancellation, and events.
Retired workers keep their completion state until they can be joined without
blocking the UI.

## Preview

[`PreviewFrameScheduler`](../../crates/zerium/src/app/preview/) tracks scene
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
task. [`app/export.rs`](../../crates/zerium/src/app/export.rs) orchestrates the
frozen timeline workflow through engine input, processing, and output stages.
Project replacement cancels active work. Destination, container, codecs, color,
bitrates, and audio format belong to the project and are saved with it.
Project settings apply these together with resolution and frame rate as one undoable
change. The export dialog only confirms those settings and edits a temporary
destination draft, committed when starting export; cancellation discards it.
Export always covers the entire root timeline, starting at frame zero.

Export preserves media, audio timeline and visual calculation errors as distinct
typed causes. It freezes the timeline and processes output frames in order.
Evaluation, decoding, rendering, and encoding overlap through bounded queues.
Export consumes RGBA16Float scene-linear output before the display transform.
The selected color standard, range, and precision determine conversion to NV12
(8-bit) or P010 (10-bit). BT.709 and BT.2020 SDR use their respective transfer
functions; BT.2100 PQ/HLG use BT.2020 primaries, a 203 cd/m² reference white, and
for HLG a 1000 cd/m² reference display with system gamma 1.2. Colorimetry is passed
to the encoder alongside the converted samples. HDR requires 10-bit output.
These settings control export; the preview retains its existing sRGB presentation
and media input decoding keeps its existing color handling.

Vulkan Video on Linux/Windows and VideoToolbox on macOS support H.264/H.265
with 8-bit BT.709 output and either range. Automatic selection falls back to
GStreamer software encoding for other color settings or unsupported devices.
VP9 and AV1 use software encoding. 10-bit AV1 uses SVT-AV1 (minimum 64 × 64);
8-bit AV1 uses libaom. Encoder selection is automatic and is not stored in the
project. Native encoder initialization failures fall back to software encoding;
failures after encoding starts are reported.

`VideoOutput` dispatches backend submission and completion; rendering and
color conversion are shared. Raw frames carry their NV12/P010 layout explicitly
into the encoder adapter. GStreamer encodes and muxes MP4, Matroska, or WebM.
GIF uses the same render and readback pipeline with straight sRGB RGBA frames,
then writes through the Rust `gif` encoder. It has no audio, video bitrate, or
selectable color settings; video settings remain available when changing formats.
Looping defaults to enabled. Frame boundaries are rounded to centiseconds without
accumulating timing drift; frames shorter than that precision may be skipped.
Transparency is reduced to GIF's binary alpha, clearing each frame before the next.
Consecutive identical images share one frame and its combined duration. Color
quantization and LZW compression run in bounded parallel batches; file writes
preserve frame order. The quantizer settings are unchanged by parallel execution.
Container compatibility is defined alongside the persisted settings; the settings
UI offers MP4 with AAC, Matroska with Opus or FLAC, and WebM with Opus. These
combinations preserve encoder delay and discard padding. Opus uses 48 kHz; FLAC
has no bitrate control. The MP4/AAC timing correction applies only to that combination.
The completed temporary file is published atomically. A typed input-closure
result preserves the upstream failure that caused it; cancellation and writer
failures take precedence over resulting channel errors.

The preview context menu saves or copies its current frame at project resolution.
It freezes the edited scene and visibility when opened, then decodes and
renders independently in the background. PNG output shares the RGBA conversion
and readback path, preserving transparency without editor overlays. File saves
use the same atomic transaction as video export; copying uses GPUI's image clipboard.

See [Rendering](rendering.md) for GPU ownership and
[Visual and audio inputs](../plugin/inputs.md) for plugin declarations.
