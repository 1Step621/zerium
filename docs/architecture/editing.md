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
The playhead stores either an exact stopped frame or a continuous playback time;
its displayed frame is derived from that state. Document navigation resets all
scope-local preview state together. Project frame-rate changes are prepared by
the project and each scene, including the scene's expansion anchor.
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
looked up by source address. Reading or selecting items never materializes values
back into authored items. Copying or detaching connected properties explicitly
bakes just their resolved fallbacks into a detached authored copy.
Single-item effect ordering and aspect-ratio mutations belong to `TimelineItem`;
`TimelineDocument` locates items and maintains document structure, while
`TimelineEditor` coordinates project validation, bindings, IDs and Undo.

An item declares playback once, using ordinary property references. All its
visual, audio, and effect media inputs share the resolved clock. Sequence inputs
inherit project FPS only when created, then preserve their authored source rate.
Changing that rate leaves source start and duration in seconds unchanged, so the
selected images can change.

## Property contracts

`PropertyAddress` identifies an item's value, optionally inside an effect, array
element, or tuple. Editing, animation, bindings, and persistence share its
`PropertyPath`. Array elements have stable IDs, so reordering preserves their
animations and bindings. Property schemas define types, defaults, constraints,
and editing metadata; see [Properties](../plugin/properties.md).

All authored item, effect, and scene-instance values use `PropertyOverrides`.
A missing entry inherits the schema default. Setting a value explicitly preserves
that override even when it equals the current default; resetting a whole property
removes it. Evaluation materializes a separate `PropertyValues` read model,
including defaults, bindings, and animation. Playback properties use the same
Inspector and authored override storage as other properties.

User creation defaults are app preferences keyed by plugin and item/effect type.
Inspector section menus capture evaluated editable properties at the playhead;
curves, connections, effect stacks, placement, and IDs are not preferences.
File references in preferences use absolute paths, independent of project location.
New items and effects receive the saved properties as authored overrides before
timing, aspect ratio, and placement are derived. Source FPS inherits project FPS
only when no initial value was supplied. File imports subsequently initialize
source-specific values from the selected asset. Duplication, paste, scene import,
and project loading preserve their authored data.
Property reset uses the saved value for that property when available, otherwise
restores schema inheritance. Resetting a whole aspect-locked tuple adopts the
reset value's ratio. Registering defaults changes only preferences; item creation
and property reset remain ordinary undoable project edits. Invalid or obsolete
saved property values are ignored when they no longer match a plugin schema.

The property inspector folds item properties and each effect independently.
Fold state belongs to the view, survives selection changes within a scene, and
resets when changing scenes or projects. Folded effects retain their header
controls. Folding preserves input drafts and does not change authored properties
or Undo history.

Item and effect access share a resolved property owner. Scene bindings use
`PropertyAddress` internally, with a separate serialized representation preserving
the project and clipboard format. Each immutable variable collection has an
ordered reverse binding index; concrete target schemas are resolved lazily and
reused within an evaluation scope.

### Scene variables and calculations

Main and reusable scenes share `SceneDefinition`, including their document and
variables. Main cannot be deleted or placed as an instance.

A scene owns arguments and calculations in one variable collection:

- Arguments define instance inputs, defaults, and optional evaluation limits.
  Authored defaults, overrides, and animation keys retain their raw values.
  Evaluation samples the curve, then applies the limits before consumers read it.
- Calculations declare an output type, a Rhai script, and named inputs connected
  to other variable IDs. They have no instance input or default value.

Main has no parent instance, so its arguments always use their authored defaults
and act as editable constants. Scene settings present them as values, without
argument-limit controls. Main supports property connections and calculations. Variables remain local to
their scene; values reach child scenes through explicit instance argument bindings.

References use stable IDs, independent of names and display order. Bindings can
consume either kind of variable. A connected property cannot also own an
animation track. Editing and loading reject missing inputs, invalid or duplicate
input names, and dependency cycles. Referenced variables must be disconnected
before deletion.

Evaluation requests only the outputs a consumer needs. Calculation results are
cached by their connected input values within an immutable scene revision;
script execution happens outside cache locks. `SceneRuntime` owns calculation
execution and its caches; `SceneDefinition` owns authored variables and exposes
scene-level evaluation. Each evaluation has a fresh scope. Scene settings can
request all default outputs, cached once per immutable scene revision. Instance
evaluation shares the per-calculation cache without storing another full result
cache for every set of instance arguments.
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

`ControlTree` describes resolved property controls, including aspect-ratio controls
for tuples. The inspector and scene settings use keyed
[`InputStore`](../../crates/zerium/src/ui/components/input/mod.rs) collections to
own input entities, subscriptions, and unfinished drafts. Input identities
survive unrelated row changes; model synchronization preserves invalid or
uncommitted calculation text. Each field keeps its own commit timing.
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
and Undo policy. Animation and timeline gestures use an explicit gesture policy;
timed text edits use a coalescing policy. Inspector text and step edits coalesce
by property and authored key using the history timeout.
Preview gestures and inspector numeric drags keep one operation and one explicit
Undo group until they end. They read back committed values and stop accepting
updates after an intervening edit. Pausing within an operation does not split its
Undo entry. Paste is also one undoable edit.

`ItemMoveEdit` keeps the original items, placements, and source-to-copy ID mapping
for one drag. It constrains frame and layer deltas for both snapping and commits;
the UI supplies pointer movement and axis choice. Holding Ctrl
switches between moving originals and moving copies, including their scene
bindings and effects; releasing it removes the copies and resumes the move.
Mode changes commit atomically and share the gesture's Undo entry. While Shift is
held, movement follows the dominant displacement axis from the drag origin,
recomputed as the pointer moves. Layer-only moves preserve
the original frame exactly, including when snapping is enabled.

The editor's edit revision advances on mutations, Undo/Redo, and document
navigation. Edit contexts use it so returning to an earlier project state cannot
revive an old operation. The project revision separately identifies saved
content and is restored by Undo, preserving dirty-state comparisons.

### Curve interactions

The curve pane displays one segment while retaining the full track for navigation.
`AnimationEdit` resolves eligibility, affected properties and movement limits for
both editing and synchronization feedback. Creating an edit is read-only. Gestures
keep their targets fixed until release and use `EditSession` to reject updates
after unrelated document changes. Their first mutation begins the Undo entry.
Menu commands retain their original property and document context; interpolation
changes and curve paste resolve synchronization targets when they execute.

Curve edits synchronize segments with matching start and end times and
interpolation settings. Keyframe moves synchronize interior keys at the same
timeline time. The source item participates even when unselected; synchronization
extends to other selected items only when the source is selected.
Timeline markers show authored keys, including fixed endpoints; only interior
keys of the inspector's displayed scalar property can be dragged. Repeat
occurrences are display-only.

Steps interpolation divides a segment into a positive number of equal intervals,
holding `floor(t * count) / count` of the value transition within each interval.
The endpoint retains its authored value; a count of one matches Hold. Step counts
are segment settings and use the same synchronized editing and Undo as curves.

### Resizing and scene expansion

Splitting selected items at the playhead duplicates each interval crossing that
frame and trims the two halves using the ordinary trim rules below, including
scene instances. It does not preserve the original curves or scene timing beyond
those rules. Left halves retain their IDs; right halves receive new item/effect
IDs and inherit scene bindings and preview visibility. Right halves replace the
split items in the selection. Other selected items stay selected. The batch is
one atomic, undoable edit; interval boundaries are not split.

Items with a `playback` declaration store source start, source span, speed, and
EOF behavior only as ordinary property overrides. Their frame duration is derived
from source span, speed, and the document FPS. The source interval retains its
authored precision; only the occupied timeline duration rounds up to whole frames.
Items without playback store an explicit fixed frame duration. Each item's start
is always stored in frames. Placement reads receive the document FPS explicitly;
there is no second authored clock or cached duration in source items.

`TimeMapping` is the resolved source interval. It performs source/frame conversion,
trim/stretch calculations, and duration-limit calculations. Property edits update
overrides and adjust animation intervals immediately, then validate placement as
one detached edit. Trimming retains speed; stretching retains the source interval
and changes speed. FPS changes retain playback properties and derive new frame
durations, while fixed clips retime their stored duration. Scene expansion extends
source span while preserving speed. Imports initialize timing only from the named
primary source; later metadata refreshes leave authored ranges unchanged.

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

Project files store explicit property overrides, frame placement, authored animation
data, and scene variable definitions. Metadata observations and their revisions
are runtime data and are excluded from project files, scene files, and clipboard data.
Core validates project and clipboard data; pasted items receive fresh IDs.
Clipboard payloads include their source FPS so placement and animation keys can be
retimed when pasted into a project with another frame rate. Playback properties
retain their source interval and speed.
Loading converts supported file versions to the current representation in memory
before validating the project. Saving writes the current format.
Version-specific transformations live in
[`project/migration`](../../crates/zerium-core/src/persistence/project/migration/).
Unsupported versions are rejected. The current project representation identifies
`main_scene` within one `scenes` collection; main and reusable scenes store items
and variables in the same form. Media observations are rediscovered after loading.
Scene files (`.scene.zero`, format version 1) store a root scene and its reachable
child scenes, source FPS, and the source project ID as their identity namespace.
Scene files carry a `type: "scene"` marker. Normal Open, scene import, and system file opening
use the file contents to distinguish scene imports from project replacement;
existing project formats without a marker remain supported. When opening a file,
only project replacement asks to discard unsaved changes. The compound suffix is a naming
convention, so renaming a scene file to `.zero` does not change its behavior.
They share scene/item serialization and validation with project files, while
excluding resolution, guides, and playhead state. Media paths remain references,
using the same relative-path rules as project files; assets and plugins are not
embedded.
Import adds independent scene definitions in one undoable edit without replacing
the current project or placing an instance. Scene and effect IDs are reassigned,
internal references and effect bindings are remapped, and conflicting scene names
receive numeric suffixes. Item IDs remain local to each scene document. Frame
placement, animation, and stretch points retime to the destination FPS, preserving
seconds to the nearest available frame. Imported definitions can then be placed
with the existing scene item picker.

Project and scene file loading and saving belong to
[`app::project::file`](../../crates/zerium/src/app/project/file.rs), using atomic
transactions from [`app::storage`](../../crates/zerium/src/app/storage/mod.rs).

Asynchronous file operations reject results from replaced projects or obsolete
edit contexts; see [Project and UI lifetimes](runtime.md#project-and-ui-lifetimes).

Application preferences are separate from project files; see
[Runtime services](runtime.md#application-state-and-configuration).
