# Runtime services

[Architecture](README.md) · [Editing](editing.md) · [Rendering](rendering.md) · [Media and export](media.md) · **Runtime**

## Startup and composition

[`app/bootstrap.rs`](../../crates/zerium/src/app/bootstrap.rs) loads application
configuration and state, selects the locale, and starts GPUI. It compiles bundled
plugin shaders and creates shared editor, project session, media, rendering, and
transport entities before wiring the workspace panes and controllers.

## Application state and configuration

[`app::state`](../../crates/zerium/src/app/state.rs) defines automatically recorded
`AppState` in `state.json`, such as support reminder history.
[`app::config`](../../crates/zerium/src/app/config.rs) defines user-selected settings
as `AppConfig` in `config.json`, including language and support reminder preferences.
Both files live in the user config directory and are created with defaults when
missing. Unspecified configuration options follow application defaults.

[`app::persistence`](../../crates/zerium/src/app/persistence.rs) provides the shared
`JsonStore<T>` implementation. `StateStore` and `ConfigStore` are GPUI globals
accessed through `read(cx)` and `update(cx, |value| { ... })`. Updates atomically
save the file before replacing the in-memory value. Load errors are logged;
invalid files are left intact and the corresponding store remains unavailable.

Both stores load together before GPUI startup and are registered when the
application context becomes available. Language selection happens before UI and
font initialization, using the configured override, system locale, or `en-US`.
Language changes take effect on the next launch.

The [settings dialog](../../crates/zerium/src/ui/config.rs) edits a draft of
`AppConfig`. Save commits it through `ConfigStore`; Cancel discards the draft.

## Plugin loading and compilation

Core owns manifest validation and the editing registry. Shader owns WESL source
resolution, interface generation, and compilation. Each declared module is linked
and structurally validated once, then checked against its pass entry points and
overrides. The application embeds bundled plugins; the plugin CLI uses the same
shader tooling.

After validation, item and effect schemas compile `PlaybackBindings`. Shared
property references identify one source mapping; independent references retain
separate mappings. Media, audio, timeline editing, and value validation resolve
typed playback settings through these bindings rather than interpreting the
capability declarations independently.
`TimeMappingProperties` reads and writes the declared property references;
`TimeMapping` handles numeric time conversion without knowing plugin declarations.

`ValidatedShader` retains its WGSL source alongside the parsed module and
validation information. Compiled pipeline descriptors share that same source.

Generated sources retain WESL module paths through fingerprinting and loading.
The writer converts those paths to filesystem paths when publishing `generated/`.
The application registers embedded WESL assets in a `VirtualResolver`; filesystem
plugins use WESL's `FileResolver`.

See [Plugin API v1](../plugin/README.md) for bundle layout and the generate/validate
workflow, and [Rendering and shaders](../plugin/rendering.md) for the WESL contract.
The [JSON schema](../../plugins/plugin.schema.json) is the field reference.

## Localization, logging, and updates

- [Localization](../../crates/zerium/src/i18n.rs) selects a shared `rust-i18n` locale
  for core and UI labels. User-authored names remain plain text.
- [Logging](../../crates/zerium/src/logging.rs) writes process logs to the application's
  state directory and stderr. `RUST_LOG` controls verbosity; the Help menu reveals
  the current log. Panic reports include backtraces.
- The [update service](../../crates/zerium/src/app/update/) checks GitHub Releases in the
  background and uses Velopack to apply updates on relaunch. Nix builds disable
  the `self-update` feature.
