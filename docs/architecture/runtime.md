# Runtime services

[Architecture](README.md) · [Editing](editing.md) · [Rendering](rendering.md) · [Media and export](media.md) · **Runtime**

## Startup and composition

[`app/bootstrap.rs`](../../crates/zerium/src/app/bootstrap.rs) loads configuration
and state, selects the locale, compiles bundled plugin shaders, and starts GPUI.
It wires the editor, project session, media, rendering, transport, and UI.

## Application state and configuration

The user config directory contains two files:

| File | Contents | Owner |
| --- | --- | --- |
| `config.json` | User preferences, including language | [`app::config`](../../crates/zerium/src/app/config.rs) |
| `state.json` | Automatically recorded application state | [`app::state`](../../crates/zerium/src/app/state.rs) |

Missing files are created with defaults. The shared
[`JsonStore`](../../crates/zerium/src/app/persistence.rs) saves updates atomically
before replacing the in-memory value. Invalid files remain intact; load errors
are logged and leave the affected store unavailable.

The settings dialog edits a draft and saves through the configuration store.
Locale selection uses the configured override, system locale, then `en-US`, before
UI initialization. Language changes take effect on the next launch.

## Plugin loading and compilation

Core validates manifests and owns the editing registry. Shader tooling resolves
WESL, generates interfaces, and compiles and validates entry points and overrides.
The application embeds bundled sources; the CLI loads filesystem plugins through
the same tooling. Generated interfaces are checked against their source contract.

Validated schemas compile typed playback bindings shared by media, editing, and
value validation. Sharing property references shares a source mapping; independent
references preserve separate playback settings.

See [Plugin API v1](../plugin/README.md) for the generate/validate workflow and
[Rendering and shaders](../plugin/rendering.md) for the shader interface.

## Localization, logging, and updates

- [Localization](../../crates/zerium/src/i18n.rs) shares one locale across core and
  UI labels. User-authored names remain plain text.
- [Logging](../../crates/zerium/src/logging.rs) writes to the state directory and
  stderr. `RUST_LOG` controls verbosity; the Help menu reveals the log.
- The [update service](../../crates/zerium/src/app/update/) checks GitHub Releases
  and uses Velopack to apply updates on relaunch. Nix builds disable `self-update`.
