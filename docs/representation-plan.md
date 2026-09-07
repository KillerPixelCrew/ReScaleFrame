# Plan: Representation — UI extraction, presentation bridge, SR+FG for DLSS, FSR and XeSS

## Context

The AC7 super-resolution input chain is done and game-tested (7 Sep 2026): jitter, translucent
velocity, composed scene colour, translucent depth, and the separate translucency layer at native all
reach the backend. What remains is presentation, and today's work on it established, from source and
runs rather than inference:

- AC7 rasterizes its front-end interface at a hardcoded 1920x1080
  (`UWidgetToTextureConverter_Setup 0x1404d5c10`, constants at `0x1425f1cac`/`0x1425f1ce4`). The
  interface's own resolution was never the problem.
- On the screens that look soft (briefing, hangar) the interface is drawn as world-space widget
  quads (`Widget3DPassThrough_Translucent`) **into AC7's own render-resolution `R8G8B8A8` UI layer**
  (`0x308F6550`/`0x308F6190`, alternating), depth-tested against the scene, and composited by the
  game into the composite before the upscale. Reinsertion also misses an unpromoted intermediate
  (`0x308E2770`) on that path. Promotion cannot sharpen this; extraction can.
- No frame-generation SDK runs on D3D11 (`sl.dlss_g.dll` manifest `"rhi": ["d3d12","vk"]`;
  FidelityFX FG and XeFG are D3D12-only). Every FG path needs a DX11→DX12 presentation bridge.
- All three FG SDKs want the same UI contract: `Final = UI.rgb + (1 − UI.a) × HUDless.rgb`,
  premultiplied, alpha 0 where no UI, same extent/format/colour space as the back buffer, plus
  post-grade HUD-less colour. One `R8G8B8A8_UNORM` layer satisfies all of them (never R10G10B10A2,
  which is AC7's back buffer).

The user's decisions: the plan covers **everything needed to be clear and set for SR and FG across
DLSS, FSR and XeSS as one framework**; the existing design is **not a boundary** (tap / reinsert /
bridge / loader-hosted structure may be reworked on merit); no lazy routes — from references
(Skyrim CS, fo4test, FO4 CS, OptiScaler, SpecialK, UE 4.18.3, the AC7 SDK, the three vendor SDKs).
What stays fixed is `AGENTS.md`: C ABI at module boundaries, game rules in `games/ac7`, presentation
and vendors in `runtime/`, warnings as errors, expected-byte guarded patches, honest status, MIT game
SDK free of GPL includes.

## Decisions (settled, with the reason)

| Decision | Choice | Why |
| --- | --- | --- |
| UI handling | **Superseded 7 Sep 2026 by measurement, see below.** Divert every classified UI draw into a mod-owned premultiplied `R8G8B8A8_UNORM` layer at back-buffer extent, no depth; scene stays HUD-less; composite at present when FG is off; hand layer + HUD-less to FG when on | Skyrim CS `SetUIBuffer`; the vendor contract; the only thing that sharpens geometry-rasterized UI |
| UI handling, corrected | Promote AC7's own interface target to output resolution and scale the rasterizing draws' viewports, so the game's UI composite runs as it always did; take the frame generation layer from that promoted target, which already holds premultiplied colour with coverage | The divert works and the insertion point is wrong: the quads read the scene and its glow chain, so they are composites rather than overlays, and compositing at present skips AC7's own UI composite, glow and grade. Promotion keeps all three and yields the same layer the vendors ask for |
| Depth for diverted quads | Divert regardless of the scene depth being bound; bind no DSV (overlay semantics); `RSF_UI_DEPTH=0` default, promoted depth via `depth_replay`'s output-res depth later | The briefing quads all bind scene depth; "divert only depth-free draws" would extract nothing |
| Alpha | Per-draw blend patch on "over" blends only (`DestBlend == INV_SRC_ALPHA`): alpha ops → `ONE/INV_SRC_ALPHA/ADD`, colour ops untouched, cached by original state pointer; Slate untouched (already accumulates); additive left (alpha 0 = pure add); modulate counted, not representable | UE base-pass `BLEND_Translucent` keeps alpha at 0 from a transparent clear (`BasePassRendering.h:1105`); Slate does not (`SlateRHIRenderingPolicy.cpp:730`); Skyrim CS `GetPatchedAlphaBlendState` |
| Identification | Plugin-side classifier: shader-hash override table → Slate input-layout fingerprint (into non-widget target) → reads a registered widget render target with a translucent blend → scene | Every format rule so far was wrong once; SpecialK's hash registry; UE's `FSlateVertex` layout is distinctive |
| Presentation | `IDXGISwapChain4` facade with PASSTHROUGH and BRIDGED modes; BRIDGED never creates the game's D3D11 chain; the FG vendor creates/wraps the real HWND chain; shared NT-handle surfaces + shared fence, two-deep ring, `UNTIL_NEXT_PRESENT` | fo4test / FO4 CS / OptiScaler `Dx11wDx12SC`; `architecture.md` bridge section |
| SR routing | DLSS-SR native D3D11 unless DLSS-G owns FG (Streamline is one device per process → SR moves to the D3D12 proxy via the bridge); FSR-SR via bridge (ffx_api ships DX12 only); XeSS-SR D3D11 on Arc, D3D12 via bridge elsewhere | Vendor facts in the appendix |
| FG order | FSR-FG → DLSS-G → XeFG (+XeLL); Claw run as the device-test milestone | FSR runs on the RTX 4070 in front of us with no XeLL and no Streamline conflict; DLSS-G is the user's GPU; XeFG runs here only in non-Intel mode (1 frame) |
| Compositor rule | Exactly one compositor per rendered frame: our D3D11 composite when FG is off; the vendor composites generated frames; egui draws into the UI layer after the game's UI so it rides both | Skyrim CS flicker rule (`FidelityFX.cpp:167`) |
| Render scale | Through the game's own per-context table (`FGraphicsSettingsManager`), expected-byte guarded; `r.ScreenPercentage` write only as announced fallback; `keep_render_scale` deleted once game-tested | The game overwrites the cvar on every transition; the table is where the value belongs |
| Texture LOD bias | Apply `log2(render.y / output.y)` as a mip bias offset while SR is active, and 0 when it is not; sampler-state patch at creation, off switch `RSF_SR_MIP_BIAS` | Rendering at half height samples mips chosen for half height, so textures arrive soft and no upscaler recovers detail that was never sampled. Every upscaler integration does this and ours did not (Luma `main.cpp:1486`) |
| SR resolution model | Create the SR feature once at the output extent with the vendor's dynamic-resolution flag set, and vary only the per-evaluation render extent | Our screen policy changes the scale on every menu/flight transition; a feature created at a fixed render extent has to be destroyed and rebuilt at each one, which costs the history. Luma `main.cpp:917` sets it unconditionally for the same reason |
| View uniforms | Read at the game's own `Map`/`Unmap` of the constant buffer, not through a staging copy | `constant_buffer_read.cpp:65-83` creates a staging buffer, `CopyResource`s and maps it, which is a GPU round trip per read and lands a frame late. The data is already in CPU memory at the moment the game writes it (Luma `main.cpp:1344-1396`) |
| Engine's own `r.HDR.UI.CompositeMode` path | Recorded, not used | Covers Slate only, HDR-encoded LUT composite, front-end UI on the screens that matter is quads |
| Deleted mechanisms | interface-target promotion, format-based interface identification, the shape hunt and its collection, relooks, `present_blit`, `dlss_bridge.c`, `keep_render_scale` (after M3), `rsf_observer_present_fn` | Replaced by extraction, the classifier, `fullscreen_pass`/`composite`, the orchestrator session |

## Architecture

### Module map and link direction

```
sdk/game/            rsf_game_sdk (INTERFACE, MIT)   game_api.h (ABI 2), game_frame.h (new)
runtime/contract/    rsf_contract (INTERFACE)        backend.h, presentation_types.h        -> GameSDK
runtime/graphics/    rsf_graphics (STATIC)           frame_tap (ABI 7), allocation_watch (was d3d11_observer, ABI 5),
                                                     scene_promote (was scene_reinsert), fullscreen_pass (from present_blit),
                                                     d3d11_state (+draw_state_save/restore), overlay_renderer, depth_replay,
                                                     motion_decode, resource_roles, resource_ref, texture_dump, overlay_input
runtime/presentation/ rsf_presentation (STATIC)      present_intercept, swapchain_facade, d3d12_bridge, shared_surface,
                                                     presentation_ring, ui_layer, hudless, composite, present_loop, d3d12_util.h
                                                     -> Graphics, Contract, d3d12
runtime/backends/dlss|fsr|xess/  rsf_backend_<v> (STATIC, RSF_HAVE_<VENDOR>)  backend_<v>.h; <v>_session, <v>_sr, <v>_fg (+xell)
                                                     -> Contract, d3d11, d3d12, dxgi
runtime/backends/rsf-upscaler/   Rust crate, kept; capability model + #[repr(C)] mirror (abi.rs), FG fields
runtime/orchestrator/ rsf_orchestrator_core (OBJECT) session, settings, backend_registry (+negotiate), sr_pipeline (was dlss_pipeline),
                                                     frame_assembly (on backend.h), frame_sequencer, overlay_host (moved), status
                                                     -> Contract, Graphics, Presentation, Backend{DLSS,FSR,XeSS}
games/ac7/           rsf_ac7_rules (STATIC, replaces rsf_ac7_scene_color)  scene_color (kept), frame_tail, ui_rules, widget_targets,
                                                     screen_policy, graphics_settings, patches, ui_shader_overrides -> GameSDK, Graphics, Contract
                     rsf_game_ac7 (SHARED)          plugin.cpp implements hooks
loader/proxy/        rsf_proxy_dinput8 (SHARED)     carrier: DirectInput forward, ini, decrypt wait + patch application, hotkeys -> commands
                                                     -> LoaderDiagnostics, OrchestratorCore, AC7Rules, AC7View (still one dinput8.dll to deploy)
```

Backends never include presentation; presentation never includes a backend header (calls
`rsf_fg_provider` through the vtable the orchestrator supplies); `games/ac7` includes neither;
`sdk/game` includes nothing else. `frame_assembly.h` stops including `dlss.h`.

### Vendor-neutral contract: `runtime/contract/include/rescaleframe/backend.h`

Enums: `rsf_vendor {NONE, NVIDIA, INTEL, AMD}` (Rust order), `rsf_gfx_api` bitmask `{D3D11, D3D12,
VULKAN}`, `rsf_quality {NATIVE, QUALITY, BALANCED, PERFORMANCE, ULTRA_PERFORMANCE, ULTRA_QUALITY}`,
`rsf_lifetime {ONLY_NOW, UNTIL_NEXT_PRESENT}`, `rsf_ui_mode` bitmask `{NONE, BACKBUFFER_UI,
HUDLESS_UI, BACKBUFFER_HUDLESS, BACKBUFFER_HUDLESS_UI}`, `rsf_motion_units`, `rsf_latency_mode {NONE,
XELL, PCL}`, `rsf_sr_route {NONE, NATIVE_D3D11, BRIDGE_D3D12}`, `rsf_backend_result` (`OK,
INVALID_ARGUMENT, ABI_MISMATCH, NOT_COMPILED, LOAD_FAILED, MISSING_ENTRY_POINT, INIT_FAILED,
NOT_SUPPORTED, NOT_READY, FEATURE_FAILED, STALE_RESOURCES, NEEDS_RESTART, WRONG_API`).

Structs (all `struct_size` first): `rsf_backend_probe_desc {game_api, adapter_luid, d3d11_device,
d3d12_device, runtime_directory_utf8, log}`; `rsf_backend_caps {vendor, name, availability, sr_apis,
fg_apis, reconstructs_camera_motion, needs_exposure, max_generated_frames, ui_modes,
supported_lifetimes, fg_owns_swapchain, sr_fg_share_session, latency_modes,
fg_requires_latency_markers, supports_dynamic_fg, driver versions, refusal_utf8}`.

`rsf_sr_provider {probe, open(rsf_sr_open_desc), plan(rsf_sr_plan), evaluate(session,
command_context, rsf_sr_frame), release_resources, close}` — `rsf_sr_frame` carries frame_id,
resource_generation, the five textures with D3D12 states, sizes, quality, jitter, motion scale and
sentinel, `rsf_camera_frame`, reset, frame time. SR inputs are always `ONLY_NOW`.

`rsf_fg_provider {probe, create_swapchain(rsf_fg_swapchain_desc → rsf_fg_swapchain_result
{present_chain, d3d12 proxies, effective max frames, effective ui mode}), set_options(enabled,
generated_frames, ui_mode), tag_frame(session, command_list, rsf_fg_frame), after_present, resize,
latency_marker, latency_sleep, get_status, destroy_swapchain}` — `rsf_fg_frame` carries frame_id
(0 when ambiguous), present_index, generation, `rsf_fg_resource {resource, state, rect, format,
lifetime, generation}` for depth/motion/hudless/ui/backbuffer, sizes, unjittered view/projection and
clip↔prev-clip, jitter, camera, frame time, reset, interpolate, generated_frames.

Each vendor header exports `rsf_<v>_sr_provider()` / `rsf_<v>_fg_provider()`; without headers the
getters exist and `probe` returns `NOT_COMPILED`; with headers and no DLL, `LOAD_FAILED` with the
path. `backend_registry.h`: `rsf_backend_registry_probe_all` and `rsf_negotiate(settings, caps[3],
game_inputs) → rsf_backend_choice {sr_vendor, fg_vendor, sr_route, ui_mode, latency,
generated_frames, restart_required}` plus reasons (the Rust `Reasons` + `FG_NEEDS_BRIDGE`,
`SR_FG_SESSION_CONFLICT`, `UI_MODE_UNSUPPORTED`). Vendor mapping per call (Streamline `slInit`
manual hooking / `slUpgradeInterface` / `slDLSSGSetOptions` with `enableUserInterfaceRecomposition`
and `eEnableFullscreenMenuDetection` / tags `eValidUntilPresent`, nulled when `interpolate == 0`;
FFX `ffxCreateContext` upscale + `FrameGenerationSwapChainForHwndDX12` + `FrameGeneration` +
`Hudless`, `RegisterUiResourceDX12{USE_PREMUL_ALPHA | ENABLE_INTERNAL_UI_DOUBLE_BUFFERING}`,
`PrepareV2` with `frameID = present_index`; XeSS `xessD3D11/D3D12Init`,
`xefgSwapChainD3D12InitFromSwapChainDesc` + `GetSwapChainPtr(IDXGISwapChain4)`,
`SetUiCompositionState`, `TagFrameResource` for HUDLESS/DEPTH/MV/UI/BACKBUFFER, `SetPresentId`,
`xellD3D12CreateContext` + markers) is in the appendix's vendor table and the architecture design.

### Frame record: `sdk/game/include/rescaleframe/game_frame.h` (MIT)

`rsf_frame_id` (u64, plugin-assigned at the input boundary, monotonic; 0 = none),
`rsf_frame_phase {INPUT, SIMULATION, RENDER_SUBMIT, SR_EVALUATED, HUDLESS_CAPTURED, UI_COMPLETE,
PRESENTED}`, `rsf_screen_class {UNKNOWN, MENU, BRIEFING, HANGAR, FLIGHT, REPLAY, VIDEO, LOADING}`,
`rsf_latency_marker` (XeLL numbering), flags `RESET, NO_SR, NO_FG, AMBIGUOUS_ID, UI_DIVERTED`.
`rsf_frame_record {frame_id, session_id, view_id, resource_generation, phase, flags, screen, render
and output sizes, input_qpc, frame_time_ms, rsf_camera_frame camera}`. `rsf_camera_frame` moves here
unchanged. Assignment: plugin → `frame_id`, `screen`, camera; orchestrator → generation (bumped on
resize, render-size change, surface/FG-chain recreation), frame time; bridge → `present_index`
(contiguous, non-TEST presents; FFX `frameID`, XeFG `presentId`). Without the outer
`FEngineLoop::Tick` hook the sequencer uses the latest begun frame and sets `AMBIGUOUS_ID`; DLSS-G
(`fg_requires_latency_markers`) then gets `interpolate = 0` with the reason reported; FFX and XeFG
interpolate on `present_index`. MFG: `RSF_FG_MULTIPLIER` 2/3/4 → generated 1/2/3, bounded by caps;
reservation at `create_swapchain`, active count via `set_options`; changing the reservation is
`NEEDS_RESTART`.

### Presentation bridge: `runtime/presentation`

Intercept (`present_intercept`): `IDXGIFactory::CreateSwapChain` (vtable 10, from a factory we
create), `IDXGIFactory2::CreateSwapChainForHwnd` (15), `D3D11CreateDeviceAndSwapChain` (IAT), and
`ID3D11Device` slots 3/5/11/12/15 once the device is known (`allocation_watch`). Installed from the
carrier's first worker at attach. A chain created before the hook armed → BRIDGED refused with
`restart_required = INTERCEPT_LATE`.

Facade (`swapchain_facade`, `SwapChainFacade final : IDXGISwapChain4`): PASSTHROUGH wraps the game's
chain and runs composite + overlay in `Present`; BRIDGED never creates the game's chain, returns the
D3D11 side of `game_backbuffer` from `GetBuffer(0)`, synthesises descs, forwards fullscreen state,
`DXGI_PRESENT_TEST` forwards only. Inner chain by FG owner: DLSS_G → Streamline proxy chain on the
`slUpgradeInterface`d factory/device/queue; FSR_FG → the chain FFX created; XEFG → the proxy from
`xefgSwapChainD3D12GetSwapChainPtr`; no owner → our own flip-model `CreateSwapChainForHwnd`
(`FLIP_DISCARD`, `max(2, requested)` buffers, tearing when supported, MSAA degrades to PASSTHROUGH).
Owners are fixed at creation; a vendor or mode change is `NEEDS_RESTART`.

D3D12 side (`d3d12_bridge`, `shared_surface`, `presentation_ring`): same-adapter `D3D12CreateDevice`
(12_0), one DIRECT queue, per-slot allocators and lists; `ID3D12Fence` shared →
`ID3D11Device5::OpenSharedFence`; surfaces created on D3D11 with `SHARED | SHARED_NTHANDLE`, opened
on D3D12, generation-stamped. Surface set: `game_backbuffer` (game format, RTV+SRV), `hudless[ring]`
(game format), `ui[ring]` (`R8G8B8A8_UNORM`, RTV+SRV+UAV), `depth[ring]` (`R32_FLOAT`, render size),
`motion[ring]` (`R16G16_FLOAT`, decoded), `present_copy[ring]` (D3D12-only), and `sr_*` when
`sr_route == BRIDGE_D3D12`.

Per-Present protocol (BRIDGED, non-TEST, render thread): (1) `s = present_index % ring`, CPU wait on
`slot_fence[s]`; (2) `CopyResource(hudless[s], game_backbuffer)`; (3) composite `ui[s]` over
`game_backbuffer`, then overlay; (4) depth/motion from this interval's SR gate into `depth[s]`,
`motion[s]`, else `interpolate = 0`; (5) `ctx4->Signal(fence11, ++v)`, `queue->Wait(fence12, v)`;
(6) list `s`: barriers, `CopyResource(present_copy[s], game_backbuffer)`, transitions to the tagged
states, `fg->tag_frame`, execute; (7) `present_chain->Present`, `fg->after_present`; (8)
`queue->Signal(fence12, ++v)`, `slot_fence[s] = v`, `ctx4->Wait(fence11, v)`; (9) clear
`ui[(s+1)%ring]` to `{0,0,0,0}`, `present_index++`, `rsf_frame_tap_end_frame`. Without an FG owner,
(6)–(7) copy into the real back buffer, so the bridge is testable before any vendor. SR through the
bridge: at the tap gate D3D11 copies scene colour/depth/motion/exposure into `sr_*`, signals; D3D12
evaluates into `sr_out`, signals; `ctx4->Wait`, then the gate binds `sr_out` as scene colour.

Resize: full fence drain, release surfaces and real-buffer references, `fg->resize` or
`present_chain->ResizeBuffers`, recreate on size/format change, `generation++`. Minimised/zero size
→ `interpolate = 0`. `DEVICE_REMOVED/RESET` → `bridge_state = dead`, forward-only. Loading, video,
menu, cuts → `NO_FG` → `interpolate = 0`, null tags for DLSS-G, `reset` from `RESET`.

### UI layer, HUD-less, composite (`ui_layer`, `hudless`, `composite`, `fullscreen_pass`)

- `ui[ring]`: `R8G8B8A8_UNORM`, back-buffer extent, RTV+SRV+UAV, premultiplied, cleared `{0,0,0,0}`
  after every Present and at creation, never bound with a DSV, shared when BRIDGED, plain when
  PASSTHROUGH; `written_this_frame` gates the composite and the tag.
- `hudless[ring]`: `CopyResource` of the graded back buffer before the composite (HUD-less by
  construction when extraction is on; on the seam when a classified UI draw is not diverted).
- `composite`: `Final = UI.rgb + (1 − UI.a) × Scene.rgb` with `ONE / INV_SRC_ALPHA`
  (`rsf_blend_premultiplied`, taken from `overlay_renderer.cpp:543-551`), point sampler, no depth;
  modes `UI`, `DEBUG_SR` (replaces F7's tonemapped blit), `DEBUG_UI` (layer on black). Built on
  `fullscreen_pass`, which is `present_blit`'s pipeline made target-agnostic (RTV-taking entry point,
  scissor saved, alpha passed through); `present_blit` is deleted.
- egui draws after the composite into the same bound target (PASSTHROUGH) or into `ui[s]` after the
  game's UI (BRIDGED), so it is on real and generated frames alike and in no FG input.
- If extraction is on and the compositor cannot run, UI is routed straight to the back buffer and the
  reason logged; a broken compositor never yields a UI-less session silently.

## UI extraction mechanics (D3D11 path)

### Identification (plugin rule in `games/ac7/src/ui_rules.cpp`, mechanism in runtime)

Registries (from `allocation_watch` hooks `CreateInputLayout` 11, `CreateVertexShader` 12,
`CreatePixelShader` 15, `CreateTexture2D` 5; records attached with `SetPrivateData` under a
ReScaleFrame GUID so pointer reuse cannot alias):
- **Slate layouts**: element signature `R32G32B32A32_FLOAT@0, R32G32_FLOAT@16, R32G32_FLOAT@24,
  B8G8R8A8_UNORM@32, R16G16_UINT@36` (5 elements; accept a 6th per-instance `R32G32B32A32_FLOAT`),
  match by format/offset/slot not semantic names; stride 40 checked at draw time from the tap's
  vertex-buffer shadow. UE creates one layout object per (declaration, VS) — register every match.
- **Canvas layouts**: `FSimpleElementVertex` (`R32G32B32A32_FLOAT@0, R32G32_FLOAT@16,
  R32G32B32A32_FLOAT@24, B8G8R8A8_UNORM@40`, stride 44) — verify against
  `BatchedElements.cpp`/`FSimpleElementVertexDeclaration::InitRHI` before coding.
- **Widget render targets**: `B8G8R8A8_TYPELESS/UNORM`, RT|SRV, mip 1, array 1, sample 1, extent in
  `RSF_UI_WIDGET_DRAW_SIZES` (default 1920x1080); cap 32, retained, evicted on address reuse.
  Confirmed at draw time: a Slate-layout draw whose target is one of these is *widget
  rasterisation* — never diverted, and the target is marked confirmed.
- **Shader hashes**: CRC32C of bytecode at creation; `RSF_UI_SHADER_FORCE_VS/PS`,
  `RSF_UI_SHADER_SKIP_VS/PS` (hex lists) resolved to pointer sets at creation; per-draw cost is a
  pointer-set compare. `PSSetShader` (context slot 9) shadowed like `VSSetShader`.

Classifier `rsf_ac7_ui_classify(const rsf_draw_desc*) → rsf_draw_class {SCENE, UI, UI_MODULATE,
SKIP}` in order: (1) override table hit; (2) Slate/Canvas layout AND `rtv0` is the back buffer (or
the viewport RT) → `UI`; Slate layout into a widget RT → widget-raster, left alone; (3) indexed,
`element_count == 6`, one target, no UAVs, a widget RT in PS slot 0..3, target neither a widget RT
nor the back buffer, translucent blend → `UI` (`DestBlend == SRC_COLOR` → `UI_MODULATE`); (4)
`SCENE`. Runs on the render thread inside the draw hook, allocation-free, on shadowed data only.
`RSF_UI_TRACE=1` logs every decision for the first N draws per screen.

**Decide once per shader, then cache the verdict by hash.** The failure this frame keeps repeating
is deciding from bindings on every draw: a running game leaves more bound than it reads, so a rule
of the form "the one that matches" eventually matches a second thing. Luma's UE path avoids it by
separating the two questions (`main.cpp:719-800`). A shader hash is first a *candidate*, from the
cheap facts alone. The first time a candidate actually draws, it is confirmed against a conjunction
of the resources bound at that moment — for Luma's TAA, two colour targets and a depth and a
velocity, all at the view's aspect ratio and no smaller than the view. The verdict is then written
into the per-hash record and every later draw of that shader is a pointer compare that never looks
at bindings again. We adopt the shape: `rsf_ac7_ui_classify` stays the pure per-draw rule, but its
result for a given shader is memoised on first confirmation, and a shader whose confirming draw
never appears stays a candidate rather than being promoted on a partial match. This is also what
makes the `UNKNOWN` count meaningful, since a stale binding can no longer graduate to `UI` merely by
being present in a later frame.

### Divert primitive (in `frame_tap`, ABI 7)

Pre-draw in all four draw hooks (`Draw`, `DrawIndexed`, `DrawInstanced`, `DrawIndexedInstanced`;
indirect/auto never diverted): prefilter by pointer compares (layout ∈ set, PS slot 0..3 ∈ widget
RTs, VS/PS ∈ force set) → build `rsf_draw_desc` from the shadow → `classify` → refuse with a counted
reason when `target_count != 1`, UAVs bound, target is the layer, no layer, widget-raster, or skip
hash. Save from the shadow only (`bound_view` — the actually bound RTV, so a plan-substituted target
restores to its promoted view; `geometry_depth`; viewports/scissors — the RS hooks shadow always, not
only while a plan is active; blend state/factor/mask from the new `OMSetBlendState` (35) shadow;
`OMSetDepthStencilState` (36) shadowed for the trace). Bind through the **originals** (never the
context's own methods): `original_set_targets(1, &layer_rtv, nullptr)`, viewport/scissor scaled per
producer (Slate/Canvas into the back buffer: 1.0; quads into a render-res target: `layer_extent /
target_extent` from the shadowed descriptor, never the plan's fixed scale), patched blend state when
the rule says so. First divert of the frame clears the layer if the ring didn't. Forward. Restore via
originals; count `draws_diverted[producer]`, `blend_states_patched`, `modulate_draws`, refusals by
reason; push a trace record for the first `RSF_UI_TRACE_DRAWS`. `ReentryGuard` becomes a depth
counter before any hook issues context calls. `rsf_frame_tap_geometry` gains an appended `diverted`
flag so `depth_replay` skips diverted draws. Status appends: `draws_diverted`, per-producer counts,
`divert_refused` + last reason, `blend_states_patched`, `modulate_draws`, `layer_clears`,
`widget_raster_draws`, `candidates_seen`.

**Skip and copy, where retargeting is not safe.** Retargeting keeps the game's own draw and moves
where it lands, which is what the widget quads need. A draw that cannot be retargeted — several
targets, unordered access views, a producer whose destination we would have to reinterpret — can
still be handled by cancelling it and issuing our own: Luma's draw hook returns a `Skip` override
and, where it has already produced the result, copies its own texture into the target the game was
about to write (`main.cpp:800-825`). It is strictly more invasive, so it is the fallback and not the
default, but naming it now matters because the refusal counters above are otherwise a list of cases
with no route. `RSF_UI_DIVERT_MODE=retarget|skip` per producer, `retarget` default.

### Reinsertion becomes `scene_promote`

Tail = `{composite, chain_targets[], scene_color, reconstruction, render size}`; interface targets
gone. `chain_targets` are the eight-bit render-resolution targets between the tonemap and the
back-buffer draw that are neither the composite nor a widget-quad destination (`0x308E2770` in the
log); the tail walk records every draw into the composite after the tonemap and points the target
watch at each candidate; a `CopyResource` hook (context slot 47) covers an intermediate filled by
copy. `depth_policy` stays (`RSF_DEPTH_POLICY drop|keep|refuse`, `RSF_REINSERT_DEPTH` alias);
`depth_mismatches` should read zero once quads no longer reach a promoted target. Restake keeps its
240-frame trigger, clears the chain set with the composite; diversion is independent of the plan.

## Game plugin: `games/ac7`

- `frame_tail.cpp`: composite identification (input ≥ half the target's height, eight-bit family),
  chain discovery, re-identification on render-size change or composite-watch drift. Relooks and the
  hunt are gone.
- `screen_policy.cpp`: `rsf_screen_class` from three facts — video (`UManaComponent::Play/Stop`
  hooks; `AUIManagerActor::ManaComponent 0x0F10`, `EManaComponentStatus::Playing == 5`; Ghidra task;
  VIDEO never reported until located), graphics context (hook on `FUN_1405f4590`: 0/5 → FLIGHT, 1 →
  MENU/BRIEFING/HANGAR until a fact separates them), no scene pass → LOADING. Maps
  `RSF_POLICY_<CLASS>` rows (`sr:,fg:,ui:,scale:`) to `NO_SR/NO_FG`, extraction, and the per-context
  render scale. Defaults: FLIGHT/REPLAY `sr:1,fg:1,ui:1,scale:50`; BRIEFING/HANGAR
  `sr:1,fg:0,ui:1,scale:50`; MENU `sr:0,fg:0,ui:1,scale:100`; VIDEO/LOADING `sr:0,fg:0,ui:0,scale:100`.
- `graphics_settings.cpp`: `rsf_ac7_render_scale_apply(percent, context)` through
  `FGraphicsSettingsManager` (`FUN_1405f5260` loads six per-context values via `FUN_1405f4590`;
  FName globals `DAT_14356eec0..ef18`; `FUN_14015fb40` references `r.ScreenPercentage` at
  `0x14265cbd8`), expected-byte guarded; jitter sequence length stays a console write; the cvar write
  remains the announced fallback. Replaces `set_screen_percentage`/`keep_render_scale`.
- `patches.cpp`: the existing four sites as a table `{name, rva, expected[], replacement[]}` applied
  by the carrier after decryption. `ui_shader_overrides.c` + `ReScaleFrame.ac7.ui.ini`.
- `plugin.cpp` implements the ABI 2 hooks table; `rendering_ready` stays 0 until SR runs through
  `hooks.prepare/start` and is game-tested.

## Game SDK ABI 2

`game_api.h` includes `game_frame.h`; `rsf_game_plugin_api` appends `rsf_game_hooks_api {prepare,
start, quiesce, stop, classify_draw, screen_policy, apply_render_scale, fill_camera}`;
`rsf_host_api {log, frame_begin, frame_phase, frame_flags, latency_marker, latency_sleep,
setting_get, patch_code}`; `rsf_draw_desc` (layout, shaders and hashes, rtv0 facts, blend and DS
state facts, topology/element/instance/stride, viewport, first 16 PS SRVs). Textures never cross the
SDK; the plugin says which draw is what and what the camera is. `tests/plugin_contract.cpp` keeps
`rendering_ready == 0`, adds hooks-table presence, a synthetic Slate-layout draw classifying `UI`,
`entry(1, ...) == ABI_MISMATCH`; `tests/sdk_c_header.c` compiles both headers as C.

## Configuration and overlay

New keys (all read once at attach, env over ini, unknown keys logged once; every mechanism has an
off switch): `RSF_UI_CLASSIFY`, `RSF_UI_EXTRACT` (0 until game-tested), `RSF_UI_DIVERT_SLATE/WIDGET/
CANVAS`, `RSF_UI_DEPTH`, `RSF_UI_BLEND_PATCH`, `RSF_UI_CLEAR_ORIGINAL`, `RSF_UI_WIDGET_DRAW_SIZES`,
`RSF_UI_TRACE`, `RSF_UI_TRACE_DRAWS`, `RSF_UI_COMPOSITE`, `RSF_UI_HUDLESS`, `RSF_UI_ALPHA_CHECK(_FRAMES)`,
`RSF_UI_SHADER_FORCE_VS/PS`, `RSF_UI_SHADER_SKIP_VS/PS`, `RSF_UI_SHADER_OVERRIDES`; `RSF_POLICY_MENU/
BRIEFING/HANGAR/FLIGHT/REPLAY/VIDEO/LOADING`; `RSF_SCREEN_PERCENTAGE_PATCH`, `RSF_VIDEO_DETECT`;
`RSF_PRESENTATION auto|d3d11|bridge`, `RSF_BRIDGE_RING`, `RSF_BRIDGE_LIFETIME`; `RSF_SR_VENDOR`,
`RSF_SR_QUALITY` (alias `RSF_DLSS_QUALITY`), `RSF_SR_MIP_BIAS`, `RSF_SR_DYNAMIC_RESOLUTION`,
`RSF_VIEW_READ`, `RSF_UI_DIVERT_MODE`, `RSF_FG`, `RSF_FG_VENDOR`, `RSF_FG_MULTIPLIER`,
`RSF_LATENCY`; `RSF_SL_BIN` (alias `RSF_STREAMLINE_BIN`), `RSF_FFX_BIN`, `RSF_XESS_BIN`;
`RSF_DEPTH_POLICY`. Removed: `RSF_UI_HUNT_*`, `RSF_DLSS_OUTPUT_*`. The 13 undocumented existing keys
get documented. `docs/review.md` finding 5 closes with `tests/config_parse.cpp` (the parser already
does hex and honest zero; the docs are stale).

Overlay ABI 3 (append-only, `abi.rs` offset tests pin `size_of` and the first appended offset —
note `rsf_overlay_stats` is 124 bytes of fields padded to 128, so the first append lands at 124):
stats gain `sr_vendor_effective, sr_route, fg_vendor_effective, fg_multiplier_requested/effective,
frames_generated, ui_extraction_on, ui_draws_diverted, ui_draws_modulate, ui_layer_generation,
hudless_available, bridge_state, present_index, restart_required, screen_class,
latency_mode_effective, fg_last_vendor_result`; intent gains `sr_vendor, fg_vendor, fg_multiplier,
ui_extraction, depth_policy, composite_mode` (+ `_changed`); the existing `quality`/`enabled` intents
get wired. The panel shows requested and effective side by side; nothing is a rate.

## Build, vendoring, CI

`vendor/fidelityfx/{include, bin/x64}` and `vendor/xess/{inc, bin/x64}` beside `vendor/streamline`
(gitignored); `RSF_HAVE_FFX`/`RSF_HAVE_XESS` discovery mirroring the DLSS target (cache path, `EXISTS`
probe, `SYSTEM`/`/external:W0` includes, `NOT_COMPILED` without headers). All three runtimes loaded
by absolute path (`LOAD_FAILED` with path, never a loader error). `rsf_presentation` and the backends
link `d3d12`; barrier helpers in `d3d12_util.h` (no `d3dx12.h`). CI: a two-leg matrix on
`windows-2022` — `vendor: none` and `vendor: headers` (pinned sparse clones of Streamline 2.12.0,
FidelityFX-SDK, intel/xess into `vendor/`) so every `RSF_HAVE_*` branch compiles; `cargo test
--workspace --locked` added to `eng/verify.ps1` and CI (review finding 8); device fixtures on WARP
with the debug layer (any `D3D11_MESSAGE_SEVERITY_ERROR` fails). `docs/dependencies.md` gains
FidelityFX (MIT headers/DLLs, notice file) and XeSS (Intel Simplified Software License, binary
redistribution terms to confirm) rows and the DLSS-G/PCL/Reflex DLLs; note that the Community
Shaders bridges were studied, not copied.

## Milestones (each ends deployed to the AC7 install and judged by one run of the standard route)

Standard route: title → main menu → hangar → briefing (F8 at the menu, F9, F6 at the briefing, F10
once) → mission (takeoff, cockpit view, HUD view, F10 in flight) → pause → results/replay → back to
menu through a mission load → one video. Deploy topic `.local/m<N>-<name>/` with backups and
`deployment.json`; `eng/deploy-ac7.sh` (refuses while the game runs, hashes, records, never
overwrites the user's ini, `--rollback <stamp>`); `eng/collect-run.sh` + `tools/summarize-run.py`
extract the pass/fail lines into the research note. Every milestone ships the attach banner (`rsf:
build <ver>+g<sha>[-dirty] <toolchain> <time>, dinput8.dll sha256, ini path, env overrides`) and
one `config: KEY=value (ini|env|default)` line per key.

**M0 — compile again, decide the four uncommitted files.** Revert the four `hunt_shape_dirty`
lines; remove the declared-but-unimplemented `rsf_frame_tap_set_divert` and the mid-struct
`draws_diverted` (both return in M2, appended, ABI 7); keep the reinsert set of 6 with
`RSF_REINSERT_ABI_VERSION 3`; keep the hunt budget/log-cap change (the hunt is deleted in M2).
Docs: drop the stale "Parser limitations" paragraph, tick review finding 5 pending its test. Pass:
`ctest --preset linux-cross-debug` 15/15; briefing run unchanged.

**M1 — classify every UI draw per screen, change nothing.** `allocation_watch` creation hooks
(layout, VS, PS, blend, DS) with `SetPrivateData` records; tap shadows `PSSetShader`,
`OMSetBlendState`, `OMSetDepthStencilState`, vertex stride; `ReentryGuard` → depth counter;
`ui_rules.cpp` + registries; the UI rule leaves `dlss_bridge.c`. Ghidra recorded: FGraphicsSettings
manager, `r.HDR.UI.CompositeMode` path, `DrawWindow_RenderThread`, whether AC7's `UWidgetComponent`
has a `Space` field. Pass: per screen `ui: screen <n>: <N> draws with a 1920x1080 input: slate <a>
(viewport <b>), widget quad <c>, canvas <d>, converter <e>, unknown-1920 0`; first-N trace lines
carry ps/vs hash, layout, blend ops, depth state, target and inputs; `viewport` exactly 1 per frame;
`tap: draws on other contexts 0`; `perf: tap <ms>/frame` under 1.0 ms. Not claimed: any picture
change.

**M2 — divert into the UI layer and composite at present, FG off.** `ui_layer`, `fullscreen_pass`,
`composite` (PASSTHROUGH facade or, until the facade lands, the present hook); divert primitive with
blend patch; `scene_promote` replaces `scene_reinsert` (interface targets deleted, chain targets
added); egui into the layer/after the composite; alpha-accumulation check every 30 frames
(`GenerateMips` on a twin, read the last mip through staging). Keys `RSF_UI_*`. Pass: briefing and
hangar at 50% with F6 on: `ui: diverted <n> (slate a, quad c), leaked 0, blend patched <p>, layer
mean alpha > 0, composite applied == frames presented, hudless by construction 1`; F10 back buffer
shows no interface; briefing text compares to the native capture within `tools/compare-captures.py`'s
2/255 budget; flight/pause/video/menu regress nothing. Not claimed: FG readiness, Windows, occluded
world-space UI in flight (`RSF_UI_DEPTH` policy decides), bloom/texel-addressing effects.

**M3 — frame identity, eligibility, the game's own screen-percentage table.** `rsf_frame_record`,
`screen_policy` with video/context/loading facts and named fallbacks, `graphics_settings`
per-context patch, `keep_render_scale` demoted to watchdog then deleted; the view uniforms move to
the game's own `Map`/`Unmap` (`ID3D11DeviceContext` slots 14 and 15) with the staging path kept
behind `RSF_VIEW_READ=map|staging` for one milestone so the two can be compared. Pass: `screen:
context <name>(<i>) percentage 50 (ours)` per transition; `eligibility: video playing, tags nulled` …
`live after <n>` bracketing every video; `the game had reset the render scale` count 0 over two
mission loads; `frame: id <n>` strictly increasing; `view: read at map, jitter agrees with staging
within 1e-6 over <n> frames, staging copies 0`.

**M4 — presentation bridge, pass-through present, no FG.** Intercept, facade, `d3d12_bridge`,
`shared_surface`, `presentation_ring`, `present_loop` BRIDGED without an owner; `RSF_PRESENTATION=
d3d11` returns today's behaviour; any setup failure self-disables with the reason. Pass: `bridge:
proxy chain for hwnd, game format R10G10B10A2 2048x1152, inner d3d12 flip 3 buffers, luid match
yes`; `bridge: state passthrough, presented == game presents, fence waits, max wait <ms>, timeouts
0, resizes <r>, rebuilds 0`; frame time within 0.5 ms of `RSF_PRESENTATION=d3d11`; alt-enter,
minimise/restore, mission load survive; overlay visible.

**M5 — migration: contract, session, SDK ABI 2, overlay ABI 3.** `backend.h`, `game_frame.h`,
`backend_dlss.h` wrapping today's Streamline SR, `sr_pipeline`, `frame_assembly` on `rsf_quality`,
`backend_registry` + `negotiate` (Rust fixtures through the C mirror), orchestrator session
(prepare/start/active/quiesce/stop) owning tap, pipeline, layer and bridge; `dinput8.dll` becomes a
carrier calling `rsf_runtime_attach()`; `dlss_bridge.c` deleted; overlay ABI 3; ini parser as a
testable unit. `sr_pipeline` creates the feature once at the output extent with dynamic resolution
on, and applies the texture mip bias while SR is active. Pass: the M4 report reproduced through the
new structure; `plugin: ac7 prepared, <h> hooks, <p> patches applied, 0 refused`; `sr: feature
created once, render extent changed <n> times, recreations 0` across a menu → briefing → flight
route; `sr: mip bias -1.00 applied to <n> samplers, restored on disable`; a briefing capture at 50%
with the bias off and on differs visibly in texture detail; `rendering_ready` flips to 1 only in the
commit that records this run, test updated with it.

**M6 — FG, FidelityFX (FSR 3.1 FG on the bridge).** `rsf_backend_fsr` FG: FFX FG chain
`ForHwndDX12` as the inner chain, `RegisterUiResourceDX12{USE_PREMUL_ALPHA |
ENABLE_INTERNAL_UI_DOUBLE_BUFFERING}`, HUD-less = the shared copy, `PrepareV2` per frame, our
compositor off while FG is on. Pass (flight at 50%, DLSS-SR + FSR-FG 2x; briefing/menu ineligible;
video): `fg: vendor fsr, requested 2x, effective 2x, generated ≈ presented while eligible, skipped
(ineligible i, no hudless h), compositor vendor, egui in ui layer 1`; presented ≈ 2× rendered (a
counter, not latency); overlay visible on generated frames.

**M7 — FG, DLSS-G (Streamline).** Inner chain through the `slUpgradeInterface`d factory/device/queue,
features `{DLSS_G, PCL, Reflex}`, tags `HUDLessColor` + `UIColorAndAlpha` (`eValidUntilPresent`),
`enableUserInterfaceRecomposition`, `eEnableFullscreenMenuDetection`, inner chain rebuilt on every
toggle; DLSS-SR moves to the D3D12 proxy via the bridge (single `renderAPI`), the "round trip that
must earn its cost" measured. Pass: `fg: dlss-g supported <yes|no: reason>`, `slUpgradeInterface
factory ok, device ok`, the M6 lines with `vendor dlss-g`, `fullscreen menu detection events <n>`,
`bridge: rebuilds == fg toggles`. Run under Proton with `PROTON_ENABLE_NVAPI=1`.

**M8 — FG, XeFG + XeLL.** `xefgSwapChainD3D12InitFromSwapChainDesc` inner chain,
`SetUiCompositionState` with `BACKBUFFER_HUDLESS_UITEXTURE` (Intel's recommendation for UE titles),
tags for HUDLESS/UI/DEPTH/MV/BACKBUFFER, `SetPresentId`, `xellD3D12CreateContext` with markers; here
only non-Intel mode. Pass: `fg: xefg init ok, max interpolated 1 (non-intel), ui mode
backbuffer_hudless_uitexture`; `xell: markers per frame` reported honestly until the outer-frame
hook exists.

**M9 — SR per vendor behind `rsf_sr_provider`.** FSR-SR via the bridge (measure the round trip);
XeSS-SR D3D11 (`NOT_SUPPORTED` on the 4070, device-tested on the Claw). Pass: `sr: vendor <v>,
evaluated <n>, refused 0, render WxH output WxH` with the DLSS path's picture counters.

**M10 — Claw device run.** XeSS-SR D3D11 + XeFG 3x/4x + XeLL: the only device-tested milestone;
`max interpolated` above 1 and complete markers are what change.

## Tests (synthetic, Wine/DXVK; skip with 77 and a reason when a device is missing)

Dedicated prefix via `eng/wine-test-prefix.sh` (DXVK `d3d11/dxgi`, vkd3d-proton `d3d12core` + the
`d3d12` shim, `nvapi64` from Proton Experimental), `linux-cross-dxvk` test preset setting
`WINEPREFIX` and `WINEDLLOVERRIDES="d3d11,d3d12,d3d12core,dxgi,nvapi,nvapi64,nvofapi64,nvngx,_nvngx=n"`.
Whether DXVK-exported NT handles and fences open on vkd3d-proton under system Wine is unmeasured:
the bridge fixture prints the `HRESULT`s and exits 77; the M4 game run under Proton is the real
measurement.

| Test | Proves |
| --- | --- |
| `frame_tap` (extended) | divert retargets and restores exactly (targets, DSV, viewport, scissor, blend); per-producer viewport scale; restores a plan-substituted target; refusals (MRT, UAV, widget-raster, skip hash); `nested_hooks_keep_the_shadow`; diverted geometry flag |
| `ui_identify` (new) | Slate 5/6-element fingerprint by format/offset; `FSimpleElementVertex`; widget-RT registry rule, address-reuse eviction, cap; shader-hash registry and force/skip sets |
| `ui_layer` (new) | alpha accumulation from a transparent clear: unpatched translucent blend leaves 0, patched gives `a1 + a2(1−a1)` within 1/255, additive stays 0 with summed RGB, modulate counted; clear timing; ring index flips; HUD-less seam and at-present snapshots |
| `composite` / `fullscreen_pass` (new; nothing covers `present_blit` today) | `ui.rgb + scene(1−ui.a)` over a known R10G10B10A2 scene within 1/1023; DEBUG modes; state incl. scissor restored |
| `allocation_watch` (extended) | creation callbacks and `SetPrivateData` records survive release-and-recreate at the same address; `ResizeBuffers` callback |
| `scene_promote` | composite + chain targets; depth policy; no interface targets |
| `ac7_ui_rules` (no device) | table of `rsf_draw_desc` → class incl. `SLATE_VIEWPORT` and widget-raster cases |
| `frame_record` | ids monotonic, eligibility transitions, `tags_nulled_reason` |
| `swapchain_facade`, `shared_surface`, `presentation_ring`, `presentation_bridge` | COM contract (`QueryInterface`, `GetBuffer` stability, refcounts, `ResizeBuffers`, `PRESENT_TEST`); NT-handle open and fence round trip (77 without D3D12); slot reuse waits on the recorded fence; ten presents with frame-indexed colours read back on D3D12 |
| `config_parse` | hex, zero-is-zero, env over ini, unknown key warned |
| `backend_contract`, `negotiate`, `<vendor>_backend` | `NOT_COMPILED` without headers, `LOAD_FAILED` with headers and no DLL, ABI/argument rejection, ordering; Rust fixtures through the C mirror |
| `plugin_contract` (ABI 2), `sdk_c_header` | as above |
| `ui/overlay` `abi::tests` | ABI 3 sizes and appended offsets |

## Documentation and tracker

Create `docs/research/ac7-ui-extraction.md` (design, per-screen classification tables, divert
rationale, measurements per milestone), `docs/research/presentation-bridge.md` (intercept decision,
sync protocol, fixture HRESULTs, Windows results), `docs/research/vendor-fg-contracts.md` (the three
UI contracts side by side with citations, swap-chain ownership matrix, eligibility rules, the
Streamline single-`renderAPI` question). Correct `ac7-ui-composition.md` (retraction banners on the
superseded sections, "superseded by" link; append the briefing tail order from the log),
`ue418-hook-map.md` (FGraphicsSettingsManager RVAs; `r.HDR.UI.CompositeMode` path and
`DrawWindow_RenderThread` gate addresses; the `UWidgetToTextureConverter` addresses; the
`CreateSwapChain` intercept site), `implementation.md` (M0–M10 checklist with status classes; retire
the "picked out by its format" paragraph; tick review findings 5 and 8 with their tests),
`review.md` status lines, `runtime/graphics/README.md` (add `depth_replay`, `ui_layer`,
`fullscreen_pass`; drop the format-identification bullet), `loader/README.md` and
`ReScaleFrame.ini.sample` (every key, grouped by mechanism), `games/ac7/engine.json` (`ui`,
`render_scale.per_context`, `video`, `presentation`), `docs/dependencies.md`, `docs/tooling.md`
(Wine test prefix), `source-map.md`/`repositories.json` (every reference cited here; `fo4test` is
missing; `references/UnrealEngine` is a full clone, checked out at `4.18.3-release`
(`0a14a8d537a3`) as of 7 Sep 2026 and to be kept there, because its default branch is 5.8.2 and
several structures this project fingerprints changed shape between the two), `design.md`/`architecture.md`
(ownership and frame flow around the record). Ghidra naming stays `Class_Method`
(`FGraphicsSettingsManager_LoadScreenPercentages`, `FSlateRHIRenderer_DrawWindow_RenderThread`),
applied only once confirmed; tags `rsf-patch`/`rsf-read` on every site the runtime touches.

## Risks (mitigation → retired by)

1. Base-pass translucent UI accumulates no alpha → per-draw alpha-op patch → `ui_layer` test; game
   `layer mean alpha > 0` wherever `diverted > 0`.
2. Diverted world-space UI loses occlusion in flight → `RSF_UI_DEPTH` policy, promoted depth via
   `depth_replay` later, per-screen policy → flight cockpit run + `ui: kept in scene <n>`.
3. Pooled targets go stale → divert keys on classification, never on a texture address; layer is
   mod-owned → `reinsert: nothing redirected` never fires for the UI path.
4. Video/menu transitions → engine facts for eligibility, tags nulled, UI straight to the back
   buffer when no compositor runs → `eligibility:` pairs bracket every video.
5. R10G10B10A2 vs R8G8B8A8 → both sides `_UNORM` non-sRGB, composite in the back buffer's encoding
   → `compare-captures.py` ≤ 2/255 on a static menu with divert off/on.
6. DXVK tolerates invalid RT/DSV pairs, Windows rejects the binding → never bind a mismatched pair;
   WARP + debug layer in CI → error count 0.
7. ~~vkd3d-proton vs Windows for shared handles/fences~~ **retired 7 Sep 2026.** Under a prefix
   built from Proton's DXVK and vkd3d-proton, `shared_surface` passes every stage: the texture
   exports an NT handle, D3D12 opens it on the same adapter, and a fence signalled on one side is
   seen on the other. WineD3D returns `E_NOTIMPL` and the fixture skips there, which is why
   `eng/wine-test-prefix.sh` exists rather than the default prefix being trusted. Windows itself is
   still unmeasured, and is the easy direction.
8. Streamline wants the swap chain (`slUpgradeInterface`) and one device per process → inner-chain
   provider model; SR moves to D3D12 when DLSS-G owns FG → M7 lines.
9. FFX replaces the chain; XeFG needs its proxy + XeLL; FG toggles need chain rebuilds → provider
   `create_swapchain`/`resize`, `bridge: rebuilds == fg toggles`.
10. Per-draw classification cost → shadowed state only, cached hashes, no device calls → `perf: tap`
    under 1.0 ms at the briefing's draw count.
11. `ReentryGuard` non-nesting; deferred contexts → depth counter; `draws on other contexts 0`.
12. The game resets the render scale → per-context table patch → `reset` count 0 over two loads.
13. egui must stay on top including generated frames → drawn into the layer after the game's UI.
14. `plugin_contract` asserts `rendering_ready == 0` → flips only with the M5 evidence commit.
15. The Slate viewport draw must not be diverted → classification uses inputs; `viewport 1/frame`.
16. Pointer reuse in caches → `SetPrivateData` records, never pointer-keyed maps.
17. Test prefix contamination → dedicated `.local/wine-test-prefix/`.

## Verification, end to end

Per milestone: `cmake --build --preset linux-cross-debug` warnings-clean; `ctest --preset
linux-cross-debug` (and `linux-cross-dxvk`) all pass or skip with 77 and a reason; deploy with
hash verification and backup; one standard-route run; the milestone's pass lines present in
`rsf-dump.log` and summarised into the research note; overlay fields agree with the report;
`docs/implementation.md` states built / synthetic-tested / game-tested / device-tested separately.
Nothing is claimed as Windows-verified from a Wine run.

---

## Appendix — research base (kept as evidence; the plan above is the deliverable)

### Runtime map: facts that shaped the design
- The tree does not compile (`hunt_shape_dirty` undeclared at four sites); `rsf_frame_tap_set_divert`
  declared, unimplemented; `draws_diverted` inserted mid-struct without an ABI bump.
- Draw hooks forward first then report (`frame_tap.cpp:1391-1425`); the tap does not hook
  `OMSetBlendState`, `OMSetDepthStencilState`, `PSSetShader`, `RSSetState`, `ClearDepthStencilView`.
- `overlay_renderer.cpp:543-551` holds the premultiplied blend state; `present_blit` hardcodes
  alpha 1 and no blend; no test covers either.
- The UI rule lives in `dlss_bridge.c:514-567`, against `AGENTS.md:98-101`.
- `report_geometry` skips substituted targets; `invalidate_geometry` does not reset gates.

### Reference findings (Skyrim CS, UE 4.18.3)
- Premultiplied-over everywhere: Skyrim CS `HDROutputCS.hlsl:106-111`, UE
  `CompositeUIPixelShader.usf:101-103`, FFX `USE_PREMUL_ALPHA`.
- Skyrim CS UI buffer: `R8G8B8A8_UNORM`, RTV+SRV(+UAV), cleared `{0,0,0,0}` before UI and after
  Present, no depth (`HDRDisplay.cpp:836-892`, `DX12SwapChain.cpp:112-141, 283`); diverted at one
  engine stage; one compositor per frame; alpha-ops patch `HDRDisplay.cpp:928-1006`.
- Slate alpha `BO_Add, BF_One, BF_InverseSourceAlpha` (`SlateRHIRenderingPolicy.cpp:726-732`);
  base-pass `BLEND_Translucent` alpha `BF_Zero, BF_InverseSourceAlpha` (`BasePassRendering.h:1105`);
  Slate material `BLEND_Modulate` writes RGB only; `InvertAlpha` flips the convention.
- UE's own composition path `r.HDR.UI.CompositeMode` (`SlateRHIRenderer.cpp:611-702, 768-870`),
  gated on HDR; `FSlateVertex` layout (`SlateShaders.cpp:51-62`); `WidgetComponent.cpp:268-345,
  573-588, 1391-1445`; `WidgetRenderer.cpp:68-117` (`CreateTargetFor` PF_B8G8R8A8 transparent;
  `MakeRoot(DrawSize/Scale, Scale)`); frame order `GameViewportClient.cpp:1276 → 1364 → Slate`.
- Skyrim CS bridge: shared NT handles + shared fence, no D3D11On12 (`DX12SwapChain.cpp:11-141,
  217-345`). Checkout lacks `Runtime/RHI` and `Windows/D3D11RHI`.

### Luma's generic Unreal path, read on 7 Sep 2026
Checked after a report that it delivered full DLSS in AC7; the report was withdrawn and the source
agrees with the withdrawal. `main.cpp:908` fixes `sr_render_resolution_scale` at 1.0 with the comment
`DLAA only`. It substitutes DLSS for the engine's TAA pass at matching extents and drives no
resolution of its own, so there is no working AC7 upscale to take a shortcut from. What it does
contribute is above: the mip bias (`:1486`), dynamic resolution (`:917`), CPU-side view uniforms at
`Map`/`Unmap` (`:1344-1463`), confirm-once-then-cache-by-hash (`:719-800`), and skip-and-copy
(`:800-825`).

Two of its conventions we checked and already match, so nothing changes. Jitter: Luma takes it from
the projection, `matrix_a.m20`/`m21` of `ViewToClip` (`:1410`), deliberately not from the cbuffer's
jitter field, which `shader_detect.hpp:17` calls unreliable across UE versions; it converts with
`x * width * 0.5` and `y * height * -0.5` (`:1062`). We read `TemporalAAJitter` at `0x720` and use
the identical conversion against the view rect (`view_uniforms.cpp:244-256`), and
`ac7-frame-capture.md:67` records that the field was verified equal to row 2 columns 0/1 of
`ViewToClip` in this build. The projection is the more portable source and is in the same buffer, so
it is the fallback if a game update moves the field. Its SR settings for UE — HDR colour, inverted
depth, unjittered motion vectors, auto exposure — are what `dlss_streamline.cpp:478-499` already
sends.

### The briefing tail from the run log (`.local/observe/rsf-dump.log` ~425120-425240)
Glow pass (1920x1080 → 1920x1080, no depth) → scene lighting into scene colour `0x308F4390` with
depth `0x307329D0` → four widget quads (6 indices, reading 1920x1080 widget textures, writing
1024x576 `R8G8B8A8_TYPELESS` `0x308F6550`/`0x308F6190`, **scene depth bound**) → tonemap into
composite `0x308F3FD0` → unwatched step → `0x308E2770` → AC7's UI composite pass reads
`0x308E2770` + `0x308F6550` + glow/lens → composite again → upscale to the R10G10B10A2 back buffer.
Ghidra: `FUN_141343660` = `DrawWindow_RenderThread`; gate = `DAT_143c783c3 && DAT_1436a2e96 &&
FUN_140df9ea0(...) && DAT_143c8f250+4 != 0 && FUN_141220f10()`; composites `FUN_141351120`
(scRGB) / `FUN_1413515d0` (PQ) through a 32³ LUT.

### AC7 SDK facts
Converter holders: `AUIManagerActor::FrontWindowConverter 0x0D48`, `ANimbusHUD::HudWidgetConverter
0x0470`, `HudPostProcessConverter 0x0478`, `AStereoWidgetActor` `0x0370/0x0378`.
`UWidgetToTextureConverter {DrawSize 0x28, Widget 0x30, RenderTarget 0x48, DownSampleRT 0xC0,
BlurXRT 0xC8, BlurYRT 0xD0, RenderTargetWithGlow 0xD8}`. AC7's `UWidgetComponent`:
`bEnableDownsampleRenderTarget 0x0828`, `DownsampledRenderTargetArray[2] 0x0830`, `GlowRenderTarget
0x0840`, no `Space` field in the dump. `UNimbusGameInstance::HUDWidgetRenderTexture 0x01F0`.
`AUIManagerActor::FullscreenMovieWidget 0x0D60`, `ManaComponent 0x0F10`; `UManaComponent::Play/Stop/
IsPlaying`, `EManaComponentStatus::Playing = 5`. `EGraphicsScreenPercentageSettings` Gameplay 0,
NonGameplay 1, VR 2-4, MPGameplay 5; `EGraphicsSettingsWindowsDrawScale` S50..S200. `UTexture` native
span `0x006B..0x00C7` holds `Resource` (offset unmeasured). `ANimbusHUD::DrawWidget*` immediate draws
bypass converters.

### Binary facts found this session (record in the hook map)
`FGraphicsSettingsManager`: `FUN_1405f5260` → six `FUN_1405f4590(this, x, 0..5, flag)`; FName
globals `DAT_14356eec0..ef18`; `FUN_14015fb40` references `r.ScreenPercentage` at `0x14265cbd8`.
`r.HDR.UI.CompositeMode` strings `0x142bc44c8`, `0x142bc5140`, `0x142cce798`; `FCompositePS0/1`
`0x142bc7d58`/`0x142bc7e28`; `FSlateElementVS` `0x142bc5320`; referencing functions
`FUN_140315560`, `FUN_141343660`, `FUN_14164a840`. `UWidgetToTextureConverter_Setup 0x1404d5c10`,
`_SetupVirtualWindow 0x1404d69a0`, front-end caller `0x1406243e0`, constants `0x1425f1cac`
(1920.0f) / `0x1425f1ce4` (1080.0f).

### Vendor contracts and D3D11 FG precedents
Streamline `ProgrammingGuideDLSS_G.md:264` (UI premultiplied, alpha 0 where none, not
R10G10B10A2; HUD-less same colour space/post-processing; tags `eValidUntilPresent`; null tags in
loading/menu/video `:345`; `enableUserInterfaceRecomposition`; chain recreated on toggle `:966`;
D3D11 has no device proxy). FFX `ffx_framegeneration.h:60-64` flags, composite `(1−a)·color + ui`,
`ENABLE_INTERNAL_UI_DOUBLE_BUFFERING` lifetime rule, HUD-less double-buffered under async compute,
DX12-only backend. XeFG `xefg_swapchain.h:100-111, 268-294` resource types and UI modes,
premultiplied default, `BACKBUFFER_HUDLESS_UITEXTURE` preferred for UE (`guide:594-597`), D3D12 +
flip model + XeLL, non-Intel max 1 frame; XeSS-SR D3D11 Arc-only (`libxess_dx11.dll`). Blueprints:
`fo4test` (`DX11Hooks.cpp:42-116`, `DX12SwapChain.cpp`, `Upscaling.cpp:495-511`), FO4 CS
(`Upscaling.cpp:2177-2217`), OptiScaler `Dx11wDx12SC` (hidden-HWND real chain, no D3D11 HUD-fix).
SpecialK shader-CRC HUD registry (`d3d11.cpp:3297-3313`); ReShade `generic_depth_addon.cpp` picker.

### What exists today
One backend (DLSS SR, five `eOnlyValidNow` tags, D3D11 manual hooking); `sl.dlss_g.dll` present and
unreferenced; no FSR/XeSS code; no D3D12, proxy, shared handles, fences, or frame identity;
orchestrator stub; everything real in `dinput8.dll`; Rust `rsf-upscaler` capability model unused;
`frame_assembly.h` includes `dlss.h`; output texture not shareable; overlay ABI 2 append-only with
pinned offsets; 14 documented + 13 undocumented config keys; CI never compiles the Streamline branch
nor runs `cargo test`.
