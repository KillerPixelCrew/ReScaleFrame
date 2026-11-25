# Rule 5 audit: inputs and output in the integration, the technique in the core

Audit of every integration and host against AGENTS.md Rule 5 (commit 5b24c866) and the top of
`.agents/skills/game-render-analysis/SKILL.md`, done on 25 November 2026 on
`NightHammer1000/rule5-audit` (branched from `rust/orchestrator-anomaly` at 5b24c866).

## Question and method

Where does ReScaleFrame break "one integration, every feature": technique living in an
integration, game knowledge living in the core, features some games have and others lack, and
inputs an integration could supply but does not?

Static source review only. No game, device fixture or visible window was started. Each finding
was located by search (game and engine names in `runtime/` and `loader/`; vendor calls, choice
masks, generated-frame limits, jitter and bias rules, settings validation and backend tables in
`games/`, `integrations/` and `loader/proxy`) and then read at the cited lines, including the
callers that decide whether it matters. Findings name what the code does, not what a run showed.
Where a claim rests on a field never being assigned (category D) it is a search result and says so.

The shape that explains most findings: there are eight places that host the technique, and only
four of them are in the core.

| Host | Where | Used by |
| --- | --- | --- |
| `rsf_d3d11_host_*` (`runtime/orchestrator/src/d3d11_host.rs`) | core | Skyrim SE, Fallout 4 |
| `rsf_render_host_*` (`render_host.rs`, D3D12) | core | RDR2 |
| `rsf_callback_host_*` (`callback_host.rs`, D3D12) | core | Unity Mono (Drag'n Wash) |
| `rsf_gl_*` (`gl_pipeline.rs`) | core | Minecraft |
| AC7 bridge (`loader/proxy/src/dlss_bridge.c`, `dinput8_proxy.c`) | AC7 carrier | AC7 |
| Anomaly stages (`games/anomaly/src/sr_temporal.rs`, `fg_stage.rs`, `overlay.rs`, `overlay_hooks.rs`) | adapter | Anomaly |
| Godot hosts (`integrations/godot/native/vulkan.rs`, `d3d12.rs`, `control.rs`); Vulkan since 6b788cb5 the core `rsf_vkhost_*` (`vulkan_host.rs`) | engine integration (D3D12), core (Vulkan) | Godot games |
| Unity package host (`integrations/unity/src/package_state.rs` and siblings) | engine integration | Unity package games |

Every host keeps its own choice list, its own settings validation and its own idea of which
provider Auto means. That is where the parity gaps in category C come from. The overlay-parity
worker (`NightHammer1000/overlay-parity`, new `runtime/orchestrator/src/sr_choices.rs`) is unifying
what the overlay offers; this audit lists the masks as findings for it and does not edit them.

## A. Technique in an integration

| # | Location | What it is | Why it violates Rule 5 | Fix |
| --- | --- | --- | --- | --- |
| A1 | `loader/proxy/src/dlss_bridge.c:479-606` (`overlay_tick`), `:402-446`, `:718-825` | The AC7 carrier applies every overlay intent itself: SR backend switch with its own restore on refusal (`:583-593`), quality and resize (`:418-446`), DLSS preset (`:410-416`), sharpening (`:402-408`), FG mode, generated frames, Reflex and frame limit (`:528-544`), FG provider request and deferred save (`:503-527`), and starts the D3D11 SR pipeline and frame tap (`:718-825`). | Provider switching, fallbacks, settings application and the overlay belong to the core. The same work exists generically in `d3d11_host.rs`; AC7 is a second D3D11 host. | Follow-up F1. |
| A2 | `loader/proxy/src/dlss_bridge.c:314-400` | `fill_overlay_stats` writes a DLSS-centred refusal text (`:344-354`), a backend name table that names Spatial "DLSS" (`:340-341`; the panel ignores `backend_name`, so it is dead), and the overlay stats the core fills for other hosts. | Status and refusal wording is the core's; a per-host copy drifts (the Spatial name already did). | Part of F1; until then drop the dead `backend_name` line. |
| A3 | `loader/proxy/src/dlss_bridge.c:292-310` | `display_refresh_mhz` reads the refresh of the foreground window's monitor for the VRR frame cap. | Duplicates `runtime/orchestrator/src/frame_limit.rs:31-85`, which samples the presenting chain's own output; using the foreground window can pick the wrong display. | Part of F1 (export the core value, delete the copy). |
| A4 | `loader/proxy/src/dinput8_proxy.c:660-700` | `backend_directory` resolves each vendor's runtime folder (env override, `RSF_FFX_BIN`, default subfolder) and `start_generation` reads FG mode, generated frames, Reflex mode and frame limit from environment variables and writes the present setup (`max_generated_frames = UINT32_MAX`, `runtime_switching`). | Vendor SDK ownership and FG/latency settings are core. `rsf_game_sys::vendor_paths::VendorPaths` already exists for the first, the settings store for the second. | Follow-up F7 (directories), F1 (settings). `dinput8_proxy.c` is being changed by the fsr4-crash worker. |
| A5 | `games/anomaly/src/sr_temporal.rs` (whole), `fg_stage.rs:1-120` | Anomaly drives the Runtime's D3D11 SR API and binds the `rsf_d3d11_present` table itself: backend switching and its result handling, FG enable, provider, generated frames and latency application. | The sequencing of SR and FG is technique. It is the D3D11 host's job, done a third time. | Follow-up F2. |
| A6 | `games/anomaly/src/fg_policy.rs:91-157` | FG history-reset and skip rules: reset on first frame, after any unfed frame, on extent change, after a scope hold, on a cut; skip during a scope hold. | History resets are technique (Rule 5 names them). The facts (cut, scope, level, device reset) are Anomaly's to report; the rule turning them into resets is generic and every FG host needs it. | Follow-up F2: report scope, cut and level as frame facts; move the reset rules into the core FG path (`d3d11_present.rs`, held for the Fallout 4 and Anomaly commits). |
| A7 | `games/anomaly/src/overlay.rs:290-356` | Anomaly builds the overlay setup: SR choices from `RSF_ANOMALY_SR_ADAPTER_BACKENDS` (`0x3a`, `sr_stage_platform.h:136`), FG choices, latency choices and `max_generated_frames = 3` (`:340`). | An adapter-owned offered set and generated-frame cap. | Overlay-parity worker (masks); F2 for the rest. |
| A8 | `games/anomaly/src/overlay_hooks.rs` (DXGI `Present`/`Present1`/`ResizeBuffers`/`ResizeTarget` detours `:589-651`, DirectInput detours `:507-588`) | Anomaly hooks the generic DXGI swap chain and DirectInput to draw and gate the overlay. | Presentation mechanism and overlay input gating are core; the core's `overlay_host` and `d3d11_host` already present the overlay for other D3D11 games. Only the sites that are Anomaly's own belong in the adapter. | Follow-up F2. |
| A9 | `integrations/godot/native/vulkan.rs:386-510` (before 6b788cb5) | The Godot integration initialises Streamline Vulkan and XeSS, sets latency options, orders shutdown (presenter drain, SR retire, XeSS, latency SDK) and runs the FG presenter lifecycle (`:546-762`). | Vendor SDK lifetime and presentation are core. | **Fixed in 6b788cb5** (the Runtime Vulkan host). |
| A10 | `integrations/godot/native/vulkan.rs:144-163`, `integrations/godot/NATIVE.md:82-91` | `fg_choices` leaves DLSS-G out of FG Auto because "its presenter's retirement after native secondary window use still ends in an access violation". | An adapter hiding a core defect. Any engine with a secondary window would hit it. | **Moved to the core in the F3b commit**: the Vulkan host passes over DLSS-G in Auto only while the engine reports native secondary windows can be in use, with a status reason; the defect itself is open (F3, follow-up). |
| A11 | `integrations/godot/native/vulkan.rs:194-254`, `control.rs:71-87` | Godot refuses settings by vendor rules: FSR3 and FSR4 FG only with one generated frame, FSR4 FG only after a provider probe, no DLSS at Ultra Quality, generated frames 1..=15, preset mask, latency mode range. | Vendor capabilities and limits are core facts; the integration repeats them. | Follow-up F8: one core settings check that returns the refusal and its reason. |
| A12 | `integrations/godot/native/d3d12.rs:397` | D3D12 FG Auto is `rsf_fg_default_backend(vendor, 0)` without the served choices; the Vulkan path in the same integration uses `rsf_fg_select_auto_backend` with them. | Auto resolution is core policy and differs even within one integration. | **Fixed in 346446d9**. |
| A13 | `integrations/unity/src/package_state.rs:62-65, 310-320`, `package_state/presentation.rs:150-160` | The Unity package validates settings against its own `MAX_GENERATED_FRAMES = 15`, `MAX_DLSS_PRESET = 13`, `MAX_LATENCY_MODE = 2`, and resolves FG Auto from the adapter vendor itself. | Same as A11. | F8. `package_state.rs` is being changed by the fsr4-crash worker. |
| A14 | `games/unity-mono/src/runtime_setup.rs:59-65, 229-256, 346-369` | Unity Mono keeps `SR_CHOICES`, reads FG and SR settings from its INI, caps generated frames at 15 and checks the DLSS preset mask (`:355`). | Adapter-owned choices and settings validation. | Overlay-parity worker (mask); F8. File is being changed by the fsr4-crash worker. |
| A15 | `games/minecraft/src/main/java/org/kpc/rescaleframecraft/client/TaaSettings.java:17-38, 75` | Minecraft keeps its own enum of SR backends, qualities, FG backends, DLSS presets, Reflex modes and sharpeners, and clamps generated frames to 1..15. | An adapter-owned option list duplicating the Runtime's. | F8: the mod's settings screen reads choices and limits from the Runtime through `jni.rs`. |
| A16 | `integrations/godot/addons/rescaleframe/settings.gd:5-27`, `integrations/unity/com.kpc.rescaleframe/Runtime/ReScaleFrameOptions.cs:48` | Engine-integration settings resources enumerate backends, qualities, preset range and generated-frame range. | Same as A15. | F8. The C# file is outside the Drag'n Wash worker's `Runtime/Engine/*`. |
| A17 | `games/rdr2/src/runtime_worker.rs:194`, `games/fallout4/src/session.rs:36-37, 148-151`, `games/skyrim-se/src/session.rs:40, 437` | RDR2 passes its own SR mask; Fallout 4 and Skyrim pass quality choices (`0x1f`, or native only when reduction is unavailable). | Adapter-owned choice masks. Fallout 4's case is really a fact (can the engine render reduced?) expressed as a mask. | Overlay-parity worker; report "reduced resolution available" as a fact. |
| A18 | `games/anomaly/src/sr_stage/math.rs:76-92`, `games/skyrim-se/src/resolution.rs:160-167`, `games/skyrim-se/src/sampling.rs:118`, `games/ac7/plugin/src/native_renderer/views.rs:240-244`, `games/ac7/plugin/src/scope_facts.rs:72-78`, `games/minecraft/.../temporal/TemporalCamera.java`, `integrations/unity/com.kpc.rescaleframe/Runtime/ReScaleFrameUpscaler.cs` | Each integration decides the jitter phase count and the texture LOD bias. The rules differ: Anomaly `ceil(8 r^2)` clamped 8..64 and `log2(render/output) + offset` clamped -3..1; Skyrim `floor(8 r^2)` clamped 8..256 and plain `log2`; AC7 an area ratio and `log2 - 0.3` floored at -2. | Phase count and LOD bias are vendor guidance that depends on the active backend (FSR, DLSS and XeSS publish different bias rules). Applying a jitter and a bias in the engine is input work; choosing their values is technique. | Follow-up F5. Not moved here: unifying the rule changes the pattern or bias in at least two working games, which needs a run. |
| A19 | `games/anomaly/src/sr_temporal.rs:44-55`, `games/anomaly/src/fg_stage/options.rs:221` | Backend name tables for logs. | Minor duplication of `overlay_settings.rs:97` and the panel's FG names. | With F2. |

## B. Game knowledge in the core

| # | Location | What it is | Why it violates Rule 5 | Fix |
| --- | --- | --- | --- | --- |
| B1 | `sdk/game/include/rescaleframe/game_frame.h:58-66`, `runtime/orchestrator/src/d3d11_host.rs:63, 1085` (lines before fe05f948) | The screen classes carry AC7's vocabulary (BRIEFING, HANGAR, FLIGHT, REPLAY) beside a later GAMEPLAY. Six integrations report gameplay as FLIGHT, two as GAMEPLAY, and the D3D11 host's capture-sequence readiness accepted only GAMEPLAY. | A core rule keyed to the value one pair of games reports. | **Fixed in fe05f948**: the host reads both as the gameplay fact through the SDK constants; the header says they are one fact and new integrations report GAMEPLAY. |
| B2 | `runtime/contract/include/rescaleframe/frame_generation.h:24-30`, `runtime/orchestrator/src/fg_choice.rs:52-63` | `rsf_fg_backend_info.ini_key`, documented as "the Unity ini path setting". No caller reads it (the only user, `dinput8_proxy.c:662-666`, reads `env_var` and `default_subdir`). | One engine's configuration key in the core contract. | Follow-up F11: drop the field when the overlay-parity worker's change to `frame_generation.h` lands. |
| B3 | `loader/proxy/src/dinput8_proxy.c` (`AC7.ini` `:300`, Unreal velocity format `:421`, jitter revival `:471-493`, `accept_ac7_window` `:645`), `dlss_bridge.c`, `dlss_bridge.h:21-26, 50` | The AC7 carrier lives in the shared loader tree. Its input work (jitter revival, Unreal motion encoding) is correctly integration work, but in the wrong directory. | `loader/` is core; AC7 knowledge there reads as shared. | Follow-up F12: move to `games/ac7/carrier/` with its CMake target; the technique part goes to the core under F1. |
| B4 | `runtime/orchestrator/src/native_translucency.rs:735-748` with `runtime/graphics/rust/src/frame_tap.rs:1085-1111`, and `runtime/orchestrator/src/sampler_bias.rs:33-47` with `d3d11_host.rs:2697-2700` | Two core implementations of texture LOD bias on D3D11: the frame tap's sampler rewrite (AC7 only) and `sampler_bias.rs` (Skyrim, Fallout 4). | Not game knowledge, but one technique twice, each reachable by one host. Every D3D11 game should get the same one. | Follow-up F6. |
| B5 | `runtime/orchestrator/include/rescaleframe/dlss_pipeline.h:148-155` | The scene-colour field of the core pipeline contract described AC7's role table and a "slot 0 of the bound set" caller rule no code uses any more. | One game's selection rule in a core contract. | **Fixed in 91325d70** (comment only). |
| B6 | Single-user core hosts: `render_host.rs` (RDR2), `callback_host.rs` (Unity Mono), `gl_pipeline.rs` (Minecraft); `dlss_pipeline`, `native_sr`, `native_translucency`, `native_composition`, `native_window` and the frame tap (AC7 carrier only; Anomaly also uses `dlss_pipeline` through `rsf_get_d3d11_sr_api`) | No per-game branches inside, but each was grown for one game and offers that game's subset. | "Host variants that only one game uses" are where parity drifts (category C). | F1-F4 converge on one host per graphics API. |

Reviewed and kept: `runtime/graphics/include/rescaleframe/colour_transport.h:11-16` (the knee was
measured on AC7, the technique is general and on for every game; another engine with dark linear
HDR would band the same way), `runtime/orchestrator/src/streamline_policy.rs:3-8` (Streamline's
own engine enumerant, which Rule 1 allows as adapter metadata), and the Unreal examples in
`motion_decode.h`, `frame_tap.h`, `d3d11_observer.h` and `frame_assembly.h`, which document
parameters the integration supplies rather than assumptions the core makes.

## C. Parity gaps

The overlay-parity worker owns the offered set; this table records the structural reason for
each gap rather than repeating its table.

| # | Gap | Technical reason | Fix |
| --- | --- | --- | --- |
| C1 | SR providers differ: Skyrim and Fallout 4 lack Spatial and Auto (`d3d11_host.rs:53`, `0x3e`); AC7 lacks Auto (`dlss_bridge.c:335`); Anomaly and RDR2 lack FSR2, Spatial and Auto (`0x3a`); Unity Mono lacks Auto in its mask; Minecraft and Godot offer Auto. | Each host keeps a constant mask instead of asking the Runtime what it can serve on this device. | Overlay-parity worker (`sr_choices.rs`). |
| C2 | Generated frames: Anomaly caps at 3 (`overlay.rs:340`), every other host at 15 or the provider maximum. | Adapter constant. | Overlay-parity worker; F2. |
| C3 | SR quality: AC7 and RDR2 offer the standard mask only (`dlss_bridge.c:493`, `render_host.rs:1171`); others the extended one. | Host constants. | Overlay-parity worker. |
| C4 | DLSS-G is never chosen by FG Auto on Godot Vulkan (A10). | A core presenter defect hidden in the integration. | Narrowed: Auto offers DLSS-G when the engine embeds its secondary windows (Godot's default) and reports none native; open while the defect is (F3). |
| C5 | Auxiliary-target upscaling (`rsf_d3d11_host_upscale_auxiliaries`), reduced-frame recovery and the `sampler_bias.rs` bias exist only behind the D3D11 host (Skyrim, Fallout 4). AC7 and Anomaly do not get them. | They are reachable only from that host's entry points. | F1, F2, F6. |
| C6 | Steam overlay forwarding (`integrations/steam`, "shared by the Skyrim SE and Fallout 4 adapters") is wired only into those two. AC7 is a Steam game with FG on a D3D11 facade. | Only those two adapters install it (`games/fallout4/src/creation.rs`, `games/skyrim-se/src/creation.rs`), though what a forwarded call means is already the Runtime's (`rsf_d3d11_host_dispatch_chain`). | Install it from the shared D3D11 host start once AC7 is on that host (F1). |
| C7 | The VRR frame cap reads the display differently in AC7 (A3) than in the core hosts. | Duplicated implementation. | F1. |
| C8 | Skyrim and Fallout 4 status text still says FG and latency ownership are incomplete (`games/skyrim-se/src/session.rs:50-62`). | Their CPU event reporting is not finished; the core features exist. | Their integration work (Skyrim creation gate worker), not a core change. |

## D. Inputs and output

Assignment search over non-test sources for the optional `rsf_game_render_pass` inputs. A zero
means no assignment was found, not that the engine lacks the data.

| Integration | `exposure` | `temporal_mask` (reactive/transparency) | `ui_input` | `depth_domains` |
| --- | --- | --- | --- | --- |
| AC7 | yes | through `native_translucency` hints | yes | no |
| Anomaly | yes | no | no | no |
| Skyrim SE | no | yes | no | no |
| Fallout 4 | no | no | no | yes |
| RDR2 | yes | no | no | no |
| Unity Mono | not in Rust; managed code not searched in depth (Drag'n Wash worker) | | | |
| Unity package | no | no | no | no |
| Godot | yes | yes | yes | yes |
| Minecraft (GL record) | no | no | no | no |

| # | Location | What is missing or wrong | Effect | Fix |
| --- | --- | --- | --- | --- |
| D1 | Skyrim SE, Fallout 4, Unity package, Minecraft | No exposure. Creation Engine computes eye adaptation; URP and Minecraft shader packs have exposure or none. | Backends run auto-exposure, which AC7 showed bands in dark HDR. | Lease the engine's adaptation value (Creation: the luminance target after adaptation) and tag it. |
| D2 | Anomaly, Fallout 4, RDR2, Unity, Minecraft | No reactive or transparency mask. | Ghosting on particles and transparent surfaces the backend cannot see. | Produce a mask from each engine's translucent pass (Skyrim and Godot show the field is usable). |
| D3 | All but AC7 and Godot | No `ui_input`, so FG falls back to whatever HUD-less handling the host has. | UI smearing in generated frames where the HUD-less capture is late or absent. | Supply the UI layer or a HUD-less frame at the UI boundary. |
| D4 | `games/rdr2/src/renderer.rs:427-431` | Every frame with a source is reported as FLIGHT (gameplay), menus and pause included. | FG may interpolate menus. | Report the screen from RAGE's pause and menu state. |
| D5 | `games/unity-mono`, `integrations/unity/src/package_state.rs:566-570`, `integrations/godot/native/d3d12.rs:326-331` | Screen class comes from a developer or INI "gameplay" switch, not the engine's state. | Correct only if the developer keeps it current. | Acceptable for an engine integration's API; document that the switch is the fact. |

Event timing (Rule 3, "reports an engine event at a moment other than when it happens") is not
established by static review; the fg-ac7 and fg-unity workers are changing that path now.

## Documentation and skills

- `integrations/godot/NATIVE.md:82-91` documents A10 as policy ("DLSS-G is not offered to FG
  Auto"). It is accurate about the code and should change with F3.
- AGENTS.md's Architecture table lists `rsf_proxy_ac7` as "dedicated AC7 carrier" but not that it
  hosts SR, FG and overlay application (A1); it lists none of the four core hosts. Update with F1.
- The skills (`game-render-analysis`, `ue418-render-integration`, `unreal4-render-integration`,
  `truesky-render-integration`) put no technique in an integration. The ue418 skill's "registered
  pre-tonemap SR/fallback graph node" is output placement plus the native fallback, which Rule 5
  leaves to the integration.
- Game READMEs and `engine.json` files describe refusals and inputs, not technique ownership; no
  contradiction found.

## Fixes committed

| Commit | What moved | Checks |
| --- | --- | --- |
| fe05f948 | B1: the D3D11 host reads FLIGHT and GAMEPLAY as one reported fact; `game_frame.h` states the equivalence. | `cargo test -p rsf-orchestrator --lib d3d11_host`, 16 passed. |
| 91325d70 | B5: one game's caller rule removed from the core pipeline contract. | Comment only; no runtime code selects scene colour by slot. |
| 571d871e | F12, F13: the AC7 carrier moved to `games/ac7/carrier` with its CMake target; the overlay host header beside its Runtime owner; the dead `backend_name` dropped. | `cargo check -p rsf-overlay-fixtures --tests`; C not compiled here. |
| a4066454 | F1, A1-A4, C7: AC7 on `rsf_d3d11_host`. The host gained `RSF_D3D11_HOST_CAP_NATIVE_GRAPH` (SR pass and translucency, cloud depth, materials, UI composition, texture binding and window-end facts through `render_pass`; the plan follows the bound chain's extent at Present) and `rsf_d3d11_host_set_adapter` (adapter overlay options after the host's, an input-preparation Present callback). The carrier keeps motion capture, the velocity pass, the frame tap that feeds them, the jitter gate, its startup options and the route dump; its SR start, quality/backend/preset/sharpening application, overlay host, stats, VRR display read, FG settings and vendor folders are gone, and with them the native SR plan state, `rsf_dlss_pipeline_backend_choices`, `rsf_overlay_request_fg_backend` and the carrier-only sharpening/settings wrappers. | `cargo test -p rsf-orchestrator --lib` (411, four new host tests), `-p rsf-ac7 --lib` (39), `cargo check -p rsf-orchestrator --features test-fixtures --tests`, `-p rsf-ac7 --features carrier --tests`. C not compiled here; no game run. |
| ef4538c4 | F6, B4: one D3D11 texture LOD bias policy. `rsf_graphics::frame_tap::biased_sampler` (linear-mip, non-comparison samplers with a mip range; summed bias clamped to the D3D11 range) creates every biased clone: the frame tap's live rebind (AC7) and the host's explicit slots (`sampler_bias.rs`, Skyrim SE, Fallout 4). The two entry points stay because the engines bind differently (a state-cached engine needs the live rebind; the others name their slots). | `cargo check -p rsf-orchestrator --features test-fixtures --tests`, `cargo test -p rsf-graphics --lib`; the WARP fixture `tests/sampler_bias.rs` updated, not run here. |
| 96fcafe0 | F2 (parity item): Anomaly installs the presentation bridge whenever its FG stage runs; `FGBridge` and its panel option are gone. | `cargo test -p rsf-anomaly --lib` (171). |
| b256fa81 | C1 for D3D11 (overlay-parity follow-up 1): the D3D11 SR pipeline serves FSR1 with the shared `sr12/easu.hlsl` kernel; the host and the D3D11 SR admission offer it, so AC7, Skyrim SE, Fallout 4 and Anomaly do. | `cargo test -p rsf-orchestrator --lib` (414), `-p rsf-anomaly --lib` (171). |
| 30cc9a23 | The D3D11 host no longer ties SR and the overlay to the presentation bridge: with the bridge refused or its chain creation fallen back, it presents natively (SR, sharpening, overlay), reports FG and latency unavailable with the reason, and reaches Present through an observer listener when the game installed the observer itself. Every D3D11 host game. | `cargo test -p rsf-orchestrator --lib` (415), new host test. |
| f46bb6f0 | FSR1's ratio table in one core module (`fsr1.rs`); the D3D12 SR path should switch to it. | `cargo test -p rsf-orchestrator --lib` (415). |
| 1bed8f5e | Unity Mono engine-FSR1 switch: the bridge delivers a pending SR policy change at the window event once its command ring is idle, so the switch no longer waits for a managed probe packet. | `cargo test -p rsf-orchestrator --lib callback_host` (18), `-p rsf-unity --lib bridge_state` (18). Not game-run. |
| b67de62b | F4, first part: `callback_host.rs` folded into `render_host` (`render_host/callback.rs`); vendor packages, engine identity, SR context setup and the overlay SR card are one implementation for RDR2 and Unity Mono. Two C entries stay: RDR2 hands its recordings to the host's fence, Unity's bridge retires its own ring. | `cargo test -p rsf-orchestrator --lib render_host` (33), clippy. |
| 81c2319c | Runtime FSR1 (overlay-parity follow-up 1) on D3D12: sr12 serves Spatial with an FSR1 EASU kernel where the engine has no scaler; RDR2 offers it. | `render_host sr12 sr_choices` (54), kernel compiled with d3dcompiler_47. Not device-tested. |
| ffb75f75 | Rule 2 for Unity Mono: SR backend, quality, enable state, generation mode and frames saved as `rsf_render_settings` in the preferences `[Rendering]` section; unsaved keys keep the adapter's `[UnitySR]` start values. | `render_host` (36), `-p rsf-unity-mono`. |
| 04ba51ba | F4: the D3D12 callback host gains per-view SR contexts (a view's context is released once idle and its last recording slot has been recorded again), an attach mode for an adapter that owns its lifecycle (V3 setup), `rsf_render_settings_get/apply` for it, and `rsf_callback_host_status_get`. | `render_host` (38). `rust_exports.c` changed, not compiled. |
| 71ab561f | F4 completed: the Unity package host moved onto that host. The package keeps its bridges, Editor preview, screen class, input and lifecycle; `package_views.rs`, its settings validation, view creation, provider requests and the bridges' ring epochs are deleted. `rsfu_configure/settings/status` translate onto the host's settings and status; package games get the shared overlay on the D3D12 route. | `-p rsf-unity --lib` (74), `render_host` (38), `-p rsf-unity-mono`, clippy, `cargo check -p rsf-unity --features package`. Not built with CMake or run in Unity. |
| a8cbbf00 | sr12 plans FSR1 from the shared `fsr1.rs` table (9b598e77, from the D3D11 branch). | `sr12 fsr1` (19). |
| 6b788cb5 | F3a, A9: the Godot Vulkan session is the Runtime's Vulkan host (`vulkan_host.rs`, `rescaleframe/native_vulkan_host.h`): SDK lifetime and shutdown order, the creation resolver (XeSS, interop and latency device stages), provider admission and Auto, the FG presenter, the SR session, capture and status. Godot's `api.h` names the host's records (layouts and versions unchanged), `exports.def` publishes the host's presenter and pass entries as `rsfg_vk_*`; Godot keeps its boot wrapper, window admission and WSI plumbing. The CMake features table in `runtime_platform.c` is gone (cargo features carry the facts); the SR session's linked route table is in-crate. Unity's Vulkan path calls no Vulkan provider (it bridges into the D3D12 host), so it is not a parallel Vulkan host. | `cargo test -p rsf-orchestrator --lib vulkan_` (61, the creation fixture among them), `-p rsf-godot` (3), clippy both. C and the engine module's `native.h` not compiled here; no Godot run. |
| 346446d9 | A12: Godot D3D12 FG Auto is `rsf_fg_select_auto_backend` over the bridge's served providers, as on the Vulkan host. | `cargo clippy -p rsf-godot`. No Godot run. |

## Follow-ups, ranked by user impact

**F1. AC7 onto the shared D3D11 host.** AC7 is the most-played integration and the one whose host
is furthest from the core: `dlss_bridge.c` applies overlay intents, switches providers with its
own restore, keeps its own stats and VRR display read, and `dinput8_proxy.c` resolves vendor
folders and FG settings. Design: AC7 starts `rsf_d3d11_host_create` like Skyrim and Fallout 4,
reports its native passes (already `rsf_game_render_pass` with roles) through the host services,
and keeps only input work: the native renderer plugin, motion capture, the velocity pass, jitter
revival, the window class and the Unreal motion encoding as setup data. The host gains what AC7
has and it lacks: the native translucency hints, UI composition capture and the frame tap's
sampler bias (F6), behind the existing pass roles. Size: several thousand lines across C and Rust
and a CMake target; not attempted here. Gains: Auto SR, auxiliaries, Steam overlay forwarding,
one VRR read.

**F2. Anomaly onto the shared D3D11 host.** Done so far: the bridge is installed whenever the FG
stage runs (96fcafe0) and FSR1 is offered through the D3D11 SR admission (b256fa81). The rest waits
for the user's next Anomaly and AC7 runs, so the AC7 move (a4066454, 30cc9a23) is confirmed first.
Plan, one commit per step, each leaving Anomaly working and tested:

1. *Host gap.* `rsf_d3d11_host_adapter` gains `option_label` (append, size-checked like the other
   records) and the host forwards it to the overlay service's `option_label` with the adapter index,
   because Anomaly's options show live status text (`overlay.rs` `option_label`). Host test: labels
   reach the panel at the adapter's offset.
2. *Session and overlay.* `games/anomaly` creates the host where `overlay.rs` configures the overlay
   service today: configuration path the Anomaly INI, section `Anomaly` (the overlay service already
   uses it, so saved SR/FG keys carry over), runtime directory beside the plugin, X-Ray version and
   project id from `sr_stage_platform.h`, units 1.0, Anomaly's motion decode
   (`RSF_ANOMALY_SR_MOTION_*`), quality 0x1f, presentation bridge 1 (factory interception, as
   `fg_stage` installs it), its window filter as `accept_window`, capabilities FG_INPUTS and
   INPUT_TIMELINE. Its adapter options (`overlay.rs` registry, saved options, capture settings) go
   through `rsf_d3d11_host_set_adapter`; `suspend`/`resume` become `rsf_d3d11_host_set_active`.
   Delete the overlay-service setup in `overlay.rs` and the DXGI Present, Present1, ResizeBuffers
   and ResizeTarget detours in `overlay_hooks.rs` (the bridge facade, or the host's native Present
   listener, delivers Present). The DirectInput detours stay: they gate the game's input while the
   panel is open, an input fact the core has no service for yet; they read the overlay capture
   state as today. Checks: Anomaly overlay tests rewritten against the host's adapter options; the
   overlay fixture (`overlay/fixture.rs`) for the coordinator's device run.
3. *SR.* `sr_temporal.rs` stops driving `rsf_get_d3d11_sr_api`. The world-window boundary in
   `sr_swap.rs`/`sr_stage.rs` reports a RECONFIGURE planning pass through `process_upscale` with the
   output extent (as `games/skyrim-se/src/resolution.rs` does) and reads the plan from
   `render_config`; the SR pass at the swap calls `process_upscale` with colour, depth, motion,
   exposure, camera and jitter. Delete backend switching, result handling, quality, preset and
   sharpening application and the log name tables in `sr_temporal.rs` (A5, A19); `d3d11_sr_admission.rs`
   and, if nothing else uses it, `d3d11_sr_api.rs` go with them. Keep the twin targets, jitter
   application, motion, geometry and foliage work. Checks: `sr_temporal` and `sr_stage` tests
   rewritten to assert the reported passes; the SR jitter fixture
   (`tests/anomaly_sr_jitter_fixture.rs`) for the device run, with its 0.05 px acceptance.
4. *FG.* `fg_stage.rs` stops binding `rsf_get_d3d11_present_api`/`rsf_get_d3d11_fg_api`. It reports
   FRAME and SUBMISSION scopes, the HUD-less boundary as FINAL_SCENE and WINDOW, and the SR-off
   depth/motion of `fg_inputs.rs` as an FG_INPUTS pass (the host's `fg_capture`), all through the
   host's `render_pass`; CPU events through its `cpu_event`. Delete the bridge installation,
   options, provider requests, generated-frame and latency application and the FG name table
   (A5, A19). `rsf_native_fg_dispatch_pass`/`_cpu` and `rsf_get_d3d11_fg_api` then have no user:
   delete them in `fg_dispatch.rs`. Checks: `fg_stage` tests against the reported passes; the FG
   device fixture (`fg_stage/fixture_device.rs`) for the coordinator.
5. *Reset rules into the core (A6).* In `native_fg.rs` `rsf_native_fg_prepare`, a source is not
   continuous when the previous presented source of the session was not fed, when the fed extent
   changed, or on the session's first fed frame; history then restarts as for a reported RESET. These
   rules apply to every FG game. Anomaly keeps only facts: a cut and a device reset as
   `RSF_GAME_RENDER_RESET`, scope frames and the `FGScopeHold` frames after them as
   `RSF_GAME_RENDER_NO_FG` (the hold is how long X-Ray's scope render disturbs the main view, an
   engine fact). `fg_policy.rs` shrinks to that reporting and its counters. Check: native_fg
   prepare tests for each rule (first, unfed predecessor, extent change, reported reset, NO_FG).

Behaviour to confirm in a run after steps 2 to 5: Anomaly upscaling with every backend and FSR1,
the jitter acceptance, every FG provider switched on from the overlay, overlay controls and Anomaly
options with live labels, latency status, input gating while the panel is open; and, for step 5,
AC7, Skyrim SE and Fallout 4 FG after menus, cuts and resolution changes (one frame without
generation after a discontinuity is expected).
**F3. A core Vulkan host for Godot.** Move `integrations/godot/native/vulkan.rs` session logic
(SDK init and shutdown order, provider resolution, admission, presenter lifecycle) into
`runtime/orchestrator` beside `vulkan_sr.rs` and `vulkan_fg.rs`, entered through a versioned
setup the engine module fills with its device, queue, window and graph-lease capability. Fix the
DLSS-G presenter retirement after secondary-window use there, then drop the Auto exclusion and
the NATIVE.md paragraph. Do the same for `d3d12.rs` against `render_host` or `callback_host`,
which also removes A12.
*Status:* the Vulkan session is the Runtime's Vulkan host (6b788cb5): `runtime/orchestrator/src/vulkan_host.rs`, entered through `rescaleframe/native_vulkan_host.h`, whose setup, settings, status and pass records are the ones Godot's `api.h` already used, so any engine that owns a Vulkan device and WSI reports the same packet and presenter calls. A12 is fixed (346446d9). Open:

- *D3D12.* `d3d12.rs` still duplicates the core D3D12 host: its own completion fence and three command slots, SR context lifetime with the fallback to the normalising context, the explicit-presentation FG install (`rsf_fg12_install_explicit`), HUD-less and window passes, the capture owner, and SR Auto by `rsf_gpu_default_sr_backend` without the served choices. Design: Godot attaches the `render_host` callback host like the Unity package (V3) and reports SR, FINAL_SCENE and WINDOW passes through its `rsf_game_host_services`; its submission report (`rsfg_submitted`) becomes the retirement fact. The host needs what Godot has and it lacks: an engine that evaluates FSR1 itself (the Unity `set_engine_spatial` path covers it), host-fenced retirement for an adapter that reports submitted command lists (RDR2's entry has the fence), and the FG install that F4 also leaves in the Unity integrations. `render_host` belongs to the D3D12 worker, so this is not done here.
- *DLSS-G in Auto (F3b).* The fault is not presenter retirement: retirement completes with strict validation clean and the process then faults in `vkDestroyDevice` after a native secondary window coexisted with a DLSS-G presenter, root cause unknown (`godot-vulkan-dlss-fg.md`). The exclusion is the core host's (`auto_without_dlssg`), keyed on facts the engine reports (`RSF_VKHOST_EMBEDDED_WINDOWS` at boot, `rsf_vkhost_secondary_window` when a native secondary swapchain appears); DLSS-G stays explicit, and the status carries `RSF_VKHOST_REASON_AUTO_WITHOUT_DLSSG`, which the overlay's FG-unavailable reason field (shared-services worker, `ui/overlay`) should show. Follow-up: the native-secondary Streamline scope (`prepare.py` skip of Reflex WSI hooks for secondary calls, the bridge export, primary-only latency create info in `eng/patch-godot.py`), an SDK rebuild with the new receipt digest, and the secondary-window shutdown control under strict validation; steps in `godot-vulkan-dlss-fg.md`.

**F4. One D3D12 host.** `render_host.rs` (RDR2) and `callback_host.rs` (Unity Mono) both own
admission, SR context lifetime, recording slots, settings and overlay routing for D3D12. Merge
them so RDR2 and Unity games share one offered set and one settings path; the Unity package host
(`integrations/unity/src`) then moves onto it as well. *Status:* RDR2 and Unity Mono share one
module, offered set and settings store (b67de62b, ffb75f75), and the Unity package attaches the
same host (04ba51ba, 71ab561f): one D3D12 host serves every D3D12 integration. Two C entries
remain because the retirement facts differ (host-fenced recordings for RDR2, an adapter-retired
command ring for Unity Mono and the package). Open: the generation install still happens in both
Unity integrations, before graphics creation.

**F5. Jitter phase count and LOD bias from the core.** Add to `rsf_game_render_config` (append,
bump the ABI) the jitter phase count and the texture LOD bias for the active backend and current
ratio. The core fills them from the vendor's published rule; integrations apply them to the
projection and the engine's samplers. Replaces A18's seven copies. Needs a run per affected game
because the values change.

**F6. One D3D11 sampler-bias mechanism.** Keep `sampler_bias.rs` or the frame tap's live rebind,
not both, and drive it from F5's value for every D3D11 host.

**F7. Vendor runtime folders in the core.** Every host resolves vendor folders through
`rsf_game_sys::vendor_paths::VendorPaths` or one Runtime export; delete `dinput8_proxy.c:660-667`
and Godot's `runtime_path` suffixes (`vulkan.rs:405-418`).

**F8. One settings check.** Export a Runtime function that validates an `rsf_render_settings`
request (provider known, quality served by that provider, preset valid, generated frames within
the provider maximum, latency mode served) and returns a reason. Godot `control.rs`, the Unity
package, Unity Mono and the Minecraft JNI layer call it and drop their constants; the Godot
settings resource, the Unity options and Minecraft's settings screen read their choices from it.

**F9. RDR2 screen class (D4)**, **F10. exposure and reactive masks (D1, D2)**: integration work
per game, each needing research into the engine's producer.

**F11. Remove `rsf_fg_backend_info.ini_key` (B2)** after the overlay-parity change to
`frame_generation.h` lands.

**F12. Move the AC7 carrier out of `loader/proxy` (B3)** into `games/ac7`, with or after F1.

**F13. Drop `dlss_bridge.c:340-341`** (dead `backend_name`), with the overlay-parity worker's
change to the same function.
