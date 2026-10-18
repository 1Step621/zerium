# Developer documentation

To work on the application, start with [Architecture](architecture/README.md).
To create a plugin, start with [Plugin API v1](plugin/README.md) and its minimal
working example.

## Application internals

| Guide | Contents |
| --- | --- |
| [Architecture](architecture/README.md) | Workspace boundaries, dependencies, source layout, and module conventions |
| [Editing and project state](architecture/editing.md) | Transactions, properties, selection, animation, scenes, and project lifecycle |
| [Rendering](architecture/rendering.md) | Scene evaluation, render plans, bounds, GPU resources, and caching |
| [Media, preview, and export](architecture/media.md) | Readers, decode scheduling, playback, and video output backends |
| [Runtime services](architecture/runtime.md) | Startup, configuration, plugin compilation, localization, logging, and updates |

## Plugin authoring

| Guide | Contents |
| --- | --- |
| [Plugin API v1](plugin/README.md) | Bundle layout, generate/validate workflow, manifest rules, and a complete minimal plugin |
| [Properties and editor features](plugin/properties.md) | Types, defaults, animation settings, and preview/timeline editing contracts |
| [Rendering and shaders](plugin/rendering.md) | Generated WESL modules, bounds, image sampling, and effect passes |
| [Visual and audio inputs](plugin/inputs.md) | Media, playback, text, numbers, render results, and camera views |

The [JSON schema](../plugins/plugin.schema.json) is the manifest field reference.
The [bundled plugin](../plugins/zerium.builtin/) provides working examples.
For the development environment, see [Contributing](../README.md#contributing).
