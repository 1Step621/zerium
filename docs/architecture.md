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

A scene argument owns a scalar-only declared contract. Its effective contract is
recomputed from that declaration and the complete current binding set, so two
identical binding sets always produce the same contract regardless of editing
history. Scene instances store only explicit non-default overrides; inherited
defaults are never inferred by comparing values. Bindings address an item or
effect parameter through a typed value path
(`Whole`, array element, tuple element, or an element of an array of tuples).
Commands resolve an address, validate the negotiated contract against every
target, and only then apply it. Persistence validation, editing, and runtime
evaluation use the same resolver. Removing an item, effect, or nested argument
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
Scene settings edit the declared contract; binding intersections determine the
effective contract without replacing the declared bounds.


Derived expressions store argument IDs as their symbols. The inspector renders
those symbols as current labels and converts edited text back to IDs. Renaming a
label therefore cannot change expression meaning. Project and timeline
clipboard format version 2 include a project identity in scene references;
development-era version 1 files are intentionally not migrated.

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
  animation/    curves, easing, typed scalar tracks, interpolation, evaluation
  plugin/       manifests, capabilities, resolved bundles, registry, shader ABI
  timeline/     placement, editing commands, scenes, evaluation, and history
  persistence/  project and clipboard representations and checked reconstruction
  media.rs      media asset metadata
```

`animation/curve` owns editable curve invariants, `easing` owns mathematical
interpolation modes, `track` owns scalar addresses and typed endpoints, and
`evaluation` applies enabled tracks to parameter collections. `interpolation`
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
color pickers do not construct numeric fields for animation operations. Number
animation displays use numeric endpoint ranges, while color curve presentations
use interpolation progress. Array controls read contracts and presentation metadata
from their parameter schema, with a separate UI animation override.
Animation interpolation accepts individual numeric scalars and colors only.
The animation module owns eligibility checks; parameter validation delegates to
that policy. Numeric endpoint commands use the same scalar channel and address
as typed endpoint commands. Animation selection, numeric displays, and animation
history keys retain a `ParameterAnimationAddress` instead of separate array and
channel fields. Each scalar track owns its endpoints and curve directly. Curve
enumeration returns an iterator; mutable track and curve access stays in the
domain layer. Property drags retain their `PropertyTarget`.
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
