# Rendering research

- [Skyrim SE integration](skyrim-se-integration.md): 1.7.104 fingerprint, current Community Shaders source leads and standalone coexistence requirements.


Use [current status](../current-status.md) for implemented/released features and acceptance limits.
These notes are dated evidence and development history. Older pending claims do not override later
corrections, and a source inspection is not a new runtime test. Published evidence snapshots keep
their original measurements, revisions and hashes.

Research began on 5 September 2026. The source comparisons are pinned to the revisions in [source-map.md](source-map.md) and `evidence/repositories.json`. Later AC7 captures and DLSS runs are recorded separately.

Recorded SR acceptance, 2 October: the user accepted the native SR/UI path, corrected TrueSky wing/cloud
production and optimized SR bridge/FSR4 compatibility deployment. Subsequent AC7 DLSS-G and
shared Unity FG acceptance are recorded independently. Missing-object motion, higher MFG/Claw
coverage and measured latency remain open. Earlier pending notes preserve the investigation trail. [Maintained skills and session corrections](skills-session-update-20261002.md)
route general Unreal 4, version-specific UE4.18 and TrueSky research.

| Document | Read it for |
| --- | --- |
| [Rule 5 audit](rule5-audit.md) | Where technique sits in integrations and game knowledge in the core, parity gaps, missing inputs, and the ranked follow-ups |
| [Godot engine integration](godot-engine-integration.md) | Developer-facing GDExtension/module, renderer probes, standard/.NET interop, DX12 runtime harness and remaining Vulkan/export work |
| [Shared FSR/XeSS generation](shared-fg-20261004.md) | Independent SR/FG selection, live provider replacement, Unity input/pacing corrections, device fixtures and final user acceptance |
| [Overlay parity across games](overlay-parity-20261125.md) | One Runtime source for the SR/FG choices every game offers, the audit of what differed and why, and the follow-ups |
| [Unity DX12 SR runtime](unity-dx12-runtime-20261004.md) | Shared shim, Mono/Harmony contracts, live SR execution, jitter/order corrections and Auto policy |
| [AC7 motion/depth follow-up](ac7-motion-depth-20261004.md) | Dense motion, translucency hints, cloud depth and material mip bias with separate validation scopes |
| [AC7 lighting/shadow investigation](ac7-lighting-shadow-20261004.md) | Later unresolved low-resolution stippling, shader transforms and capture requirements |
| [Shared Unity Mono plugin](unity-mono-plugin.md) | Reflection/Harmony adapters, existing-domain bootstrap, RenderGraph execution and lifecycle design |
| [Drag'n Wash renderer](drag-n-wash-renderer.md) | First Unity case: 6000.3.14f1 Mono/URP, complete selected assembly decompiles, temporal/UI boundaries and unmeasured runtime limits |
| [AC7 DLSS-G/Reflex MVP](ac7-dlss-fg-20261003.md) | Shared presentation/SR host, native CPU/Present identity, recorded flight/hangar activation, hybrid-laptop pacing investigation and limits |
| [Project Wingman renderer](project-wingman-renderer.md) | UE4.27.2 fingerprint, Function ID results, temporal/widget hook map and remaining HUD/VR validation |
| [Red Dead Redemption 2 renderer](rdr2-renderer.md) | RAGE fingerprint, encrypted code, native Reflex/FSR2/NGX leads, reference mod review, DX12 FG requirements and open research |
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
the runtime supplies DLSS, FSR and XeSS plus ordered D3D11/D3D12 transfers. That SR acceptance alone does not establish FG or latency. Later
[AC7 DLSS-G work](ac7-dlss-fg-20261003.md) and [shared FG corrections](shared-fg-20261004.md)
record runtime switching, device activity and Unity user acceptance separately. The Claw target and standalone/WSGM
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

## Port and cutover evidence notes

These notes record how each C++ owner moved to Rust, with the layouts, offsets, SDK hashes and game observations the move depended on. Notes that held only source-review narration were removed; Git history records every cutover. They are grouped by area; `rust-anomaly-cpp-baseline.md` holds the Standard and AVX executable hashes the C++ adapter baseline was recorded against.


### AC7

- [AC7 carrier startup and renderer lifecycle](rust-ac7-a01.md)
- [AC7 carrier startup policy in Rust](rust-ac7-carrier-startup.md)
- [AC7 game entry in Rust](rust-ac7-entry.md)
- [AC7 F9 capture artifact publication in Rust](rust-ac7-motion-artifacts.md)
- [AC7 F9 before-draw rows in Rust](rust-ac7-motion-before-draw.md)
- [AC7 F9 dispatch records in Rust](rust-ac7-motion-dispatch.md)
- [AC7 F9 ordinary draw records in Rust](rust-ac7-motion-draws.md)
- [AC7 F9 geometry rows in Rust](rust-ac7-motion-geometry.md)
- [AC7 F9 renderer-root registration in Rust](rust-ac7-motion-root-admission.md)
- [AC7 F9 capture session in Rust](rust-ac7-motion-session.md)
- [AC7 F9 constant-buffer observations](rust-ac7-motion-uploads.md)
- [AC7 native renderer in Rust](rust-ac7-native-renderer.md)
- [AC7 native renderer sites in Rust](rust-ac7-native-sites.md)
- [AC7 queued render scopes in Rust](rust-ac7-render-scopes.md)
- [AC7 native renderer transaction in Rust](rust-ac7-renderer-transaction.md)
- [AC7 TrueSky patches and relays in Rust](rust-ac7-truesky-relays.md)
- [AC7 velocity shader and replay ownership](rust-ac7-velocity-a04-a05-residual.md)
- [AC7 D3D11 velocity replay and WARP fixture port](rust-ac7-velocity-d3d11.md)
- [AC7 velocity geometry shader assembly](rust-ac7-velocity-geometry.md)
- [AC7 velocity-pass model and view history in Rust](rust-ac7-velocity-history.md)
- [AC7 velocity pass and history port](rust-ac7-velocity-pass-a05.md)
- [AC7 velocity-pass sources and variant admission in Rust](rust-ac7-velocity-pass-cache.md)
- [AC7 velocity hull and domain shader assembly](rust-ac7-velocity-tessellation.md)
- [AC7 website input dump](ac7-website-input-dump.md)

### Anomaly

- [Anomaly cold entry in Rust](rust-anomaly-bootstrap.md)
- [Anomaly C++ adapter baseline on the Rust runtime](rust-anomaly-cpp-baseline.md)
- [Anomaly frame-generation stage in Rust](rust-anomaly-fg-stage.md)
- [Anomaly motion history in Rust](rust-anomaly-motion-history.md)
- [Anomaly motion-stage owner in Rust](rust-anomaly-motion-stage.md)
- [Anomaly input and overlay port](rust-anomaly-n02.md)
- [Anomaly native geometry in Rust](rust-anomaly-native-geometry.md)
- [Anomaly native HookSet in Rust](rust-anomaly-native-hook-set.md)
- [Anomaly native observer in Rust](rust-anomaly-native-observer.md)
- [Anomaly overlay probe policy in Rust](rust-anomaly-overlay-probes.md)
- [Anomaly shader loading port](rust-anomaly-shader-loader-n05.md)
- [Anomaly shader preparation in Rust](rust-anomaly-shader-loader.md)
- [Anomaly motion manifest owner in Rust](rust-anomaly-shader-manifest.md)
- [Anomaly SR configuration owner in Rust](rust-anomaly-sr-configuration.md)
- [Anomaly SR D3D11 resource and pass port](rust-anomaly-sr-d3d11.md)
- [Anomaly SR pass construction in Rust](rust-anomaly-sr-pass.md)
- [Anomaly SR stage driver in Rust](rust-anomaly-sr-stage.md)
- [Anomaly SR world-window swap in Rust](rust-anomaly-sr-swap.md)
- [Anomaly startup and observation source port](rust-anomaly-startup-n01.md)

### Fallout 4

- [Fallout 4 camera math in Rust](rust-fallout4-camera.md)
- [Fallout 4 depth-domain admission in Rust](rust-fallout4-depth-domains.md)
- [Fallout 4 render-input ownership in the F02 port](rust-fallout4-f02.md)
- [Fallout 4 World and Present admission in Rust](rust-fallout4-render-hooks.md)
- [Fallout 4 render-input admission in Rust](rust-fallout4-render-inputs.md)
- [Fallout 4 producer resolution in Rust](rust-fallout4-resolution.md)
- [Fallout 4 capability and latency admission audit, 16 November 2026](fallout4-capability-admission-audit-20261116.md)
- [Fallout4 early DPI startup, 16 November 2026](fallout4-dpi-startup-20261116.md)
- [Fallout 4 matched native capture numeric audit](fallout4-native-capture-audit-20261116.md)
- [Fallout 4 native close access violation, 16 November 2026](fallout4-native-close-av-20261116.md)
- [Fallout 4 native shader-input audit, 16 November 2026](fallout4-native-shader-input-audit-20261116.md)
- [Fallout 4 reduced Main-colour producers, 2026-11-16](fallout4-reduced-colour-producers-20261116.md)

### Skyrim SE

- [Skyrim CPU source and render ownership in Rust](rust-skyrim-cpu-ownership.md)
- [Skyrim SE native creation in Rust](rust-skyrim-native-creation.md)
- [Bounded Skyrim display validation preparation, 15 November 2026](skyrim-presentmon-validation-20261115.md)

### RDR2

- [RDR2 ASI startup and engine settings in Rust](rust-rdr2-asi.md)
- [RDR2 capture ownership and diagnostics](rust-rdr2-capture.md)
- [RDR2 game plugin in Rust](rust-rdr2-plugin.md)

### Unity

- [Unity D3D11 queued packet texture leases](rust-unity-d3d11-packet-leases.md)
- [Unity D3D11 pass plan, route and retirement verdict](rust-unity-d3d11-pass-plan.md)
- [Unity D3D11 shared surface ownership](rust-unity-d3d11-surfaces.md)
- [Unity Mono startup and configuration port](rust-unity-mono-startup.md)
- [Unity native bridge leaves](rust-unity-native-bridge-leaves.md)
- [Unity Editor preview resource ownership](rust-unity-preview-resources.md)
- [Unity Vulkan command resource ownership](rust-unity-vulkan-command-resources.md)
- [Unity Vulkan DXGI swapchain ownership](rust-unity-vulkan-dxgi.md)
- [Unity Vulkan shared graphics device ownership](rust-unity-vulkan-shared-device.md)

### Godot

- [Godot engine module Rust ownership](rust-godot-engine-e01.md)

### Vulkan

- [Standalone Vulkan capture submission](rust-vulkan-capture-submit-cutover.md)
- [Rust DLSS Vulkan frame-generation owner](rust-vulkan-dlss-fg-cutover.md)
- [Rust DLSS Vulkan present capture owner](rust-vulkan-dlss-present-capture.md)
- [Vulkan-to-D3D12 FG transfer plan in Rust](rust-vulkan-dx12-copy-plan.md)
- [Vulkan-to-D3D12 FG interop and guide retirement](rust-vulkan-dx12-interop.md)
- [Native FidelityFX Vulkan FG HUD-less copy in Rust](rust-vulkan-ffx-fg-copy-plan.md)
- [Vulkan FidelityFX 3 frame generation Rust cutover](rust-vulkan-ffx-fg-cutover.md)
- [Vulkan FidelityFX 3 FG SDK leaf in Rust (V02)](rust-vulkan-ffx-fg-sdk-leaf.md)
- [FidelityFX Vulkan SR staging images in Rust](rust-vulkan-ffx-sr-images.md)
- [V05: FidelityFX Vulkan bridges without first-party C++](rust-vulkan-ffx-v05.md)
- [FidelityFX 3 Vulkan WSI route in Rust](rust-vulkan-ffx-wsi-owner.md)
- [Standalone FSR3 Vulkan bridge: binding, SR, FG and WSI in Rust](rust-vulkan-fsr3-legacy-sr.md)
- [Vulkan readback owner Rust port](rust-vulkan-readback-owner.md)
- [Vulkan sharpening Rust cutover](rust-vulkan-sharpening-cutover.md)
- [Vulkan Streamline call leaves in Rust (V04)](rust-vulkan-streamline-leaves.md)
- [Vulkan XeSS frame generation Rust cutover](rust-vulkan-xess-fg-cutover.md)
- [XeSS Vulkan bridge source port](rust-vulkan-xess-fg-v06.md)
- [Standalone VulkanStory NGX module in Rust](rust-vulkanstory-ngx-loader.md)
- [XeSS Vulkan super-resolution Rust cutover](rust-xess-vulkan-sr-cutover.md)
- [Vulkan character history bias](vulkan-character-history-bias-20261115.md)
- [Explicit Vulkan execution ownership](vulkan-explicit-execution-20261115.md)
- [Vulkan FG image enumeration](vulkan-fg-image-enumeration-20261115.md)
- [Generic Vulkan FG latency context attachment and retirement](vulkan-fg-latency-context-20261115.md)
- [Retain Vulkan FG owners for unsubmitted recordings](vulkan-fg-recorded-retirement-20261115.md)
- [Effective FSR4 provider query, 15 November 2026](vulkan-fsr4-provider-info-20261115.md)
- [Vulkan FSR4 SR submission leases, 15 November 2026](vulkan-fsr4-sr-leases-20261115.md)
- [Vulkan PCL registered-thread message delivery, 15 November 2026](vulkan-pcl-thread-delivery-20261115.md)
- [Vulkan SR native port follow-up](vulkan-sr-native-port-followup.md)
- [Vulkan SR provider admission port](vulkan-sr-provider-admission-port.md)
- [XeSS admission through the shared SR entry point](vulkan-xess-outer-admission-20261115.md)

### D3D11, D3D12 and OpenGL

- [D3D11 outer native-chain adoption in Rust](rust-d3d11-native-chain-adoption.md)
- [Shared D3D11 presentation bridge Rust cutover](rust-d3d11-present-cutover.md)
- [Presentation command slot ownership in Rust](rust-d3d11-present-slots.md)
- [Presentation synchronization ownership in Rust](rust-d3d11-present-sync.md)
- [D3D11 SR pipeline Rust cutover](rust-d3d11-sr-pipeline-cutover.md)
- [Native D3D12 frame generation Rust cutover](rust-d3d12-fg-cutover.md)
- [Native D3D12 overlay frame slot ownership in Rust](rust-d3d12-overlay-slots.md)
- [D3D12 readback owner Rust port](rust-d3d12-readback-owner.md)
- [Generic D3D12 SR Rust cutover](rust-d3d12-sr-cutover.md)
- [D3D12 Streamline generation and compatibility port](rust-d3d12-streamline-generation-port.md)
- [OpenGL shared-surface owner Rust port](rust-gl-shared-surface-cutover.md)
- [D04 D3D12 SR and generation host port](d04-d3d12-host-port.md)
- [Private presentation-chain usage investigation, 16 November 2026](d3d11-private-chain-usage-20261116.md)

### Overlay, graphics, loader and shared runtime

- [C05 native fixture port](rust-c05-native-fixtures.md)
- [C05, G02 and G03 native leaf closure](rust-core-leaves-closure.md)
- [Shared D3D11 frame tap Rust cutover](rust-frame-tap-cutover.md)
- [Generation ownership and lease port](rust-generation-ownership.md)
- [Graphics observer ownership port](rust-graphics-observers.md)
- [Rust colour, depth, motion and fullscreen passes](rust-graphics-passes.md)
- [Loader census and diagnostics Rust port](rust-loader-diagnostics.md)
- [Rust Direct3D frame-generation CPU owner](rust-native-fg-cpu-cutover.md)
- [Shared D3D11 translucency owner Rust cutover](rust-native-translucency-cutover.md)
- [Shared D3D11 overlay host in Rust](rust-overlay-host.md)
- [Generic D3D12 render host Rust cutover](rust-render-host-cutover.md)
- [Runtime shader patch service in Rust](rust-shader-patch.md)
- [Shared texture and frame capture](rust-shared-capture-port.md)
- [Shared overlay rendering and panel source port](rust-shared-overlay-o02.md)
- [G05 sharpening port review](rust-sharpening-g05.md)
- [Numeric packed-colour auxiliary transactions, 16 November 2026](auxiliary-numeric-packed-colour-20261116.md)
- [Immediate DLSS present pixels after dedicated capture fences](dlss-present-immediate-legacy-flush-20261115.md)
- [FSR and XeSS generation state invalidation](generation-provider-state-invalidation-20261116.md)
- [Copied depth domains for native input normalization, 16 November 2026](native-depth-domain-contract-20261116.md)
- [Native marker admission, 16 November 2026](native-marker-admission-20261116.md)
- [Direct overlay service and panel admission](overlay-direct-admission.md)
- [Typed Windows overlay renderer and fixture port](overlay-native-sdk-port.md)
- [Saved Unicode overlay options](overlay-saved-unicode.md)
- [Generic unframed input receipt routing proposal](unframed-input-receipt-routing-proposal-20261116.md)
- [DLSS backend notes: motion data, Streamline and Wine](dlss-backend-notes.md)
