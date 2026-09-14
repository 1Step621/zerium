# Architecture

Zerium is a layered application with a functional core and stateful edges.
Dependencies point inward:

```text
app ──────> ui ──────> engine ──────> domain
 │          │                         ^
 └──────────┴─────────────────────────┘
```

`domain` must not depend on GPUI, WGPU, FFmpeg processes, or audio devices.
`engine` may depend on domain models but not on UI entities. `ui` coordinates
entities and translates gestures into domain commands. `app` is the sole
composition root.

Project serialization and validation stay in `domain`; filesystem access is an
`engine::project_io` adapter at the stateful edge.

## Timeline boundaries

The timeline has four distinct kinds of state:

- `TimelineDocument`: persistent items, layers, parameters, effects, and IDs.
- `SelectionState`: current and remembered editor selection.
- `PreviewVisibility`: non-persistent layer, item, and effect overrides.
- `EditHistory`: bounded undo/redo policy and interaction coalescing.

`TimelineEditor` owns these components and exposes crate-private commands. It is
intentionally not cloneable or dereferenceable as raw project state. Background
consumers obtain a detached `TimelineSnapshot`, which
implements the read-only `TimelineView` contract. This prevents an accidental
copy from silently changing history semantics and prevents worker jobs from
mutating live editor state.

The implementation follows those boundaries: `item` owns item/effect data and
geometry rules, `document` owns persistent indexes and invariants, `scene` owns
scene definitions and argument validation, and `evaluation` produces a
hierarchical runtime scene graph. `editor` keeps session/history coordination;
its child command module in `commands.rs` contains scene, item, effect, and
animation commands.

History snapshots include the active scene path, and coalescing keys are scoped
to the active document. Persistent project state is shared with history through
copy-on-write. Documents store item payloads behind `Arc`, so the first mutation
after a snapshot clones the indexes and only the item payloads on the changed
path rather than every asset, parameter, and animation in the project. Item IDs
are local to a document, so restoring project state without restoring its
document location would make selections and edit keys ambiguous. Scene-duration clamping is only
run when the active scene's cached duration changes (or a cross-scene deletion
explicitly invalidates it), rather than after every parameter edit.

Project loading validates serialized data before constructing a
`TimelineDocument`. Replacing a project swaps that complete document and resets
all session-only state in one operation. Project, plugin, media, rendering,
export, and audio-playback failures use typed categories; adapter errors retain
their underlying source where the caller can usefully inspect an error chain.

A `TimelineItem` has an explicit `Plugin` or `Scene` kind. A plugin item always
owns its plugin/item identity and validated schema snapshot; a scene item always
owns a scene ID. Empty plugin IDs and mismatched `schema`/`scene_id` option pairs
are therefore not representable. Selection APIs return ordinary detached
`TimelineItem` values instead of a transparent dereference wrapper.

A scene argument owns one scalar-only contract. Scene instances store only
explicit non-default overrides; inherited
defaults are never inferred by comparing values. Bindings address an item or
effect parameter through a typed value path
(`Whole`, array element, tuple element, or an element of an array of tuples).
Commands resolve an address and require its scalar type to match the argument.
Each binding clamps the argument value to its target constraints before writing
the target parameter, so one argument can drive targets with different ranges.
Persistence validation, editing, and runtime evaluation use the same resolver.
Removing an item, effect, or nested argument
also removes dependent bindings; animation enablement and array shrinking are
rejected when they would invalidate a binding. Expected connection failures are
returned as typed command errors instead of an undifferentiated boolean.
Item/scene/effect creation and media attachment follow the same rule; `bool` is
reserved for commands whose only outcome is changed/unchanged.
Scene commands accept validated `NumericSettings`, whose defaults and optional
bounds retain their `f32`, `i32`, or `u32` types. Missing bounds remain absent.
Serialized constraints have a single `f64` representation, preserving every
32-bit integer endpoint. Scene settings and ordinary number inputs share
`NumericInput`: text parsing rejects fractional integers, while drag and step
operations perform rounding. Display scaling and numeric animation axes use
`f64`.
Scene settings edit the argument contract directly. Target constraints do not
change that contract.


Derived expressions store argument IDs as their symbols. The inspector renders
those symbols as current labels and converts edited text back to IDs. Renaming a
label therefore cannot change expression meaning. Project and timeline
clipboard format version 9 use value stops, two-endpoint interval curves, and
stable array-element identities.
Earlier project and clipboard formats are intentionally not migrated.

Property-inspector controls and their subscriptions are owned by one ephemeral
control-state object keyed by typed `PropertyPath` values. A structural
selection change replaces that object as a unit, preventing stale inputs,
pickers, and subscriptions from drifting out of sync across independent reset
paths. Item and effect parameters share one schema-to-control dispatch and one
renderer; ownership is data on the control, not a parallel set of type switches.
A dedicated transport state machine owns playback, audio, and timeline/curve
scrubbing. Project-scoped asynchronous work carries a session generation and is
cancelled or ignored after a project switch.

Media probing, timestamp-based decoding, resampling, proxy generation,
encoding, and muxing use the FFmpeg libraries in process. The application does
not depend on the `ffmpeg` or `ffprobe` command-line programs, pipe raw frames
through child processes, or supervise encoder processes. `MediaReaderRegistry`
remains the boundary between timeline/rendering code and the concrete FFmpeg
adapter. Video caches use presentation timestamps and frame intervals rather
than synthetic average-FPS indexes. Export keeps a bounded pipeline: reusable
GPU readback slots overlap successive frames, while a dedicated worker owns
FFmpeg conversion and encoding. Project saves, proxy publication, and export
output use same-directory file transactions so failures do not replace a valid
existing file.

## Time and identity

Timeline positions use `Frame`; positive spans use `FrameDuration`; frame rates
are positive rational `FrameRate` values. Constructors reject zero values rather
than deferring failure to arithmetic. Items, layers, and effect instances use
separate stable ID types so values cannot be mixed accidentally. Effect instance
IDs are allocated once at the editor boundary and remain unique across the root
timeline and every reusable scene, because preview visibility spans expanded
scene content.

## Domain module boundaries

The directory entry points (`mod.rs`) expose the API for each concept; callers
import shared parameter types from `domain::parameter`, not through `plugin`.

```text
domain/
  parameter/    contracts, values, constraints, metadata, and JSON adapters
  animation/    easing, typed scalar tracks, interpolation, evaluation
  plugin/       manifests, capabilities, resolved bundles, registry, shader ABI
  timeline/     placement, editing commands, scenes, evaluation, and history
  persistence/  project and clipboard representations and checked reconstruction
  media.rs      media asset metadata
```

`easing` owns mathematical interpolation modes and their two-endpoint handle
behavior, `track` owns scalar addresses, ordered typed stops, and one
interpolation mode per interval, and `evaluation` applies enabled tracks to
parameter collections. `interpolation`
provides explicit functions accepting parameter types or values; it does not add
inherent methods to types owned by `parameter`. Filling missing overrides from
defaults belongs to `parameter`, because it does not require animation.

`persistence/project` captures and reconstructs project state, while
`persistence/clipboard` uses the same checked item representation for clipboard
payloads. The `persistence` entry point exports encoding/decoding and errors.
Filesystem reads and atomic writes remain in `engine/project_io`; the live project
and editor state remain in `timeline`.

## Plugin model

Plugins enter through three explicit layers: `manifest` parses and validates the
root document, the `item`/`capability`/`effect` modules own semantic contracts,
and `bundle` resolves every referenced shader asset. `registry` only provides
deterministic lookup across completed bundles. Shared parameter concerns live in
`domain/parameter/`, independently of plugin loading. `types` and `value` own the
type algebra and checked runtime values; `schema` owns the read-only model;
`wire/` contains type and schema JSON adapters. Structural types use external
Serde tags; array payloads contain their element `type` and item bounds.
`validation` owns contract rules;
`compatibility` and `constraints` implement value acceptance and range algebra;
`ui` provides presentation metadata; `numeric` converts numeric scalars for number inputs. Input generation walks structural scalars without flattening colors or tuples.
The inspector keeps property identity in `PropertyTarget`, shared by scalar inputs
and their event bindings. `NumberInputSettings` holds numeric input configuration;
color pickers do not construct numeric fields for animation operations. Between
stops, animated inspector rows show the current segment's start and end values;
on a stop they show only that stop's value. Color curve presentations use
interpolation progress. Numeric text changes are accepted only while their input
owns keyboard focus; selecting a curve handle transfers focus to the curve editor
before its drag begins. Curve handles are edited only by dragging them in the
graph and do not open a value editor. The curve editor derives the interval
containing the playhead and projects only that interval to a fixed 0–1 graph. A compact overview
above the graph shows every interval, highlights the projected one, and seeks to
an interval's first frame when clicked. It also shows the playhead. Internal
boundaries in the overview move stop positions with frame snapping, snap to the
playhead within the pointer threshold, and preserve the adjacent curves, stop
value, and focused segment. A playhead already attached to the moved boundary
follows it. The graph has no zoom. The overview context menu inserts a
frame-aligned stop by copying the temporally nearest stop's value (preferring
the preceding stop at an exact midpoint) and deletes internal stops when opened
on a boundary. Context-menu actions derive their target directly from the
opening pointer position. Graph stop markers are fixed,
cannot be selected, and retain context-menu deletion. Array
controls read contracts and presentation metadata from their parameter schema,
with a separate UI animation override.
Animation interpolation accepts individual numeric scalars and colors only.
The animation module owns eligibility checks; parameter validation delegates to
that policy. Numeric stop commands use the same scalar channel and address
as typed stop commands. Animation selection, numeric displays, and animation
history keys retain one `ParameterAnimationAddress` containing the parameter,
stable array-element ID, and scalar channel. `ParameterAnimations` maps that full address
directly to one scalar track; there is no parameter- or tuple-level animation
wrapper. Each scalar track owns an ordered stop list and one
two-endpoint interpolation per adjacent pair. Editing a stop propagates its new value to contiguous
equal-valued stops outside the focused segment. Color equality tolerates the
small component error introduced by picker color-space conversion. The focused
segment's opposite endpoint remains independent, so a flat interval can be
split. Mutable track and interpolation access stays in the domain layer. Property drags retain their
`PropertyTarget`.

Runtime parameter arrays store `ArrayElement` values with IDs that survive
reordering and deletion of neighboring elements. Project and clipboard data
persist these IDs, while plugin-schema JSON and the plugin ABI expose only the
ordered element values. Array animations therefore follow element identity;
UI paths and scene bindings continue to use the element's current index.
The timeline keeps aggregate stop markers visible for every animated property
and overlays the focused property's markers with a stronger style. Only internal
stops in that focused track are draggable there; moves stay frame-aligned,
cannot cross neighboring stops, snap to the playhead, and carry the playhead
when it was attached to the moved stop.
Array insertion owns its tuple midpoint policy in
the inspector and applies scalar interpolation to numeric elements.

`ParameterError` has no dependency on plugin loading. The plugin boundary converts
it to `PluginError` and additionally validates WGSL identifiers and generated names.
`plugin/abi` remains the authority for generated WGSL and CPU-side packing. ABI
layouts, fixed pass data, and worst-case byte budgets are compiled once at plugin
load time. Shared plugin cross-schema rules live in `plugin/validation`.

Manifest storage is not an application-facing interface: item, effect,
capability, shader, and pass-constant fields are private and exposed through
immutable queries. Engine and UI code ask for semantic views such as
`texture_inputs()`, `visual_shader()`, `passes()`, and `audio()` rather than
reinterpreting the JSON object graph. Parameter mutation is restricted to the
`domain` layer because scene-argument projection is a domain operation;
rendering and UI receive read-only accessors. Runtime-only adapters use
crate-scoped visibility and are not part of the external Rust API.
`PluginCatalogEntry` is the crate-private common item/effect metadata contract
used by picker UIs.
Item-generic editor behaviors are explicit capability references (`size`,
`label`) validated against the structural scalar or tuple parameter type.
Presentation hints such as `multiline` remain valid for items and effects. An
array of strings uses the array inspector's text element editor by default; the
font picker is selected explicitly with `ui.editor = "font_family"`. Array rows,
length constraints, structural edits, and scene bindings are shared independently
of the element editor. Capability
wiring instead names parameter IDs inside the
capability itself: a text visual maps each rasterizer input to a parameter,
and an audio capability names its gain parameter, following the temporal pass
`sampling` precedent. Storage types stay structural while contradictory
capability/type pairs are rejected by validation.

`PluginRegistry` indexes bundles by namespaced plugin ID, rejects duplicates,
rejects renderer-level shader ID collisions, and exposes deterministic
iteration. `PluginBundle::from_loader` is a transport-independent construction
boundary, while `plugin_catalog` owns embedded-asset transport for built-ins.
There is no dormant external-directory loader in the application surface.
Timeline data can therefore rely on
schema/type agreement while project and plugin files remain untrusted at their
input boundaries.

Validated timeline items and effects retain shared schema snapshots. Runtime
evaluation therefore does not consult the process-wide plugin catalog and
cannot start panicking because an ID lookup disappears after construction.

## Rendering and export

Rendering depends on `TimelineView`, not editing commands. Timeline evaluation
returns a hierarchical graph, preserving scene composition boundaries so an
effect on a scene instance is applied once to the composed children. Temporal
passes reevaluate the matching node path at each sample time, and media requests
carry that time through to timestamp-based decoding. Preview and export own
independent render sessions while sharing immutable pipelines and the GPU
device. Intermediate rendering and effects use scene-linear floating-point
textures; source transfer decoding and final output encoding occur only at the
pipeline boundaries.

## Change rules

- Persistent state changes must go through `TimelineEditor` so revision and
  history bookkeeping cannot be skipped.
- Preview-only state must not increment the project revision or enter history.
- New background work must accept immutable snapshots, never a live GPUI entity.
- Invalid external data returns an error; public constructors must not panic on
  user-controlled values.
- Until Zerium intentionally becomes a library, its Rust API exposes only
  `zerium::run`; domain, engine, plugin, and UI details remain crate-private.
