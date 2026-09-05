# Implementation experiments and completion criteria

The complete first target is **AC7 with lower-resolution XeSS-SR, XeLL, and XeSS MFG on the Claw**, available through a standalone launcher with in-game egui and through WSGM profiles. SR-only and native-resolution AA are useful intermediate results. Neither completes this target.

No experiment below has run yet. These are proposed acceptance criteria, not a test report. The reference checkouts establish useful implementation approaches; they do not establish compatibility with the installed game or the Claw.

The subsequent static inspection is recorded in [ue418-hook-map.md](ue418-hook-map.md): the installed game is build 9855922, its executable fingerprint and graphics imports are known, and stock UE4.18 source provides concrete hook leads. Runtime acceptance remains pending. Include the controlled Vulkan comparison described in [presentation-backends.md](presentation-backends.md) when evaluating alternative backends.

## 1. Establish early loading and observe AC7

Build the smallest standalone loader, bootstrap, orchestrator, and AC7 plugin that can report startup and rendering boundaries. It should initially leave frame contents and resolution alone.

Record the installed executable's canonical path, SHA-256, PE architecture/version, Steam build ID where present, renderer, adapter LUID, driver, output mode, and loaded graphics wrappers. Detection should select a known plugin/build profile. Unknown or ambiguous matches should produce an unsupported-build result before installing game-specific patches.

Trace the following without launching a second game instance:

- Loader armed, actual game process entry, bootstrap match, runtime initialization, and plugin detection.
- DX11 device and DXGI factory creation, swap-chain creation, first back-buffer acquisition, and first present.
- Candidate frame/input, game-thread, render-thread, RHI-thread, TAA, post-process, and HUD boundaries.
- The final game process identity after any Steam or launcher handoff.

**Pass:** the runtime reaches the required boundaries before AC7 retains the corresponding objects, and the game renders normally with observation enabled. Both standalone launch and the relevant Steam launch route must have explicit evidence. A late successful DLL load is not an early-loading pass.

If the hook-host route cannot achieve reliable startup ordering on this game, use the already planned DXGI shim to isolate renderer work while fixing the launcher. Record the limitation; do not quietly redefine shimming as the preferred product architecture.

## 2. Prove the mixed DX11/DX12 bridge independently

Create a small DX11 test application that uses the same orchestrator and presentation code intended for AC7. Render moving geometry, a moving camera, transparency, a fixed UI layer, and a frame-ID pattern. The test program supplies known depth, motion, jitter, exposure, and timing.

Implement the DX11-facing swap-chain facade, a same-adapter DX12 device/queue, shared resources, fences, and the XeSS-FG proxy. Establish ordinary presentation with interpolation disabled before enabling XeLL and FG.

| Experiment | What it must establish |
| --- | --- |
| Passthrough bridge | Same frame contents and correct DX11 outward interfaces; no adapter mismatch or second HWND swap chain |
| Shared-resource stress | No reuse before GPU completion, correct resource states, bounded memory, and no stale frames |
| XeLL sequencing | One sleep before input per rendered frame; valid marker pairs; IDs survive the application/render handoff |
| XeSS 2x | One generated frame per rendered frame when supported; valid resource tagging and presentation status |
| XeSS 3x/4x | Two/three generated frames where the actual Intel device, driver, and initialized SDK support them |
| UI composition | Fixed text and translucent UI remain sharp and correctly blended across generated frames |
| Lifecycle | Resize, minimize/restore, focus changes, tearing/VRR choices, and clean shutdown preserve the facade contract |

Use SDK validation and XeSS Inspector where supported, plus the graphics debug layers in the synthetic application. Debug instrumentation can change timing, so collect performance evidence separately with it disabled.

A non-Intel development GPU can help validate the bridge and supported cross-vendor modes. It cannot prove the Claw's XMX execution or Intel-only higher interpolation counts. Test the actual Claw early enough that a mixed-API XeLL limitation can change the design before it spreads through the codebase.

**Pass:** visual correctness, correct GPU ownership, stable presentation, and valid markers. Measure latency against bridge-only and native presentation. Successful context creation or a larger FPS counter alone is insufficient.

## 3. Capture and understand AC7's temporal inputs

Use the observer to locate the TAA pass and inspect its resources. Source leads include Luma's shader classification, the ultrawide mod's HUD shaders, and UEVR/UESDK/patternsleuth engine discovery. Revalidate every lead against the installed build; no reference hash is automatically an AC7 compatibility signature.

Capture a small set of diagnostic frames covering stationary-camera object motion, camera-only motion, combined motion, and a camera cut. Establish:

- The actual TAA input and output allocation sizes, viewport rectangles, colour formats, and ordering relative to tone mapping and spatial scaling.
- Depth convention, valid background values, and the matrices used to reconstruct camera motion.
- Velocity packing, sign, units, jitter treatment, object-motion coverage, and dilation state.
- How the game supplies jitter, previous matrices, exposure/pre-exposure, and temporal-history reset signals.
- Which passes contain clouds, cockpit glass, rain, smoke, targets, subtitles, and menus.
- Which resources survive until the final present and which need an owned copy.

Convert the game inputs on the GPU and visualize them in diagnostics. Do not treat a plausible-looking vector image as sufficient: compare signs and scale against known camera/object movement. Keep separate motion representations where SR and FG require different dilation.

**Pass:** each required input has an identified producer, explicit convention, valid lifetime, and verified per-frame association. If the game does not supply an input, the plugin must identify a defensible reconstruction or report the feature unavailable.

## 4. Prove true SR and larger-output reinsertion

First replace native-resolution TAA as a controlled integration check. Then lower scene rendering below output resolution using an SDK-recommended input size and return a genuinely larger reconstructed texture into the post-processing chain.

The key experiment is the larger output. Determine exactly which downstream render targets, viewport/scissor values, shader constants, and resource bindings must change. Trace and remove a redundant spatial upscale when applicable. Keep HUD at output resolution.

Compare these cases at the same final output dimensions:

1. Original game rendering and AA.
2. Runtime present, effects disabled.
3. Native-resolution XeSS AA.
4. Lower-resolution game rendering with its original scaling path.
5. Lower-resolution XeSS-SR with the new output insertion.

This separates runtime overhead, AA replacement, lower-resolution savings, and actual reconstruction quality. Record input/output dimensions from the GPU path, not just the settings menu.

**Pass:** the scene is rendered at the requested lower dimensions, reconstructed at output dimensions, consumed correctly by post-processing, and combined with a sharp HUD. The result has useful GPU/frame-time savings at matched output conditions. If only native-resolution AA works, this milestone remains incomplete.

## 5. Integrate AC7 MFG and real frame timing

Connect the proven bridge to AC7. Begin with one generated frame, then exercise all supported higher counts on the Claw. Query maximum interpolation support rather than enabling menu choices from a GPU name.

Establish the actual before-input boundary and carry the source-frame ID through the game/render/RHI handoff. Correlate it with the colour, depth, motion, constants, and present ID. A render thread may be working on the previous simulation frame; the most recent global counter is not a reliable association.

Capture HUD-less colour after all relevant scene post-processing in the final back-buffer dimensions, format, and colour space. Start with the SDK composition mode that matches the available data. Add a UI-only texture when its alpha and blend rules are understood. Keep the egui menu in this same composition system.

Verify in representative AC7 situations:

| Situation | Main concern |
| --- | --- |
| Fast banking and low-altitude terrain | Camera motion, disocclusion, and edge stability |
| Cockpit, chase, and alternate cameras | Reset events, cockpit geometry, and HUD alignment |
| Clouds, smoke, rain, and glass | Missing/incorrect motion and transparency ordering |
| Target boxes, missiles, subtitles, and menus | Sharp UI, additive/semitransparent elements, and correct capture boundary |
| Cutscenes, loading, pause, and mission transitions | History reset and per-frame interpolation eligibility |
| egui open/close and focus loss | Input ownership, UI composition, and recovery |

**Pass:** SR and each supported MFG mode work together with meaningful XeLL markers and stable presentation. SDK status confirms effective interpolation; independent presentation/latency evidence agrees. A 2x result on the development laptop does not complete Claw MFG validation.

If XeLL cannot provide useful pacing in the mixed-API arrangement, record that result and investigate the submission architecture. Do not fabricate DX11 timing by placing all markers at `Present`, or describe FG as reducing simulation/input latency.

## 6. Measure whether it helps the handheld

Use the same repeatable mission segment, camera route, output dimensions, game settings, power limit, and refresh/VRR mode. Warm shader/model caches before timing. Repeat enough times to distinguish an improvement from run-to-run variation. Include a thermally settled run rather than only a cold burst.

Compare native rendering, lower-resolution XeSS-SR, and SR plus each supported MFG setting. Include runtime-disabled/bridge-only baselines when investigating overhead. On the Claw, compare native DX11 SR against the optional DX12 SR round trip before changing the recommended default.

Record:

- Rendered FPS and rendered-frame-time distribution, including tail spikes.
- SDK-reported generated/presented frame counts separately from rendered FPS.
- Observed presentation cadence and dropped/repeated frames using a suitable presentation trace or capture.
- SR, resource conversion/copy, synchronization, and MFG GPU costs.
- Memory use, power, thermal behavior, and sustained battery-oriented performance.
- Input-to-display latency using an appropriate external measurement method when available. Mark SDK/CPU estimates as estimates.
- Visual defects with the triggering scene and exact settings.

Choose the multiplier from the sustainable base frame rate and panel refresh budget. A nominal 4x setting can be worse than 2x if its extra GPU work reduces the base frame rate or increases latency. Do not count generated frames as new simulation frames or assume a universal latency benefit.

No fixed performance promise is justified before these measurements. Acceptance requires a useful configuration for handheld play, with its tradeoffs documented.

## 7. Finish standalone and WSGM operation

Ship the same runtime packages in both modes. The standalone application must launch the game, persist profiles, show supported options, and expose in-game egui without WSGM running. Test keyboard/mouse and actual handheld controller navigation, including DPI scaling and focus recovery.

For WSGM, integrate both launch paths: the managed launcher and the native Steam Input lease path. Preserve Steam arguments/environment, de-elevation, process-tree lifetime, and exit handling. A running-game observer alone does not establish early loading.

Use WSGM's canonical application ID and a single persistence authority per session. Settings should report requested versus effective state, unavailable-feature reasons, and restart requirements. Closing a frontend must not stall the render thread.

Coordinate frame limiting and AutoTDP. Verify that RTSS/game/XeLL pacing policies do not compete, that prior settings are restored, and that AutoTDP uses rendered timing and the intended rendered target rather than generated FPS. Upscaling must still work with device integration disabled.

**Pass:** both launchers select the same plugin and apply equivalent settings; the standalone experience is complete; WSGM profile and power policies remain consistent when MFG is enabled.

## Delivery order

1. Native Game SDK skeleton, observer runtime, and standalone early loader.
2. Synthetic DX11/DX12 presentation and XeSS MFG/XeLL bridge, with an early Claw test.
3. AC7 input capture and true SR reinsertion.
4. AC7 frame identity, HUD composition, MFG, and latency validation.
5. Shared profile commands, standalone egui, and WSGM launch/AutoTDP integration.
6. Release packaging, compatibility metadata, notices, and a concise tested-settings report.

Keep compatibility evidence tied to executable hash, runtime/plugin/SDK versions, GPU/driver, and settings. Reuse the same diagnostic harness when a change touches resource ownership or presentation. Broader backend support can follow once this first game proves the architecture.

The final report must distinguish source-inspected, built, synthetic-tested, AC7-tested, and Claw-tested results. None of those labels implies the others.
