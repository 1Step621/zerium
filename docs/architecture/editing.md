# Editing and project state

[Architecture](README.md) · **Editing** · [Rendering](rendering.md) · [Media and export](media.md) · [Runtime](runtime.md)

## Transactions and read models

Persistent changes go through `TimelineEditor`, which validates edits, records
Undo history, and advances the project revision. Property, animation, move and
resize commands prepare detached item candidates and publish the batch only after
validation. They capture a history snapshot only when beginning an Undo group;
continuous updates do not retain a rollback snapshot that would force project
maps to be cloned. Structural commands that update several parts of a project
use a rollback boundary restoring documents, ID allocators, selection, visibility
and media observations on failure or no change. History and revisions advance
only for committed changes.

Media discovery has a separate revision in the immutable project. Validated item
edits infer their impact from file reference changes; generic commands explicitly
declare whether they affect media dependencies. Naming, curve, numeric and project
display changes preserve the media revision. Undo restores the revision with its
project, while fresh edits allocate new revisions across history branches.

Selection, preview visibility, playhead, and history are session state.
Rendering reads a `TimelineView`; background work receives immutable
`TimelineSnapshot`s. Project roots, scene collections and individual scene
documents share storage until mutation. Editing a scene or its metadata does not
clone unrelated document indexes. Instance-wide argument updates locate matching
items before acquiring mutable access, preserving storage for unrelated items.

`TimelineItem` is authored data. `EvaluatedItem` and `EvaluatedEffect` contain
sampled values and cannot be passed to editing or persistence APIs. Evaluated
items retain their source identity separately from their placement-specific ID;
their interval describes the visible placement. They retain only sampled values
and display/render metadata; authored curves remain in the document and are
looked up by source address. Clipboard materialization explicitly applies resolved
values to a detached authored copy at the property-resolution boundary.
Single-item effect ordering and aspect-ratio mutations belong to `TimelineItem`;
`TimelineDocument` locates items and maintains document structure, while
`TimelineEditor` coordinates project validation, bindings, IDs and Undo.

## Property contracts

`PropertyAddress` identifies an item's value, optionally inside an effect, array
element, or tuple. Editing, animation, bindings, and persistence share its
`PropertyPath`. Array elements have stable IDs, so reordering preserves their
animations and bindings. Property schemas define types, defaults, constraints,
and editing metadata; see [Properties](../plugin/properties.md).

Item and effect access share a resolved property owner. Scene bindings use
`PropertyAddress` internally, with a separate serialized representation preserving
the project and clipboard format. Each immutable variable collection has an
ordered reverse binding index; concrete target schemas are resolved lazily and
reused within an evaluation scope.

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
script execution happens outside cache locks. `SceneRuntime` owns calculation
execution and its caches; `SceneDefinition` owns authored variables and exposes
scene-level evaluation. Each evaluation has a fresh scope.
The shared [script runtime](../../crates/zerium-core/src/script.rs) limits execution
and exposes no imports, dynamic evaluation, time, or randomness. Projects store
script source. Supported outputs are numbers, integers, booleans, text, and
colors; integer results require explicit conversion from floats, and colors use
four-component arrays or `rgba(r, g, b, a)`.

Invalid scripts remain editable and saveable. Property resolution returns both
values and typed diagnostics, preserving authored values for failed connections and
applying successful connections independently. Failures propagate through
calculation dependencies and nested scene arguments. Preview uses the fallback;
export stops only when a consumed output depends on a failure. Unused outputs do
not fail export. Diagnostics retain scene and variable IDs, the exact property
address, and dependency/instance-argument causes. Audio adds the contributing
frame; visual export adds sample time and instance path. Formatting happens at
the UI boundary. Visual and audio consumers choose their own properties and sample
times; see [Rendering](rendering.md#scene-evaluation-and-render-planning) and
[Audio](media.md#audio).

Grouping copies the variables and calculation dependencies required by the selected
items, retaining the original definitions in the parent scene. Only arguments
become public inputs of the new scene.
Clipboard data retains authored fallbacks and resolved connection values:
pasting within the source scene preserves connections; pasting elsewhere uses
the values at copy time. Unconnected instance arguments keep inheriting defaults.
Paste validates remapped connections before committing.

### Inspector controls

`ControlResolution` resolves domain values and editing context into a
`ControlTree`. Tuple controls carry an optional aspect-ratio control. Row
composition lives in module functions; `PropertyInspector` owns input entities,
unfinished drafts and callbacks that update inspector state. Model synchronization preserves invalid or
uncommitted calculation input text; input identities survive changes to other
rows. The inspector and scene settings share a keyed `InputStore`, which owns
input entities, subscriptions, draft preservation, and obsolete-row cleanup.
Each field retains its own commit timing and model synchronization policy.
Numeric controls display authored values and show the applied value when
argument limits change the result.

Color inputs can sample the composited preview image, including alpha, through
the eyedropper button. Sampling pauses playback and excludes editor overlays and
UI backgrounds. The GPU pixel is converted from premultiplied scene-linear color
to straight sRGB and applied through the input's existing edit and Undo path.
Escape, right-click, clicking outside the preview, or changing the target or
editor state cancels sampling.

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

`ValueEdit` owns fixed targets; its shared `EditSession` owns revision validation
and Undo policy. Animation and timeline gestures use the same session type directly, with an
explicit gesture policy; timed text edits use a coalescing policy. Inspector text and
step edits coalesce by property and authored key using the history timeout.
Preview gestures and inspector numeric drags keep one operation and one explicit
Undo group until they end. They read back committed values and stop accepting
updates after an intervening edit. Pausing within an operation does not split its
Undo entry. Paste is also one undoable edit.

### Curve interactions

The curve pane displays one segment while retaining the full track for navigation.
`AnimationEdit` resolves the affected properties for both the synchronization
indicator and editing. The indicator describes curve edits; each key's tooltip
describes its movement. Creating an edit is read-only: it fixes the targets and
captures an `EditSession`, which rejects mutations after unrelated document changes.
Timeline and curve drags hold this edit from press through release; only drag
movement mutates the document, and its first change begins the Undo entry.
Menu commands also capture their original property and document context rather
than querying the inspector's selection when they execute. Interpolation changes
and curve paste resolve their synchronization targets when the command executes.
The core resolves source eligibility and key movement limits;
timeline markers use these same limits. Matching looks up a key by its canonical
time in each track, and an edit contains each affected property once.
Curve edits synchronize segments with matching start and end times and interpolation
settings. Keyframe moves synchronize interior keys at the same timeline time.
Direct manipulation includes the source item even when it is not selected;
synchronization extends to other selected items only when the source is selected.
Linked curves and movable keys retain the accent hue with stronger contrast,
thicker curves, and filled key markers. Independent keys use hollow markers in the
normal accent color.
Tooltips list all affected properties in label order, with item labels on every row.
The list and synchronization count both include the source property.
The timeline shows all keys, including the endpoints, in a single row aligned to
the item top. Dots at the same screen position are grouped, prioritizing the
displayed property's keys.
Repeat occurrences use the same appearance as ordinary dots and are display-only;
authored dots support seeking. The item label stays vertically centered.
Only keys of the inspector's displayed scalar property can be dragged, with the
core's interior-key eligibility and movement limits. Other items and properties
remain ordinary dots. The displayed property's timeline keys show their edit
scope in a tooltip and use filled markers when linked
and hollow markers when independent; the active key reflects its fixed edit scope.
Moving a key leaves the playhead in place in both the timeline and curve pane.
`GraphInteraction` owns the active edit and freezes the interval and value range
for a handle drag. Ending the gesture releases that state.
Curve gestures and step-count input discard their edit state when an unrelated
document change makes it stale. Step-count text and drag edits share one edit;
numeric pointer motion is owned by the curve pane for both step and repeat inputs.
Interval controls use the same authored interval identity as view fitting.

Steps interpolation divides a segment into a positive number of equal intervals,
holding `floor(t * count) / count` of the value transition within each interval.
The endpoint retains its authored value; a count of one matches Hold. Step counts
are segment settings and use the same synchronized editing and Undo as curves.

### Resizing and scene expansion

- Trimming keeps interior keys. A shrinking boundary adopts keys it crosses
  without interpolation. Extending moves the endpoint when it equals its
  neighbor; otherwise it adds a new endpoint with the same value.
- Stretching uses integer rational arithmetic. Interior keys win collisions with
  endpoints; the later interior key and its outgoing interpolation win other
  collisions.

Scene expansion inserts time at the definition's stretch point by shifting keys.
Expansion preserves media speed and repeat behavior. `RenderEvaluation` resolves
visual placements on demand using the same scene expansion and binding rules as
the `SceneWalker` used for editor read models, audio, and metadata discovery.
See [Rendering](rendering.md#scene-evaluation-and-render-planning) for visual
source intervals and [Media and export](media.md) for audio and discovery scopes.

## Project files and asynchronous operations

Project files store overrides relative to plugin defaults, authored animation
data, and scene variable definitions. Core validates project and clipboard data;
pasted items receive fresh IDs. Projects are saved with `format_version: 2`.
Loading converts supported file versions to the current representation in memory
before validating the project. Saving writes the current format.
Unsupported versions are rejected before decoding their contents.
Filesystem access and atomic replacement belong to
[`engine::project_io`](../../crates/zerium/src/engine/project_io.rs).

[`ProjectSession`](../../crates/zerium/src/project_session.rs) identifies project
generations so asynchronous results from a replaced project can be ignored.
[`ProjectController`](../../crates/zerium/src/app/project_controller.rs) owns file
operations and resets transient state on replacement. Active file pickers and
exports keep their operation and task together until completion or cancellation.

Application preferences are separate from project files; see
[Runtime services](runtime.md#application-state-and-configuration).
