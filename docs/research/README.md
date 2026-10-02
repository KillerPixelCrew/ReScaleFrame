# Rendering research

Research began on 5 September 2026. The source comparisons are pinned to the revisions in [source-map.md](source-map.md) and `evidence/repositories.json`. Later AC7 captures and DLSS runs are recorded separately.

| Document | Read it for |
| --- | --- |
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

The super resolution input chain is game-tested in AC7 as of 7 September 2026: jitter, translucent velocity, composed scene colour, translucent depth and the separate translucency layer at native all reach the backend. What remains is presentation, and the [representation plan](../representation-plan.md) covers it: UI extraction, the DX11 to DX12 bridge, and SR plus frame generation for DLSS, FSR and XeSS as one framework. The Claw target (XeSS-SR, XeLL, MFG, standalone/WSGM operation) is its last milestone; source inspection does not prove that combination works.

The interface question that used to come first is answered in [AC7 UI composition](ac7-ui-composition.md) and [AC7 UI extraction](ac7-ui-extraction.md): the interface is rasterized at 1920x1080 and drawn into the scene as widget quads on the screens that look soft. Extraction into a layer of our own was built and measured, and it discolours the frame because the quads are composites the engine keeps processing; so the layers the quads draw into are promoted instead, with the classifier naming them and the game compositing them. The [tracker](../implementation.md) carries the milestones and the planned egui inspection/capture workflow. The [repository review](../review.md) lists concrete code defects.

Reference checkouts, vendor binaries, Epic source, raw captures, and game dumps stay outside Git. Publish methods, permitted metadata, observations, and uncertainty here.

The 30 September on/off capture qualifies the earlier accepted SR input evidence: an allocation
used for the tonemap output is a GBuffer earlier in the frame, and premature substitution corrupts
lighting/reflection inputs. The [ownership audit](ac7-renderer-roots-20260930.md) records the
capture-proven defect, phase correction and native graph/UI migration route. The user confirms
lighting/reflections restored. New jitter/shimmer and the native migration remain pending;
earlier backend evaluation counts do not establish complete image quality.

- [Native AC7 renderer refactor](ac7-native-renderer-refactor-20261001.md): engine-owned view sizing, pre-tonemap graph node, queued identity and native resource retirement; MSVC/WARP checked, game validation pending.
