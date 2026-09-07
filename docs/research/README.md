# Rendering research

Research began on 5 September 2026. The source comparisons are pinned to the revisions in [source-map.md](source-map.md) and `evidence/repositories.json`. Later AC7 captures and DLSS runs are recorded separately.

| Document | Read it for |
| --- | --- |
| [AC7 frame capture](ac7-frame-capture.md) | Resource contents, motion encoding, camera offsets, jitter, reduced-scale HUD behaviour |
| [AC7 overlay device mismatch](ac7-overlay-device.md) | Reproduced view-creation crash, swap-chain device selection, pixel readback and resize checks |
| [UE4.18 hook map](ue418-hook-map.md) | Engine source leads and the researched executable fingerprint |
| [Binary analysis](ghidra-tooling.md) | Why a runtime dump was needed and how it was checked |
| [Methodology](methodology.md) | A repeatable analysis workflow |
| [Architecture](architecture.md) | Plugin contracts, interop, presentation, and WSGM integration |
| [Presentation backends](presentation-backends.md) | Native DX11/DX12 and Vulkan options |
| [Validation plan](validation-plan.md) | Experiments and completion criteria |

The current diagnostic path evaluates DLSS in AC7. Output reinsertion and egui wiring remain unfinished. The complete Claw target adds lower-resolution XeSS-SR, XeLL, MFG, and standalone/WSGM operation; source inspection does not prove that combination works.

Two integration questions come first: which exact pass/view supplies each temporal input, and how an output-resolution scene returns to post-processing without reducing HUD resolution. The [tracker](../implementation.md) covers these and the planned egui inspection/capture workflow. The [repository review](../review.md) lists concrete code defects.

Reference checkouts, vendor binaries, Epic source, raw captures, and game dumps stay outside Git. Publish methods, permitted metadata, observations, and uncertainty here.
