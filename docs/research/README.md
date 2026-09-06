# AC7 upscaling and multi frame generation research

Research date: 5 September 2026.

The first complete target is **Ace Combat 7, DX11 x64, with XeSS-SR on XMX, XeSS Multi Frame Generation, and XeLL on the MSI Claw 8**. SR alone is an intermediate milestone. The wider project will support DLSS, FSR, and XeSS through the same orchestrator.

The agreed architecture is workable: a launcher loads an in-game orchestrator, which loads a separate game plugin through a small Game SDK. Game plugins prepare their renderers and provide the required inputs. The orchestrator owns vendor integrations and presentation. The same runtime works independently with an in-game egui menu or under WSGM's per-game profiles.

Read [architecture.md](architecture.md) for the design and its reasoning, [validation-plan.md](validation-plan.md) for the experiments that must establish AC7 compatibility, and [source-map.md](source-map.md) for source links pinned to the inspected revisions.

The follow-up [UE4.18 hook map](ue418-hook-map.md) uses the authorized Epic source checkout and the installed AC7 executable. [Vulkan versus DX12](presentation-backends.md) compares the presentation options and Community Shaders' current Vulkan work.

[Binary analysis tooling](ghidra-tooling.md) covers the Ghidra tooling this project had to write and records why the shipped executable cannot be analysed statically. [Frame capture](ac7-frame-capture.md) records the render targets the game actually allocates, including the two velocity textures.

## What the research established

- NVIDIA's upstream Streamline does not supply the complete DLSS/FSR/XeSS SR-and-MFG stack. Community Shaders' custom fork adds Vulkan XeSS-SR and FSR integrations; its XeSS plugin is SR-only. Our orchestrator should keep its own common interface.
- XeSS-SR supports Vulkan. Current native XeSS-FG/MFG and XeLL support is DX12; Vulkan rendering with XeSS MFG would need a mixed-API path.
- Intel provides a native DX11 XeSS-SR path for Arc hardware. XeSS-FG and XeLL require DX12. Our recommended Claw path keeps SR in DX11 and uses a DX12 presentation bridge for MFG.
- MFG supports choosing the number of generated frames through the SDK. The runtime must query the available maximum and initialize accordingly; 2x, 3x, and 4x must not be exposed unconditionally.
- Skyrim and Fallout 4 Community Shaders contain concrete examples of presenting DX11 rendering through a DX12 swap chain. They are useful implementation references, not proof of XeSS MFG working in AC7.
- Luma's generic Unreal implementation explicitly sets the SR scale to 1.0 and calls it `DLAA only`. Its AC7 compatibility listing supports TAA-replacement feasibility, not a claim that lower-resolution reconstruction or MFG is already solved.
- Special K explicitly prefers early global injection. SKIF manages the injection host; the payload implements the hooks. That separation fits WSGM integration and a standalone launcher.
- AC7's UE4.18 branch predates Epic's later temporal-upsample pipeline. Returning a larger reconstruction result to the post-processing chain is a real game-plugin task.
- A settings overlay, target markers, cockpit UI, and other HUD elements need deliberate MFG composition. Presenting a completed frame alone is insufficient for this project.

## Research workspace

Source-focused reference checkouts and Intel guides were inspected in a separate local research workspace. They are not included in this repository. `evidence/repositories.json` records exact revisions and checkout selections, and the source map links to the original guides and source files. Vendor binaries and recursive submodules were not fetched for this inspection.

The existing WSGM repositories were inspected read-only. Their code, launch settings, game files, and installed mods were not changed. No injector was run. No source inspection here constitutes an AC7 or Claw runtime pass.

## Remaining decisive questions

1. Can we reliably identify AC7's real frame start, TAA pass, motion/depth inputs, and first HUD draw on the installed build?
2. Can the plugin introduce a larger SR output without breaking downstream post-processing or cloud rendering?
3. Does the DX11/DX12 bridge provide stable MFG pacing and useful XeLL behavior on the Claw?
4. Do the combined visual quality, latency, power use, and rendered frame times improve the replay experience?

The first implementation should answer those questions with a small diagnostic runtime and a mixed-API graphics test program before expanding the framework.
