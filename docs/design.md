# Design

ReScaleFrame connects game-specific rendering hooks to shared upscaling and frame-generation backends. The current AC7 experiment runs DLSS through a `dinput8` research proxy. The design below describes the runtime that will replace that glue.

## Ownership

| Component | Owns |
| --- | --- |
| Launcher or WSGM | Game selection, profile storage, early loading, session lifetime |
| Bootstrap | Loading the orchestrator at a safe boundary |
| Orchestrator | Plugin lifecycle, vendor SDKs, resources, settings, latency, presentation |
| Game plugin | Build recognition, engine hooks, frame data, SR output reinsertion |
| Game SDK | Versioned C contract between plugin and runtime |
| egui | Settings intents, effective status, inspection, and capture controls |

These components share a repository and release version. C interfaces keep ownership explicit across DLLs. Game plugins do not load their own vendor runtimes.

## Frame flow

1. The plugin identifies the input/frame boundary and carries that identity through the game, render, and RHI threads.
2. At the reconstruction boundary, it supplies colour, depth, motion, jitter, camera/exposure state, and valid rectangles.
3. The runtime evaluates SR synchronously and returns the output to the plugin for downstream post-processing.
4. After scene processing, the plugin supplies output-resolution HUD-less colour and the available UI/final-frame data.
5. The runtime handles FG, UI composition, and presentation with matching resources and frame IDs.

SR scene colour and FG HUD-less colour come from different stages. Retaining a texture keeps its allocation alive; it does not preserve its contents. Generated frames do not advance simulation.

## Backend and frontend choices

The planned Claw path uses native DX11 XeSS-SR and a same-adapter DX12 presentation bridge for XeSS MFG/XeLL. Vulkan/DXVK remains an alternative to measure. The API findings are tied to the SDK revisions in [presentation research](research/presentation-backends.md).

One provider owns FG/presentation at a time. Query capabilities from the actual device, driver, API, and SDK configuration, then report requested and effective settings separately.

Standalone and WSGM use the same runtime. Each session has one profile persistence authority; IPC carries settings and status. The egui interface uses the game's window and rendering resources, with explicit input ownership and graphics-state restoration.

See [architecture](research/architecture.md) for contracts, [validation](research/validation-plan.md) for acceptance criteria, and [the tracker](implementation.md) for implementation status.
