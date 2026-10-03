# Rendering research

Research began on 5 September 2026. The source comparisons are pinned to the revisions in [source-map.md](source-map.md) and `evidence/repositories.json`. Later AC7 captures and DLSS runs are recorded separately.

Current status, 2 October: the user accepted the native SR/UI path, corrected TrueSky wing/cloud
production and optimized SR bridge/FSR4 compatibility deployment. Earlier dated pending notes
preserve the investigation trail. Missing-object motion coverage, FG and latency integration
remain separate work. [Maintained skills and session corrections](skills-session-update-20261002.md)
route general Unreal 4, version-specific UE4.18 and TrueSky research.

| Document | Read it for |
| --- | --- |
| [Shared Unity Mono plugin](unity-mono-plugin.md) | Reflection/Harmony adapters, existing-domain bootstrap, RenderGraph execution and lifecycle design |
| [Drag'n Wash renderer](drag-n-wash-renderer.md) | First Unity case: 6000.3.14f1 Mono/URP, complete selected assembly decompiles, temporal/UI boundaries and unmeasured runtime limits |
| [Native AC7 renderer](ac7-native-renderer-refactor-20261001.md) | Native view/graph/UI ownership, copied RHI identity, shared-uniform crash correction and retirement |
| [Reconstruction stability](ac7-stability-20261001.md) | Exact input rectangles, bloom/exposure order, TrueSky jitter and valid-zero motion |
| [Aircraft and cloud corrections](ac7-plane-artifacts-20261002.md) | TrueSky x1 activation, actual Texture2D views, RG32F depth bounds, FP16 production and user acceptance |
| [SR interop and FSR4](sr-interop-performance-20261002.md) | GPU fence handoff, three command slots, FSR4 INT8 capability/provider proof and measurement limits |
| [Repository skills update](skills-session-update-20261002.md) | Transcript-derived workflow, skill routing, historical corrections and focused checks |
| [AC7 frame capture](ac7-frame-capture.md) | Resource contents, motion encoding, camera offsets, jitter, reduced-scale HUD behaviour |
| [AC7 motion vectors](ac7-motion-vectors.md) | Native shader semantics, DLSS conversion discrepancy, captured unjittered reprojection, and RenoDX/Luma replacement routes |
| [AC7 renderer ownership audit](ac7-renderer-roots-20260930.md) | Captured GBuffer/post-process allocation reuse, lighting/reflection regression, native view/widget roots and engine-owned integration |
| [AC7 graph capture and pause crash](ac7-graph-capture-crash-20260930.md) | Runtime pass-to-draw identities, native FXAA/material/HUD allocations, captured pause image and borrowed-texture capture crash |
| [AC7 UI and colour investigation](ac7-ui-hdr-20260930.md) | Flight HUD producer and frozen-input colour comparisons, with the later upstream lifetime correction |
| [AC7 overlay device mismatch](ac7-overlay-device.md) | Reproduced view-creation crash, swap-chain device selection, pixel readback and resize checks |
| [AC7 UI composition](ac7-ui-composition.md) | How the interface reaches the frame, the 1920x1080 converter, the briefing tail order, and the retracted promotion attempts |
| [AC7 UI extraction](ac7-ui-extraction.md) | Producers, classification, divert, alpha, composite; the measurements that showed extraction skips AC7's own interface processing, and the promotion route that replaced it |
| [D3D11 runtime vtable rewrite](d3d11-runtime-vtable-rewrite.md) | Why no Windows run ever observed a draw: the stock runtime rewrites the work-submission entries of its heap vtable on every flush, and how the frame tap survives it |
| [Present hook coexistence](windows-present-hook-coexistence.md) | The first Windows game run: Steam's overlay and RivaTuner both re-assert Present's entry, the chain-following detour that sits beside them, the user32 detours that make the panel's mouse work, and the DPI-sized swap chain |
| [Frame generation contracts](vendor-fg-contracts.md) | What DLSS-G, FidelityFX and XeSS-FG require of a UI layer, HUD-less colour, the swap chain and frame identity, with citations |
| [Presentation bridge](presentation-bridge.md) | The DX11 to DX12 facade, sharing and synchronisation design, and what the fixtures can prove |
| [UE4.18 hook map](ue418-hook-map.md) | Engine source leads and the researched executable fingerprint |
| [Binary analysis](ghidra-tooling.md) | Why a runtime dump was needed and how it was checked |
| [Reflection mapping](ue-reflection-mapping.md) | Joining a reflection SDK dump to the binary through Unreal's registration arrays |
| [Methodology](methodology.md) | A repeatable analysis workflow |
| [Architecture](architecture.md) | Plugin contracts, interop, presentation, and WSGM integration |
| [Presentation backends](presentation-backends.md) | Native DX11/DX12 and Vulkan options |
| [Validation plan](validation-plan.md) | Experiments and completion criteria |

The early SR input chain was game-tested on 7 September. The 2 October implementation uses
engine-owned view sizing, a native SR/fallback graph node and coordinated UI/cloud producers;
the runtime supplies DLSS, FSR and XeSS plus ordered D3D11/D3D12 transfers. That accepted SR
path does not complete the presentation/FG/latency milestones in
[the representation plan](../representation-plan.md). The Claw target and standalone/WSGM
operation remain unproven as a complete combination.

The interface investigation in [UI composition](ac7-ui-composition.md) and
[UI extraction](ac7-ui-extraction.md) explained why extracting widget composites discarded
later native processing. Texture promotion was an intermediate approach. The accepted native
implementation fixes physical target/raster density through actual converter/game-instance
owners while preserving logical layout and native composition. The [tracker](../implementation.md)
separates that accepted scope from remaining framework work.

Reference checkouts, vendor binaries, Epic source, raw captures, and game dumps stay outside Git. Publish methods, permitted metadata, observations, and uncertainty here.

The 30 September on/off capture qualifies the earlier accepted SR input evidence: an allocation
used for the tonemap output is a GBuffer earlier in the frame, and premature substitution corrupts
lighting/reflection inputs. The [ownership audit](ac7-renderer-roots-20260930.md) records the
capture-proven defect, phase correction and native graph/UI migration route. The user confirmed
lighting/reflections restored. Later jitter/shimmer and native migration work is covered by
the accepted 2 October corrections above; early evaluation counts alone did not prove image quality.

- [Native AC7 renderer refactor](ac7-native-renderer-refactor-20261001.md): development history,
  MSVC/WARP contracts and later user acceptance, with remaining FG/latency coverage kept distinct.
