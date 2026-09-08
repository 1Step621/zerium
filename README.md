# Zerium

A GPU-accelerated, non-linear video editor written in Rust.

## Architecture

- `domain`: validated project, timeline, animation, media, and plugin models
- `engine`: rendering, decoding, playback, and export infrastructure
- `ui`: GPUI views and interaction controllers
- `app`: the composition root that wires domain, engine, and UI services

The live timeline is a command-oriented `TimelineEditor`. Background work uses
an immutable `TimelineSnapshot`, so save and export jobs cannot accidentally
carry editor history or mutate UI session state.

See [docs/architecture.md](docs/architecture.md) for dependency rules and state
ownership details.

## Development

Use `direnv` to enter the shared Rust environment:

```sh
direnv allow
```
