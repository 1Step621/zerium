# Runtime services

[Architecture](README.md) · [Editing](editing.md) · [Rendering](rendering.md) · [Media and export](media.md) · **Runtime**

## Startup and composition

[`app/mod.rs`](../../crates/zerium/src/app/mod.rs) loads configuration and state,
selects the locale, translates and validates bundled plugin shaders, and starts
GPUI. It wires the editor, project session, transport, engine, and UI.

## Project and UI lifetimes

[`ProjectSession`](../../crates/zerium/src/app/project/session.rs) owns the current
project session and the registry of pending imports, file selections, saves,
loads, and exports. Operation tokens contain session and operation identities;
their activity is stored in the registry. The project controller consults that
registry when deciding whether the window can close, rather than collecting
separate busy flags from each controller.

[`app/transport.rs`](../../crates/zerium/src/app/transport.rs) owns playback and
scrubbing state, including which pane started a scrub. Timeline and curve views
query it instead of maintaining parallel scrub flags. A change of project or active scene resets the timeline's viewport and
transient interaction state through its editor subscription.

File selections retain their property target and editor edit context together
with the pending task. Editing, undo, scene navigation, or deselecting that target
cancels the selection, so a delayed probe cannot overwrite a newer edit. File
drops also retain their destination scene and discard results after navigating
to another scene.

## Application state and configuration

The user config directory contains two files:

| File | Contents | Owner |
| --- | --- | --- |
| `config.json` | User preferences, including language | [`app::storage::config`](../../crates/zerium/src/app/storage/config.rs) |
| `state.json` | Automatically recorded application state | [`app::storage::state`](../../crates/zerium/src/app/storage/state.rs) |

Missing files are created with defaults. The shared
[`JsonStore`](../../crates/zerium/src/app/storage/mod.rs) saves updates atomically
before replacing the in-memory value. Invalid files remain intact; load errors
are logged and leave the affected store unavailable.

The settings dialog edits a draft and saves through the configuration store.
Locale selection uses the configured override, system locale, then `en-US`, before
UI initialization. Language changes take effect on the next launch.

## Plugin loading and validation

Core validates manifests and owns the editing registry.
[`zerium-shader`](../../crates/zerium-shader/src/) generates interfaces, translates
WESL to WGSL, and validates modules, entry points, and pipeline constants before
GPU pipeline creation. Application startup and CLI validation share
`validate_plugins`; parsed modules are reused across the plugin's pipelines.

The application embeds bundled sources; the CLI loads filesystem plugins through
the same tooling. Loading checks that generated interfaces match their source
contract and rejects stale bundles.

See [Plugin API v1](../plugin/README.md) for the generate/validate workflow and
[Rendering and shaders](../plugin/rendering.md) for the shader interface.

## Localization, logging, and updates

- [Localization](../../crates/zerium/src/i18n.rs) shares one locale across core and
  UI labels. User-authored names remain plain text.
- [Logging](../../crates/zerium/src/logging.rs) writes to the state directory and
  stderr. `RUST_LOG` controls verbosity; the Help menu reveals the log.
- The [update service](../../crates/zerium/src/app/update/) checks GitHub Releases
  and uses Velopack to apply updates on relaunch. Nix builds disable `self-update`.
