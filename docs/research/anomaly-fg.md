# Anomaly: frame generation through the shared D3D11 presentation bridge

Status, 12 November 2026: built and fixture-tested on an RTX 4070 Laptop GPU. **Not game-tested.** Rendering-readiness
flags stay 0. Code: `games/anomaly/src/fg_{stage,policy,inputs,overlay}.*`, hook-ups in `bootstrap.cpp` and
`native_observer.cpp`, overlay choices in `overlay_service.cpp`. Runtime: `rsf_get_d3d11_fg_api`
(`runtime/orchestrator/include/rescaleframe/d3d11_fg_api.h`) and three generic additions to `native_fg.h`.

## Question and approach

How does Anomaly (X-Ray Monolith 1.5.3, D3D11, delivered as a DLL shim) feed the Runtime's existing D3D11 frame
generation, the path AC7 uses, without moving engine knowledge into the Runtime?

AC7 links `native_fg.cpp` statically and drives it with game-ABI CPU events and render passes. A DLL adapter could not
reach that path: `rsf_get_d3d11_present_api` exposes presentation only. Rather than duplicating the transfer slots,
retirement and eligibility in the adapter, the new table exposes the same owner to DLL adapters, and the adapter
speaks the ABI's vocabulary (CPU events; FRAME, SUBMISSION, FINAL_SCENE, WINDOW passes) from engine sites the native
observer already hooks. No new detour was needed.

## Runtime additions (generic, Rule 1)

- `rsf_get_d3d11_fg_api` (version 1, size-checked, C and C++ contract tests through the real DLL): log, options,
  status, Reflex ordering, `cpu` (record + latency/pacing), `pass` (the shared dispatch by role), `bind_source`,
  `capture`, `inputs_ready`, `dump_inputs`, and the bridge callbacks `prepare`, `retire`, `present_event`,
  `latency_event`. Every entry is the existing native_fg/native_cpu/native_scene/native_window function or a
  three-line dispatcher.
- `rsf_native_fg_bind_source`: an adapter whose SR pass carries no frame identity (Anomaly's `sr_pass.h` builds
  passes with `source_frame_id = 0`) declares the source frame on the graphics thread. `rsf_native_fg_scene` then gives
  only the FG scene copy that identity and screen class, and ORs RESET/PRIMARY/AFTER_SIMULATION; the SR evaluation and
  its history keys are untouched. Passes that carry their own identity (AC7, RDR2, Unity) are never rewritten.
  Without it, DLSS SR on the bridge's shared Streamline host refuses every frame (`rsf_native_fg_evaluate` needs a
  live token for the scene's source id).
- `rsf_native_fg_inputs_ready`, `rsf_native_fg_dump_inputs`: observation, and the FG input capture through the frame
  dump service (`<prefix>_fg_depth`, `_fg_motion`, `_fg_hudless`, exactly what generation receives).

`frame.ui` (a separate UI layer) is still not transported by native_fg. Anomaly does not need it: the bridge runs in
`RSF_UI_MODE_BACKBUFFER_HUDLESS` and every provider derives the interface from the final frame and the HUD-less copy.

## Engine sites (verified by `tests/anomaly_fg_evidence.py --verify` against the installed exe)

| Site | RVA | FG use |
| --- | --- | --- |
| `IRenderDeviceRender::Begin` | `0x822cb0` | after the original: simulation end, FRAME and SUBMISSION begin, SR source binding |
| `IRenderDeviceRender::End` | `0x822eb0` | before: submission end, WINDOW scope; after: frame end |
| Present inside End | `0x822f16` | `jmp [rax+40h]` tail jump on `HW.m_pSwapChain` (`0xceb8e0`), skipped on SVP frames |
| `SetCacheXform` | `0x823250` | the frame's main camera (SR-off camera record, cut test) |
| `CHUDManager::RenderUI` | `0x0f1e60` | the HUD-less boundary, entered as a tail jump (`0xedad6`, vtable +0x60) |
| `CRender::RenderToTarget` | `0x898640` | target 2 (`rtSVP`, call at `0xedac1`, vtable +0x1e0) marks a scope frame |
| `CRender::reset_begin` | `0x83a020` | device reset |
| `HW.pBaseZB` | `0xceb8f0` | SR-off depth (read by `CRender::Render`'s `u_setrt`) |

All 16-byte guards are the existing `native_profile.h` entries; `sr_profile.h` holds the HW layout.

## The HUD-less boundary

`CLevel::OnRender` (`Level.cpp:714-779`, matched in the binary above): 3D PDA draw and `RenderToTarget(rtPDA)` into the
back buffer, then the world (`CRender::Render`, ending in `phase_pp` to the back buffer, which overwrites the PDA
draw), `Game().OnRender()`, `BulletManager().Render()` (tracers), `RenderToTarget(rtSVP)` on scope frames, then
`HUD().RenderUI()`. The copy is taken at RenderUI's entry. In it: world, post-processing, first-person weapon,
item-attached UI drawn inside `Render` (`RenderActiveItemUI`, world-space), tracers. Not in it: HitMarker,
`pUIGame->Render` (HUD, 2D PDA, inventory, dialogs), fonts, the HUD target/crosshair, the pause text, everything later
in `seqRender` (console, statistics) and the RSF overlay (drawn from the bridge's `before_present`, after the tag).
Unverified: `OnRenderPPUI_main/PP` inside `phase_combine` (menu postprocess UI); the main menu removes the level from
`seqRender` (`MainMenu.cpp:224`), so those frames never reach the boundary.

## Frame timeline, pacing and latency markers

13 November correction: the adapter reports engine facts under [the shared latency contract](input-keyed-latency.md).
It no longer invents an input sample or asks for pacing when the previous Present returns.

| When (engine) | Facts / pass | Core orchestrator behavior |
| --- | --- | --- |
| CInput::OnFrame entry, before KeyUpdate/MouseUpdate | FRAME_BEGIN, INPUT_SAMPLE, SIMULATION_BEGIN | Reserve a token, join previous Present as required, run the one pre-input pacing operation, then SimulationStart |
| Successful buffered DirectInput return during that callback | INPUT_EVENT kind bits | Deduplicate each received kind for this source, route Reflex/PCL or XeLL input events, count them |
| Window subclass receives a message | INPUT_EVENT with WINDOW_MESSAGE and its ID, source 0 outside a frame | Recognize the Runtime's registered PCL ping; match an unframed candidate within 250 ms or expire it |
| IRenderDeviceRender::Begin returned | SIMULATION_END, FRAME/SUBMISSION begin, source identity | SimulationEnd then RenderSubmitStart |
| RenderUI entry | Capture, FINAL_SCENE, HUD-less image | Prepare resources independently of input/latency policy |
| End before original / real Present | SUBMISSION end / WINDOW / actual Present callbacks | RenderSubmitEnd then PresentStart/End with the same source identity |
| End returned | FRAME_END | Close source ownership; do not reserve or pace the next frame here |
| Another native input update without a render | Previous SIMULATION_END and FRAME_END with RENDER_SKIPPED | Retire that unpresented token, then reserve the new input frame |
| Render without an input callback | Only observed frame/render facts | Label a core input substitute, skip a late sleep and refuse FG for that source |

The input callback is the primary CInput pureFrame slot 0 on both exact builds, found through RTTI and verified by its
keyboard-then-mouse calls. A guarded slot swap avoids MinHook's VEX decoding limit. The complete 0xc3-byte OnFrame body,
the three unique entry guards and the expected slot target are checked. Standard/AVX RVAs and bytes are in the
[profile note](anomaly-avx-profile.md#input-facts-and-latency-boundary). The existing DirectInput hooks observe the
successful count returned by GetDeviceData, including reacquire/retry, and report only while this thread executes the
verified callback. Empty, failed and peek-only reads do not produce received-input facts. The hooks also work with the
overlay disabled; input installation is idempotent for the same overlay gate. The overlay's filtering remains intact.
The stock source's controller implementation is commented out; the shipped OnFrame has no controller poll. Controller
counts therefore remain zero in this exact build, while the common protocol and fixtures cover that kind.

The former frame-open sequence was a synthetic timing approximation. Source revision
028ccf5f5189b393396480076b11e40852ba3ecd registers CInput on seqFrame at REG_PRIORITY_HIGH and reads keyboard then mouse
inside OnFrame (Xr_input.cpp:647-654); game input-receiver callbacks occur inside those updates. Native Begin is the
existing simulation-end boundary, followed by render submission and the real Present. The core default synchronous
join completes the previous Present before pacing the next sampled source; the adapter no longer selects Reflex ordering.
Scope, menus and pause retain their real input facts independently of FG eligibility. Loading frames without a poll are
explicit substitutes, and inactive updates that do not render cannot leave a token blocking the next frame.

`Latency window` logs per-window keyboard/mouse/controller counts, registered ping count, substitutions, duplicates,
marker-order violations and Anti-Lag 2 driver updates/refusals. The existing Reflex driver report must show nonzero
keyboard and mouse counts after real input, with PCL ping nonzero when a ping tool targets the game window. Expected
marker-order violations are zero. These are SDK/fact counters, not measured end-to-end latency.

The shared Runtime now has owner 3, AMD Anti-Lag 2, alongside Reflex and XeLL. Its pinned MIT SDK is initialized only
for an AMD physical device; non-AMD and failed SDK initialization are unavailable. DX11 uses its loaded-driver interface;
DX12 FSR uses MarkEndOfFrameRendering before the provider's Present and the SDK-defined swapchain private data so
FSR 3.1.1+ marks real/generated frames. A required Reflex/XeLL provider keeps its single pacing owner; Anti-Lag is disabled
rather than stacking a second sleeper. The overlay reports driver updates and offers Off/On through the existing saved
latency setting; Boost maps to On for AMD. The stub contract covers gating, settings, one update per identity, failure,
DX11/DX12 selection and FSR metadata. No AMD hardware test was possible on this NVIDIA/Intel machine. SDK reference:
[AntiLag2-SDK integration guide](https://github.com/GPUOpen-LibrariesAndSDKs/AntiLag2-SDK#integration).

Pacing selection is independent of generation provider: an available NVIDIA Reflex host also paces FSR, while a
required XeLL owner takes precedence for XeSS FG. XeLL has the same saved Off/On control; Boost maps to On and active
XeSS FG still requires XeLL enabled. The SDK fixtures verify queued ping acceptance before pacing and combined
controller/ping deduplication for Reflex and PCL profiles; their synthetic inputs are not an external latency-tool run.

This input-keyed correction passed static/fixture validation and has not been run in Anomaly. No game or installed
file was touched. First game checks: hook install without refusal; real keyboard/mouse counts; ping count with the tool;
zero marker-order violations; one sleep per sampled source; Latency Off/On/Boost, frame limit, pause/menu/scope/loading,
resize and provider switching. AMD acceptance additionally needs its supported driver and latency monitor/FLM. Existing
rendering-readiness flags stay zero.

Validation: full Release build in build/wk-avx passed; the requested anomaly|thin|motion|shader_patch|latency|fg
selection passed 115 tests with five skips in 207.09 seconds. Four skips need SDK arguments; the DLSS SR-off device
fixture could not obtain foreground ownership. Both executable maps pass 62 evidence checks (8 originals and 54
negative mutations) and production read-only mapped-image slot probes. Explicit D3D11 bridge plain/switch/borderless
runs passed on vendor 0x10de. Switching recorded 99 Reflex sleeps across 120 mixed-provider frames, 840 markers,
zero order violations, active FSR3/XeSS, and inactive DLSS-G without foreground ownership. Independent Reflex/PCL
fixtures passed with 12/0 sleeps and 96 markers each. Rust formatting, Clippy, 74 tests and Release overlay build
passed; package tests passed 10/10. Logs and the complete limits are in .local/anomaly/input-latency/report.md.

## Which frames feed history (`fg_policy.h`)

- Not a level frame (main menu, `LoadDraw`, no level): no FINAL_SCENE, skip `no_level`.
- Scope: `RenderToTarget(rtSVP)` frames do not Present (`dxRenderDeviceRender::End`). Every frame within `FGScopeHold`
  (4) engine frames of the last one gets screen class UNKNOWN, so the Runtime keeps the inputs and refuses to generate.
- No inputs (SR stages 2-4, or Stage 5 with SR disabled or refused), no Present: skipped with that reason.
- RESET on the next fed frame after any skip, a device reset, a scope run, a presented-size change, or a camera cut
  (`sr::camera_cut`, 4 m / 45 degrees). SR's own resets (sequence, cut, settings) also reach FG through the SR pass.
- Backend switches and size changes inside the Runtime restart provider history themselves.

## Inputs

- SR Stage 5: the SR pass, under the bound identity, gives `native_fg` the pipeline's resolved depth and dense motion
  at render resolution and the jittered camera record. DLSS SR runs on the bridge's shared Streamline host.
- SR off (`SRStage` 0 or 1): `fg_inputs.cpp` copies `HW.pBaseZB` (D24S8) and the motion target (`Motion=1`) and
  writes R32 device depth and R16G16 previous-minus-current pixel motion: stored motion where valid, zero for
  unwritten first-person pixels (depth < 0.02), camera motion from depth and `clip_to_previous_clip` elsewhere.
- Depth: standard, finite far (`depth_inverted = 0`, `depth_infinite = 0`), metres (`units_to_meters = 1`).
- HUD-less colour and the back buffer at output resolution. Providers read render-size depth/motion with motion scale 1
  (pixels; DLSS-G divides by the motion width).

## Retirement

`native_fg` aborts a provider frame only when it never presented (an SVP frame, an abandoned window, a dropped
submission). Presented frames retire at PresentEnd; completed inputs never call the provider's abort, which for XeSS
would clear interpolation history (the RDR2 fix). The XeSS fixture shows `reset=0 source_gap=1` through gameplay and
`reset=1` only after the menu and scope runs. Shared surfaces are reused only after the vendor's retirement fence
(`rsf_native_fg_retire`). The adapter holds no device reference between frames; its SR-off textures are released at
device reset.

## Hybrid GPU (rendering adapter differs from the display adapter)

The fixtures create the device on the NVIDIA adapter (vendor 0x10de, adapter 1; adapter 0 is Intel UHD). The facade's
`GetContainingOutput` reports an output of the Intel UHD adapter. The bridge creates its D3D12 device and queues on
the D3D11 device's adapter and the physical flip chain through the engine's factory on that queue; the OS copies across
adapters. Every backend generated and presented under that arrangement in the fixtures. Things in the bridge that depend
on it: Auto FG picks by the rendering adapter's vendor; tearing support comes from the engine's factory; with
runtime switching the physical chain is created at the first Present or `GetBuffer`, and for a non-windowed engine
chain the bridge then calls `SetFullscreenState(TRUE)` on it. On an output owned by the iGPU that call may fail, and
the bridge returns the failure to the engine's Present (it does not fall back to stock presentation at that point).
Not exercised: the fixtures and every game run so far are windowed. Prefer windowed or borderless.

## Diagnostics and keys

INI (`[Anomaly]`, Observe mode): `FG`, `FGBridge` (default 1, so FG can be switched on live from the overlay), `FGBackend` (1, 3, 4, 5, 7 Auto), `FGGeneratedFrames`, `Latency`,
`FrameLimitUs`, `FGScopeHold`, `FGDebug`, `FGDevelopment`, `FGCapture` (+`Start`, `Count`, `Interval`, `Dir`); vendor
directories are SR's (`SRStreamlineDir`, `SRFidelityFXDir`, `SRXeSSDir`). The bridge installs only at cold start.

Log every 120 render frames: `Anomaly FG window:` source, level, tagged, fed, presented, unpresented, scope frames,
skips by reason, resets by reason, Runtime tagged, vendor presented, captures; `Anomaly FG state:` owner, backend,
choices, switch result, requested/effective mode and count, active, Runtime reason, last result, Reflex, pacing owner.
`FGDebug=1` adds the Runtime's eligibility bitmask and pacing lines. The overlay offers FG backends, generated frames,
latency and frame limit once the bridge is installed, and reports effective FG from the provider's status.

## Fixture results (12 November 2026, RTX 4070 Laptop, build/wk-fg Release)

`tests/anomaly_fg_device.cpp` calls the production `fg_stage.cpp` exactly as the observer does, on a
`D3D11CreateDeviceAndSwapChain` chain (two buffers, sequential, windowed) that the bridge intercepted at cold start.
Each run: gameplay, a menu run, gameplay with an overlay panel drawn in `before_present`, a scope run (every other frame
an SVP frame without Present), gameplay, `ResizeBuffers` device reset, gameplay.

| Run | Result |
| --- | --- |
| DLSS-G, SR Stage 5 (DLSS SR on the shared host, 853x480 to 1280x720), live switch to FSR3, XeSS, Off, DLSS-G | PASS; 2 presents per rendered frame in gameplay; 0 Runtime tags in menu and scope runs; 640 SR evaluations |
| DLSS-G, SR off | PASS (in one of three runs DLSS-G stayed inactive for the 60 frames after `ResizeBuffers`; it recovered within a few frames in the others) |
| FSR3, SR off | PASS; HUD-less dump excludes the HUD rectangle |
| XeSS, SR Stage 5 | PASS |

## Open problems

- Game acceptance: everything in the list below.
- `SRStage=5` with SR switched off in the panel, and Stages 2-4, give FG no inputs (skip `no_inputs`).
- DLSS-G after `ResizeBuffers` occasionally stays inactive for a while (seen once in a fixture).
- With DLSS SR and a non-DLSS FG provider, Streamline logs "presentCommon() was not observed" (shared host, also AC7).
- PDA texture content is world material: FG interpolates it like the world (possible text ghosting).

## Game acceptance list

1. Generated frames presented: `Anomaly FG state` effective mode 1 and active, vendor presented about twice the engine
   presents at 2x, for DLSS-G, FSR3 and XeSS; `SRStage=5` and SR off.
2. Pacing: frame-time graph (PresentMon or `FGDebug=1` pacing lines) even between real and generated frames; Reflex
   and frame limit behaviour with `Latency` 0/1/2 and `FrameLimitUs`.
3. HUD/UI not ghosted: HUD, crosshair, hit marker, inventory, dialogs and the RSF panel static while turning; compare a
   `FGCapture` HUD-less dump against the presented frame.
4. PDA (3D and 2D) and scope transitions: no smear on open/close; scope runs show skips `scope` and `scope_end` resets.
5. Menu, loading and level change: `no_level` skips, a reset on return, no frozen or smeared frames.
6. Overlay FG controls: toggling FG, switching backend live (switch result 0), generated frames 1-3, latency modes.
7. Teardown: quit to desktop and vid_restart/resolution change without hang or device removal.
