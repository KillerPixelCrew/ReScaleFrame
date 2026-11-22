# Design

ReScaleFrame joins game-specific rendering hooks to shared SR, FG and latency services. Current
AC7 and Unity paths implement much of the native runtime; standalone loading and frontend IPC
remain intended architecture. [Current status](current-status.md) separates implementation and
recorded acceptance from plans and the published SR-only AC7 package.

## Ownership

| Component | Owns |
| --- | --- |
| Launcher or WSGM, planned | Game selection, profile storage, early loading, session lifetime |
| Bootstrap, currently version-only | Intended general runtime-loading entry point |
| Current carriers | AC7 DirectInput entry and Unity version shim; runtime/plugin loading |
| Orchestrator | Plugin lifecycle, vendor SDKs, resources, settings, latency and presentation |
| Game plugin | Build recognition, engine hooks, frame data and SR reinsertion |
| Game SDK | Versioned C contract (`sdk/game/include/rescaleframe/game_api.h` holds the current ABI version), with leased native resources |
| egui | Settings intents, effective status and FPS display; inspection/capture expansion planned |

First-party components share a repository and release version. C interfaces keep ownership explicit.
Plugins do not load competing vendor contexts. AC7 links runtime objects into its proxy; Unity
loads the shared runtime DLL. Loading/preparing a plugin precedes graphics activation.

## Frame flow

1. The plugin identifies CPU frame/input boundaries and copies identity through renderer submission.
2. At reconstruction, it supplies pre-tonemap colour, depth, motion, jitter, camera state and rectangles.
3. The runtime evaluates SR at that execution boundary and feeds downstream native post-processing.
4. FG uses the completed display-size frame and matching normalized depth/motion, with available guides.
5. One presentation provider handles interpolation, pacing and actual presentation; stale or unmatched inputs refuse.

SR and FG colour are different stages. Retaining a texture preserves its allocation, not contents.
Resource leases and GPU fences govern reuse. Generated frames do not advance simulation.

AC7 keeps native UI raster/composition owners at output resolution. The earlier experiment diverting
widget composites to a mod-owned layer produced incorrect colour and processing and was superseded.
Full-resolution widgets are not proof of a compositable FG UI layer. Current source can copy eligible
native family colour before Slate as an optional guide, retaining unclassified-UI fallback.
Unity's SDR pre-UI hook captures completed colour after post-processing; HDR refuses that guide path.
Neither route establishes every provider's moving-scene/HUD quality.

## Retained native code

The Rust crates own the runtime; native C remains only where Rust cannot express the work: MinHook
calls, COM calls on vendor objects, and guards for foreign C++ exceptions. MSVC raises a C++ throw
as SEH code `0xE06D7363`, so each C11 guard is a `__try` whose filter accepts only that code, the
filter `runtime/orchestrator/src/plugin_callbacks.c` uses for what a `catch (...)` stopped under
`/EHsc`. Access violations and other faults still propagate to the caller. A Rust panic is also
raised as `0xE06D7363` on MSVC, so panics stay contained. Scopes that C++ destructors closed on
unwind use `__try/__finally`. One guard deliberately keeps `EXCEPTION_EXECUTE_HANDLER`: the Wingman
`acquire_texture` engine/COM probe, which treats any fault as "no texture". Moving the Wingman report
and the D3D11 observer COM calls into Rust was considered and rejected: they are single vendor-vtable
calls whose only purpose is to sit inside the exception frame.

## Backends and frontends

AC7 keeps a D3D11-facing facade and uses same-adapter D3D12 services for D3D12-only SR/FG paths.
Drag'n Wash uses native D3D12. Native XeSS D3D11 SR remains unimplemented; the earlier proposed Claw
DX11 route is research history. Claw hardware acceptance and any alternative Vulkan/DXVK route need
separate evidence.

SR and FG selection are independent. Live FG replacement drains the old provider and preserves
engine buffers. Query actual device/SDK capabilities; report requested, effective and active state
separately. DLSS-G uses Reflex, XeSS uses XeLL and FSR owns its pacing; one provider owns presentation.
The pinned AMD FG path is 2x; larger DLSS/XeSS counts are capability-gated and not proven here.

Standalone and WSGM are intended to use bounded configuration/status IPC, without GPU textures or
per-frame frontend loops. They are not implemented launch/control routes. The shared overlay uses
the game's window/resources with explicit input and graphics-state ownership.

See [architecture history](research/architecture.md), [FG design](frame-generation-plan.md),
[validation targets](research/validation-plan.md) and [the tracker](implementation.md).
