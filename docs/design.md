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

Step 4 is where AC7 departs from a stock engine: it rasterizes its interface at a fixed 1920x1080 and, on several screens, draws it as world-space widget quads into its own render-resolution layer before compositing. The runtime therefore does not receive a HUD-less image from the game; it makes one, by diverting every interface draw the plugin classifies into a mod-owned premultiplied `R8G8B8A8` layer at output resolution and compositing it back at present. The layer and the HUD-less copy are the same two inputs every frame generation SDK asks for. The frame record, the vendor-neutral backend contract, the facade over the swap chain, and the order of events per Present are specified in the [representation plan](representation-plan.md); the frame flow above is the summary of it.

## Backend and frontend choices

The planned Claw path uses native DX11 XeSS-SR and a same-adapter DX12 presentation bridge for XeSS MFG/XeLL. Vulkan/DXVK remains an alternative to measure. The API findings are tied to the SDK revisions in [presentation research](research/presentation-backends.md) and the frame generation requirements of all three vendors are in [vendor contracts](research/vendor-fg-contracts.md). No frame generation SDK runs on D3D11, so the bridge is on every path, not only the Claw's; the order the plan builds the vendors in is FidelityFX, then DLSS-G, then XeFG, because that is the order they can be measured on the development machine.

One provider owns FG/presentation at a time. Query capabilities from the actual device, driver, API, and SDK configuration, then report requested and effective settings separately.

Standalone and WSGM use the same runtime. Each session has one profile persistence authority; IPC carries settings and status. The egui interface uses the game's window and rendering resources, with explicit input ownership and graphics-state restoration.

See [architecture](research/architecture.md) for contracts, [validation](research/validation-plan.md) for acceptance criteria, and [the tracker](implementation.md) for implementation status.
