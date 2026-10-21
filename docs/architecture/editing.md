# Editing and project state

[Architecture](README.md) · **Editing** · [Rendering](rendering.md) · [Media and export](media.md) · [Runtime](runtime.md)

## Transactions and read models

Persistent changes go through `TimelineEditor`, which validates edits, records
history, and advances the project revision. Selection, preview visibility,
playhead, and history are session state. Rendering reads a `TimelineView`;
background work receives immutable `TimelineSnapshot`s.

Property and resize commands prepare detached `ItemEdit` candidates. Dependent
timing, playback settings, and placement are checked before a batch is published;
failed batches leave the document and history unchanged. Preview value gestures
use this transaction boundary for all property axes.

## Property contracts

Editing, animation, scene bindings, and persistence share `PropertyPath`. Array
elements have stable IDs so reordering preserves their animations and bindings.
`PropertyAddress` identifies a value; `TimelineEditor` resolves its schema using
the current project and scene contracts.

Property evaluation applies plugin defaults, scene variables, animation, and
constraints through a pipeline shared by the inspector, rendering, and audio.
`SceneVariable` owns normalization of its scalar contract and updates to its
defaults and numeric settings.

### Scene variables and calculations

Scene definitions own a single collection of `SceneVariable` outputs. Its typed
source distinguishes arguments, which own instance property schemas and defaults,
from calculations, which have no default value or instance editing metadata.
Argument input schemas validate authored values and keys independently of their
evaluation limits. Defaults, instance overrides, and animation keys retain their
original values when limits change, including through persistence and clipboard
operations. Evaluation samples the original curve, then clamps the argument
before dependent calculations or property bindings consume it. Restoring a limit
restores the original values and motion. Numeric controls show authored values
and display the applied value when the current sample is limited.
Calculations are private to the definition and have an explicit output type, a
Rhai script, and named inputs connected to other variable IDs. Display names and UI ordering
never determine references or evaluation order. Reference IDs are generated
independently of display ordinals and are never reused by Undo or reload.
Property bindings can select
either kind of output; connected properties cannot also own animation tracks.

Property readers request only the connected outputs they consume. Their
transitive inputs are evaluated iteratively, including arguments inherited from
parent scene instances. Each calculation caches results by its own connected
input values, so changing an unrelated argument does not execute it again.
Cache locks protect lookups and insertion, never script execution. Dependency
plans and bounded per-calculation caches belong to an immutable scene revision;
mutating variables detaches this derived state without invalidating history.
`SceneDefinition::evaluate_variables` explicitly evaluates all variables for the
authoring UI and shares those complete results as well. Each script receives its
connected inputs in a fresh scope; local changes never persist between samples.
Results are checked against the declared scalar type, then destination property
constraints apply when binding. Numeric, integer, boolean, text, and color outputs
are supported; integer results require explicit conversion from floating point.
Colors use four-component arrays, with `rgba(r, g, b, a)` as a convenience.

Editing and loading reject missing inputs, duplicate or invalid local names,
and dependency cycles. Referenced variables cannot be deleted until their inputs
are disconnected. Invalid scripts remain editable and saveable; evaluation errors
propagate to dependent calculations, including through nested scene arguments.
Diagnostics follow property connections: an unused calculation, unused nested
argument, or inactive destination does not stop export. Preview uses the
destination's authored value for failed bindings; export stops when an output it
consumes depends on a failed calculation, reporting its source and evaluation
time. Each property resolution returns its value, including partial authored
fallbacks, together with diagnostics from those same connections. Rendering
collects these diagnostics while resolving its properties, audio reads them with
its gain, and nested arguments propagate failed inputs from the same result.
Consumers do not search the bindings again to discover errors.
Item rendering resolves only `render.properties`, the same contract used
by shader packing, bounds, and visual inputs. Audio-only controls cannot execute
or report diagnostics through visual temporal evaluation.
Temporal image requests also limit the effect chain before property resolution;
calculations connected only to omitted effects do not execute for those samples.
Export checks calculation errors on nodes actually consumed by output or capture
dependencies.

`AudioTimelineSnapshot` captures either the root export timeline or the currently
edited scene and preview visibility. It builds one immutable audio
plan containing expanded placements, static decoder sources, and gain bindings.
Its node variants distinguish scene scopes from audio sources; gain property
contracts are resolved when compiling the plan.
Preview visibility applies only to the edited composition, never to similarly
numbered layers or items inside nested instances. Source planning resolves only
decoder properties, without running volume or visual calculations.

Playback, meters, and export use the same frame-based gain evaluator. Audio
consumers request only decoder-backed input IDs, so missing streams and unset
file inputs do not evaluate gains or report calculation errors. Gain animations
and their connected calculations are evaluated at integer video frames. The
mixer linearly interpolates adjacent frame gains in the sample domain; branches
and hold animation segments follow this same interpolation rule. Changes that
occur entirely between video frames are not represented. The final occupied
frame of a clip holds its last gain until its end, without an implicit fade.
Authored half-open clip intervals and interpolation anchors use the same
floor-rounded sample boundaries, including fractional frame rates.

The mixer retains adjacent frame evaluations across blocks. Per-calculation
input caches reuse results when inputs stay constant; unrelated animation does
not enable sample-by-sample scripting. Playback workers accept new immutable
snapshots at block boundaries and discard previous frame gains when content
changes. Source and frame-rate changes reconcile clips through the same update
API. Unchanged assets retain their decoders across placement and playback edits;
replacement assets open before committing the new configuration. UI and meter
code do not compare source configurations.
Interpolation produces control ramps, which are applied directly to decoded
audio during mixing. The evaluation and interpolation do not depend on render
block size.

The scene settings pane separates arguments from calculations. Each calculation
shows its declared output type, input aliases and source selectors, multiline
code, and a result or diagnostic evaluated from the scene's argument defaults.
The property inspector offers both arguments and calculations as binding sources.
Grouping items extracts the calculations and transitive inputs required by the
selected items. Exclusive calculations move to the new definition; calculations
still used by the parent are copied as independent calculations in the group.
Only real arguments cross the public scene boundary. Failed calculation bindings
continue to use each destination's authored fallback. Clipboard data keeps the
authored items and the resolved values of connected properties separately.
Pasting into the source scene preserves connections and authored fallbacks;
pasting elsewhere uses the values at copy time, while unconnected instance
arguments retain default inheritance. Paste validates the remapped bindings
against the destination contracts before committing. Deletion, persistence, and undo handle
both kinds of variable.

Calculation input names retain drafts until committed. Model synchronization
preserves unfinished or invalid input text, and controls use input names rather
than row positions so deleting a different input does not move a draft.

Compiled `Script` values are shared across snapshots and history; project files
store only their source. The shared Rhai runtime also evaluates plugin bounds.
It limits execution, recursion, container sizes, and expression nesting, and
exposes no filesystem imports, dynamic evaluation, time, or random sources.

### Inspector controls

`ControlTree` builds descriptions directly from domain items and borrowed scene
variables. `PropertyInspector` retains UI state and handles input. Leaf variants
own typed values; `ControlMeta` holds shared presentation and editing metadata.
Tuples, arrays, and effects have separate variants. Array rows keep their stable
IDs with their controls; effect controls own their identity, label, and children.

Numeric widgets use `NumericInput::{F32, I32, U32}`. Schema conversion admits only
these types. `NumericInputSpec` adds display settings and UI constraints to the
numeric type; parsing, formatting, and step rules operate on that validated type.

## Selection and editor targets

Selection stores the user's explicit item IDs; seeking, stepping, and playback
never change that set. Clipboard, deletion, grouping, visibility, movement,
animation synchronization, and timeline keyframe editing use this selection.

`active_selected_item_ids` derives the subset at the current frame for preview and
inspector display. The curve pane requires its focused item to be active. Inspector
targets and animation focus persist outside the selected items' time ranges; the
inspector's editing target is independent of timeline selection.

## Animation and Undo

Authored key times, repeat periods, and phase offsets use integer frames.
Keyframes lie within a track's span; insertion, movement, exact-key lookup, and
synchronization operate on `Frame` values. Ping-pong periods are even.
`AnimationClock` maps authored frames to timeline time and continuous sample frames,
including repeats. Normalized positions are derived for UI layout.

Editing exactly on a keyframe changes only that key, regardless of its spacing.
Between keyframes, editing an interval endpoint also updates equal neighboring
values outward from that endpoint; the opposite endpoint remains independent.
Inspector and preview value gestures freeze their target keys when an operation
begins, so crossing another key's value never expands the target set. Coincident
preview handles choose the key nearest the playhead for each property axis.

`AnimationValuePlan` describes those fixed key targets. `ValueEdit` owns the
plans, revision validation, and Undo policy. Inspector value edits coalesce by
property and authored key using the history timeout. Switching between exact-key
and interval editing resolves new targets without forcing an Undo boundary.
Preview gestures use one explicit Undo group across their value targets. Pauses
within either kind of value operation do not split its Undo entry. A paste is
also one undoable edit.

### Curve interactions

The curve pane draws one `GraphSegment`: two fixed endpoints and one interpolation.
The authored track retains its full key list for overview navigation and editing.
`BezierHandle` identifies the segment's two controls directly.

`GraphInteraction` owns each drag's `AnimationEdit`; a handle drag also owns the
frozen interval and value range. Ending or replacing the gesture releases this
state together.

### Resizing and scene expansion

Item resizing follows two rules:

- Trimming preserves keys within the new interval. A shrinking boundary adopts
  each key it crosses without sampling interpolated values. Extending moves
  the endpoint to the new boundary if its value equals the neighboring key.
  Otherwise, it retains the endpoint and adds a new endpoint with the same value.
  Interior key positions remain unchanged.
- Stretching uses integer rational arithmetic. Interior keys win collisions with
  endpoints; collisions between interior keys retain the later key and its
  outgoing interpolation.

Scene instances expand at their definition's stretch point by shifting keys.
Visual and audio evaluation use the same expansion operation.
`SceneExpansionPlan` separates source-interval lookup from the expanded item's
placement, key shifts, and media interval. The inverse lookup is not a playback
clock: expansion preserves media speed and repeat behavior.

## Project files and asynchronous operations

Project files store property overrides relative to plugin defaults and persist
integer animation data directly. During development, the format version stays
fixed without migrating old representations. Core validates loaded values and
clipboard input; pastes assign fresh IDs. Filesystem access and atomic replacement
belong to [`engine::project_io`](../../crates/zerium/src/engine/project_io.rs).

Timeline copies store JSON in the application's `application/x-zerium-timeline-items`
clipboard format (`dev.1step621.zerium.timeline-items` on macOS), alongside a
human-readable item count. Zerium owns the format and decoding; WGPUI transfers
the requested custom formats as bytes.

[`ProjectSession`](../../crates/zerium/src/project_session.rs) identifies project
generations so results from a replaced project can be ignored.
[`ProjectController`](../../crates/zerium/src/app/project_controller.rs) owns file
operations and resets transient state when replacing a project.

File-property and scene-argument pickers keep their operation and task in one
optional `FileSelection`. Completion or a project change releases both.

Application preferences and automatically recorded state are separate from
project files; see [Runtime services](runtime.md#application-state-and-configuration).
