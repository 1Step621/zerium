# Editing and project state

[Architecture](README.md) · **Editing** · [Rendering](rendering.md) · [Media and export](media.md) · [Runtime](runtime.md)

## Transactions and read models

Persistent changes go through `TimelineEditor`, which validates edits, records
Undo history, and advances the project revision. Property and resize commands
prepare detached candidates and publish the batch only after validation; a failed
batch leaves the document and history unchanged.

Selection, preview visibility, playhead, and history are session state.
Rendering reads a `TimelineView`; background work receives immutable
`TimelineSnapshot`s.

## Property contracts

`PropertyAddress` identifies an item's value, optionally inside an effect, array
element, or tuple. Editing, animation, bindings, and persistence share its
`PropertyPath`. Array elements have stable IDs, so reordering preserves their
animations and bindings. Property schemas define types, defaults, constraints,
and editing metadata; see [Properties](../plugin/properties.md).

### Scene variables and calculations

A scene owns arguments and calculations in one variable collection:

- Arguments define instance inputs, defaults, and optional evaluation limits.
  Authored defaults, overrides, and animation keys retain their raw values.
  Evaluation samples the curve, then applies the limits before consumers read it.
- Calculations declare an output type, a Rhai script, and named inputs connected
  to other variable IDs. They have no instance input or default value.

References use stable IDs, independent of names and display order. Bindings can
consume either kind of variable. A connected property cannot also own an
animation track. Editing and loading reject missing inputs, invalid or duplicate
input names, and dependency cycles. Referenced variables must be disconnected
before deletion.

Evaluation requests only the outputs a consumer needs. Calculation results are
cached by their connected input values within an immutable scene revision;
script execution happens outside cache locks. Each evaluation has a fresh scope.
The shared [script runtime](../../crates/zerium-core/src/script.rs) limits execution
and exposes no imports, dynamic evaluation, time, or randomness. Projects store
script source. Supported outputs are numbers, integers, booleans, text, and
colors; integer results require explicit conversion from floats, and colors use
four-component arrays or `rgba(r, g, b, a)`.

Invalid scripts remain editable and saveable. Property resolution returns both
values and diagnostics, preserving authored values for failed connections and
applying successful connections independently. Failures propagate through
calculation dependencies and nested scene arguments. Preview uses the fallback;
export stops only when a consumed output depends on a failure. Unused outputs do
not fail export. Visual and audio consumers choose their own properties and sample
times; see [Rendering](rendering.md#scene-evaluation-and-render-planning) and
[Audio](media.md#audio).

Grouping moves calculations used exclusively by the selected items and copies
shared dependencies. Only arguments become public inputs of the new scene.
Clipboard data retains authored fallbacks and resolved connection values:
pasting within the source scene preserves connections; pasting elsewhere uses
the values at copy time. Unconnected instance arguments keep inheriting defaults.
Paste validates remapped connections before committing.

### Inspector controls

`ControlTree` describes controls from domain values. `PropertyInspector` owns
input entities and unfinished drafts. Model synchronization preserves invalid or
uncommitted calculation input text; input identities survive changes to other
rows. Numeric controls display authored values and show the applied value when
argument limits change the result.

## Selection and editor targets

Selection stores explicit item IDs. Seeking and playback do not change it.
Clipboard, deletion, grouping, movement, visibility, and synchronization use this
selection. Preview and the inspector derive its active subset at the current
frame. Inspector target and animation focus are separate from timeline selection
and survive seeking outside the item's interval.

## Animation and Undo

Authored key times, repeat periods, and phase offsets use integer frames.
`AnimationClock` maps these into timeline time and continuous sample positions.
Ping-pong periods are even; normalized positions are derived for display.

An exact-key edit changes that key only. Between keys, editing an interval
endpoint also changes equal neighboring values outward from it; the opposite
endpoint stays independent. `AnimationValuePlan` fixes those targets when an
operation begins, so crossing another value does not recruit more keys.
Coincident preview handles choose the key nearest the playhead for each axis.

`ValueEdit` owns targets, revision validation, and Undo policy. Inspector text and
step edits coalesce by property and authored key using the history timeout.
Preview gestures and inspector numeric drags keep one operation and one explicit
Undo group until they end. They read back committed values and stop accepting
updates after an intervening edit. Pausing within an operation does not split its
Undo entry. Paste is also one undoable edit.

### Curve interactions

The curve pane displays one segment while retaining the full track for navigation.
`GraphInteraction` owns the active edit and freezes the interval and value range
for a handle drag. Ending the gesture releases that state.

### Resizing and scene expansion

- Trimming keeps interior keys. A shrinking boundary adopts keys it crosses
  without interpolation. Extending moves the endpoint when it equals its
  neighbor; otherwise it adds a new endpoint with the same value.
- Stretching uses integer rational arithmetic. Interior keys win collisions with
  endpoints; the later interior key and its outgoing interpolation win other
  collisions.

Scene expansion inserts time at the definition's stretch point by shifting keys.
Visual and audio evaluation share this operation. Source-interval lookup is
separate from playback clocks: expansion preserves media speed and repeat
behavior.

## Project files and asynchronous operations

Project files store overrides relative to plugin defaults, authored animation
data, and scene variable definitions. Core validates project and clipboard data;
pasted items receive fresh IDs. The format can change without migration during
development. Filesystem access and atomic replacement belong to
[`engine::project_io`](../../crates/zerium/src/engine/project_io.rs).

[`ProjectSession`](../../crates/zerium/src/project_session.rs) identifies project
generations so asynchronous results from a replaced project can be ignored.
[`ProjectController`](../../crates/zerium/src/app/project_controller.rs) owns file
operations and resets transient state on replacement. Active file pickers and
exports keep their operation and task together until completion or cancellation.

Application preferences are separate from project files; see
[Runtime services](runtime.md#application-state-and-configuration).
