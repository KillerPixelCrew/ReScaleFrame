# Rendering research

Research began on 5 September 2026. The source comparisons are pinned to the revisions in [source-map.md](source-map.md) and `evidence/repositories.json`. Later AC7 captures and DLSS runs are recorded separately.

| Document | Read it for |
| --- | --- |
| [AC7 frame capture](ac7-frame-capture.md) | Resource contents, motion encoding, camera offsets, jitter, reduced-scale HUD behaviour |
| [AC7 overlay device mismatch](ac7-overlay-device.md) | Reproduced view-creation crash, swap-chain device selection, pixel readback and resize checks |
| [AC7 UI composition](ac7-ui-composition.md) | How the interface reaches the frame, the 1920x1080 converter, the briefing tail order, and the retracted promotion attempts |
| [AC7 UI extraction](ac7-ui-extraction.md) | The design that replaces promotion: producers, classification, divert, alpha, composite; per-screen results as they land |
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

The interface question that used to come first is answered in [AC7 UI composition](ac7-ui-composition.md): the interface is rasterized at 1920x1080 and drawn into the scene as widget quads on the screens that look soft, so it is extracted rather than promoted. The [tracker](../implementation.md) carries the milestones and the planned egui inspection/capture workflow. The [repository review](../review.md) lists concrete code defects.

Reference checkouts, vendor binaries, Epic source, raw captures, and game dumps stay outside Git. Publish methods, permitted metadata, observations, and uncertainty here.
