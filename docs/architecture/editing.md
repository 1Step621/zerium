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

Property evaluation applies plugin defaults, scene arguments, animation, and
constraints through a pipeline shared by the inspector, rendering, and audio.
`SceneArgument` owns normalization of its scalar contract and updates to its
defaults and numeric settings.

### Inspector controls

`ControlTree` builds descriptions directly from domain items and borrowed scene
arguments. `PropertyInspector` retains UI state and handles input. Leaf variants
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
  each key it crosses without sampling interpolated values. Extending retains
  the original endpoint and adds a new endpoint with the same value.
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

[`ProjectSession`](../../crates/zerium/src/project_session.rs) identifies project
generations so results from a replaced project can be ignored.
[`ProjectController`](../../crates/zerium/src/app/project_controller.rs) owns file
operations and resets transient state when replacing a project.

File-property and scene-argument pickers keep their operation and task in one
optional `FileSelection`. Completion or a project change releases both.

Application preferences and automatically recorded state are separate from
project files; see [Runtime services](runtime.md#application-state-and-configuration).
