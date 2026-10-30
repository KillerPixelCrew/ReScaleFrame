# Red Dead Redemption 2 renderer research

## Runtime quality presets and shared overlay, 26 October 2026

**Implemented and game-tested:** DLAA/Native, Quality, Balanced, Performance and Ultra Performance
change the engine input size during play. The human confirmed the permanent build works.
In PID140840, ASI66e5b9a9...fb4d3d and Runtime6329d2cf...aa8760 produced these actual inputs
for2560x1600 output: Native2560x1600, Quality1707x1067, Balanced1485x928, Performance1280x800,
Ultra Performance853x533. Repeated transitions include Native->Quality, which shares a native
carrier enum. Evidence: `.local/rdr2-tools/quality-overlay-20261026/permanent-{game.log,result.json}`.
These are ordinary installed calls, not the earlier guarded memory diagnostic.

### Owner and reset contract

RAGE's manager+8 tree contains active settings; manager+b8 contains pending settings. Native
Apply copies the pending values into the active tree. Earlier candidates mutated the active
object, so their changes were overwritten during reconfiguration. `live-settings.json` measured
the distinct graphics groups (id8c463422, derived vtable3326488): active version37 and pending
version0. Both versions are valid. Collections start at owner+b0 and owner+240. Only RSF-owned
scalar fields are changed: collection+e8 DLSS selector, +ec DLSS mode, +f4 FSR2 carrier and
+15c native marker pump. Unrelated pending fields and string ownership are preserved.

The ScriptHook game fiber resolves the pending group with +2c736f4 using a16-byte path
`{uint32_t* ids, uint64_t count}` containing hash8c463422. The manager accessor is+2cf31f8;
+2c76114 resolves the active graphics owner. Resolve the DLSS selector from active owner+410,
count+42c: measured active choices [5,6,2,1,0,3] omit Ultra Quality4, while a newly constructed
pending owner can still contain [5,6,4,2,1,0,3]. Fixed indices are therefore insufficient.

+2c7e8f4 enters manager state9 and requests a synchronized graphics reset through manager
virtual+60 (vtable3326620 -> +23b570), then renderer virtual+188 (object3c85d78,
vtable3626150 -> +266dd1c). Allocation and resource retirement remain with the engine.
The +58a908 observer records refreshed dimensions. The same request is hooked to normalize
pending RSF-owned fields before stock Apply can turn off or replace the native carrier.
The host chooses its backend quality from actual consumed dimensions, so it does not configure
a new vendor mode against old inputs while a change is pending. Brief native fallback while
old submitted slots retire was observed, followed by accepted resized RSF output.

New guarded entries (all exact1491.50; complete guard arrays remain in engine_settings.cpp):

| RVA | Purpose | Expected entry bytes |
| --- | --- | --- |
| 2c7e8f4 | Native graphics reconfiguration | `40 53 48 83 ec 20 ba 09 00 00 00 48 8b d9 39 91` |
| 2c736f4 | Pending group lookup | `40 53 48 83 ec 30 48 83 7a 08 00 49 8b c0` |
| 58a908 | Dimension refresh observation | `48 81 ec 38 01 00 00 48 8d 4c 24 20 e8 cf d7 fc` |
| 26f5270 | DLSS resolution-table producer | `48 89 5c 24 08 55 56 57 41 54 41 55 41 56 41 57` |
| 26ecc3c | FSR2 render dimensions | `48 8b c4 48 89 58 08 48 89 70 10 48 89 78 18 55` |

Native/DLAA keeps an active Quality carrier. RAGE mode5 means disabled, not modern NGX DLAA.
The DLSS table has output dimensions at0, five optimal pairs at8, minimum pairs at30 and maximum
pairs at58; Native returns1:1 pairs for both allocation and native feature creation. On the FSR2
carrier, Native and Ultra Performance override dimensions to1:1 or1:3 while keeping a supported
native mode1..3. Non-NVIDIA hardware has policy/static/build evidence, not in-game acceptance.

### Overlay and validation

Controls live in the shared Rust overlay; native menu insertion was explicitly cancelled.
ScriptHook supplies game-thread application and input suppression. Frontend ABI2 appends
sr_quality; overlay ABI13 and game ABI19 are unchanged. Quality persists in [RDR2] SRQuality0..4.
Two source-level UI omissions were corrected: busy settings observation now retains its last
snapshot, and a full three-slot GPU ring waits for its oldest already-submitted private fence.
A deliberate WARP backlog regression retains the fourth UI submission without D3D12 errors.

The final Runtime additionally maps disabled FG to the Off selector while retaining its saved
provider for re-enable. Once the SDK is configured Off, the UI no longer reports a pending
provider switch. Successful preference writes update the UI cache immediately; Auto resolves
to the selected provider. This passed the native frontend regression and is deployed as
Runtimed7b43b45...c0e6ad6; a separate visual run of that last UI-state correction has not occurred.

Release VS2026 passed97 discovered/11 optional skips, fmt/Clippy. Rust41+19 tests/release passed
for the unchanged panel binary14369273...03e43f. Latest gate:
`.local/rdr2-tools/rdr2-fg-off-selection-verify.log`; deployment/backup hashes are in
`quality-overlay-20261026/deployment.json`. ScriptHook is required for runtime quality control.
HDR, complete HUD modes and non-NVIDIA hardware remain unproven; broad rendering_ready remains0.

Rejected attempts are retained: active-only mutation caused corruption without resizing;
GUI/XML commits and settings notifications did not request renderer reconfiguration; state9
alone still copied stale pending values. The guarded pending-owner diagnostic first established
all five correct sizes, followed by the permanent run above. No caches were changed. RTSS,
Steam and the Social Club selector correction remain enabled. Corrected analysis names and
decompilation are retained in `.local/rdr2-menu-research/quality-final-*`.

## XeSS FG activity flicker, 26 October 2026

The ScriptHook frontend exposed rapid active/inactive changes after live selection of XeSS SR
and FG. Logging actual SDK last-present results established result3 (`WARNING_TOO_FEW_FRAMES`),
enabled1/presented1 followed by success/presented2. Adding prepared reset/source-gap fields
showed resetHistory1 at the skips even with source-gap1. Bounded read-only sampling of the
deployed runtime's native FG state also found accepted inputs/reason0. An initial experiment
setting optional XeSS frameRenderTime to zero did not resolve the symptom.

Source tracing found normal RAGE PresentEnd cleanup in `rsf_fg12_latency` calling
`rsf_d3d11_present_abort` for completed/skipped CPU inputs. On the XeSS path this invokes the
provider's abort_frame, which clears history even for an unused CPU input. Completion and
resource cancellation are different operations. The correction marks those CPU inputs complete
and retains authoritative Streamline token retirement without calling provider abort. Genuine
provider cancellation remains available. No game RVA or cache is changed by this runtime fix.

Raw evidence: `.local/rdr2-tools/xess-flicker-20261026/before-retirement-fix.log`,
`live-admission.json`, and `retirement-deployment.json`. A guarded in-memory test was prepared
but not applied because the game had exited. The permanent correction passed Release VS2026
verification (82 discovered, 11 optional skips, fmt/Clippy) and was backed-up/hash-deployed as
Runtime68b7229e...5bd37a0. Follow-up gameplay evidence is now recorded in `retirement-result.json`
beside `after-retirement-fix.log`: after warm-up,83.344 seconds span4320 source frames and8640 SDK
presented frames, with37 periodic observations all active/result0 and no recurring reset/skip
transitions. The human reports improved look/feel and no orange flashes. This accepts the
retirement correction for that foreground XeSS gameplay interval, not every game mode/provider.
Residual white text comes from the ScriptHook busy observer label: retain cached readings through
brief contention and indicate stale data after1s without a successful observation. Actual inactive
samples are not delayed or filtered. The separate text correction was backed-up/hash-deployed
after normal exit at20:19 as ASIa1a7c98e...e07544c; the verified FG runtime is unchanged.
Receipt: `xess-flicker-20261026/staged-status-label/deployment.json`. Visual acceptance remains pending.

Current continuation record: [games/rdr2/RESEARCH-HANDOFF.md](../../games/rdr2/RESEARCH-HANDOFF.md).
It retains the 26 October native run-Z pixels, camera-reset evidence, Mor3a 0.82 source review,
exact current deployment and unfinished capture-bundle work. Read it before requesting another
run; older no-capture/ScriptHook-pending statements below describe historical checkpoints.

Started 25 October 2026. Evidence status follows `games/README.md`: measured, inferred or
assumed. The first sections record fingerprinting and source review; [the runtime dump
section](#runtime-dump-and-static-analysis-25-october) records the first in-game run and the
analysis of its decrypted image. Later sections record forced DX12, census diagnostics and the
Social Club frame-ownership repair. Full SR/FG execution was unimplemented at that initial checkpoint; current accepted paths are recorded above.

## Goal and decisions

Full SR and FG on every provider: DLSS SR/FG/MFG, FSR3/FSR4 SR and FG, XeSS SR/FG with XeLL.

- **DX12 is forced when RSF is installed** (user decision). The D3D12 runtime (`rsf_fg12_*`,
  `native_sr_d3d12`) covers all three vendors natively. Vulkan would add an interop bridge for
  XeSS FG and needs ABI work, since the game ABI is D3D-shaped.
- **Inputs come from the game's DLSS calls for every RSF backend** (user clarification,
  26 October). The earlier proposal to use native FSR2 as an alternate input source is superseded.
  The FSR2 census remains useful historical comparison evidence, but a fresh FSR2 run is not an
  acceptance requirement for this integration.
- **Both native DLSS and FSR selections must route into RSF's common DLSS input adapter**
  (26 October clarification). Investigate replacing those native menu controls with RSF's own
  controls so the RSF overlay is optional. This replaces native vendor execution while retaining
  the engine's temporal input production and output insertion; disabling that production is not
  an acceptable way to disable the native upscalers. Native-menu replacement is a requested
  feasibility track, not an implemented feature.
- **Latency comes from the game's native Reflex markers**, translated to Streamline PCL/Reflex,
  XeLL or Anti-Lag 2 (user-reported, supported by the strings below).
- **Deployment is an ASI module**, not a forwarding system DLL. RDR2 modding already ships ASI
  loaders, so `ReScaleFrame.RDR2.asi` is loaded by one.
- **ScriptHookRDR2 is optional.** The native C++ SDK is preferred over ScriptHookRDR2DotNet-V2,
  which runs on top of it and adds a CLR to the process. Use: screen state (pause, map, loading,
  fades, cutscenes, photo mode, Online) and the final rendered camera as a reference value for
  render-thread matrices. It is closed source, tied to each game build and not redistributable,
  so production camera and reset data must come from the render thread.
- **Red Dead Online must be refused.** The detection method is not designed yet.

## Executable fingerprint (measured)

Question: which build is installed, and can its code be read statically?

`tools/inspect-game.py` and a section-entropy pass ran on `RDR2.exe` from
`steamapps/downloading/1174180` while Steam installed build 13773296. All 86 MiB blocks were
non-zero before hashing. Output is in `.local/rdr2-fingerprint-20261025.json`.

| Field | Value |
| --- | --- |
| SHA256 | `b56c9548f670654a9b73bf25def3cd73af12e269f6e47dba28a34079adaf465e` |
| Size | 89,562,608 bytes |
| File version | 1.0.1491.50 |
| PE timestamp | 2024-03-11T09:04:46Z |
| Entry point RVA | `0x2e4fc44` |

The main `.text` (53 MB at RVA `0x1000`) has entropy 7.998 bits/byte, and a second `.text`
(20 MB at `0x6033000`) has 7.934. Code is encrypted on disk; static analysis needs a runtime
image, as AC7 did. `.rdata` (5.89) is readable. Recheck the hash after Steam stages the install.

Imports relevant to this work: `DINPUT8` (`DirectInput8Create`), `VERSION`, `WINMM`,
`NvLowLatencyVk.dll` (5), `ffx_fsr2_api_x64.dll` (7), `ffx_fsr2_api_dx12_x64.dll` (5),
`ffx_fsr2_api_vk_x64.dll` (5), `bink2w64.dll` (23), `amd_ags_x64.dll` (1) and `d3d9`
(`Direct3DCreate9Ex`). There are no static D3D12, DXGI or Vulkan imports; `d3d12.dll`,
`dxgi.dll`, `vulkan-1.dll` and `nvapi64.dll` appear as strings and are resolved at runtime.

Relevant strings: `kSettingAPI_DX11/DX12/Vulkan`; `kSettingReflex_Off/On/On_Boost`,
`rage__fwSettings__eReflexSettings`; `nvapi_QueryInterface`; `NvLL_VK_InitLowLatencyDevice`;
`ReflexStatsInit/Event/Flags/Input`, `ReflexStatsTraceLoggingProvider`; FSR2
`ffxFsr2ContextCreate/Dispatch/Destroy`, `GetJitterOffset`, `GetJitterPhaseCount`,
`GetInterfaceDX12/VK`. A string is a lead, not a verified call site.

Inferred: on DX12, Reflex goes through NVAPI by `nvapi_QueryInterface` and on Vulkan through
`NvLowLatencyVk`. The ReflexStats strings indicate PCL stats ETW. Whether the game emits markers
on AMD or Intel GPUs is unknown.

## Reference: rdr2-vulkan-frame-generation 0.82

`https://github.com/mor3a/rdr2-vulkan-frame-generation` at `bc853d0b4a9e7ba23ee1fe00c2c107092ccd596d`
(22 October 2026), MIT, cloned to `references/`. Read for method only; no code copied.

- Hooks `GetProcAddress` for `NVSDK_NGX_VULKAN_*`, then reads `Depth`, `MotionVectors`,
  `Jitter.Offset.X/Y` and `MV.Scale.X/Y` from the evaluate parameters, and the DLSS create flags
  (IsHDR, MVLowRes, MVJittered, DepthInverted, AutoExposure). The driver has no C getters for
  NGX parameters; it calls them through the parameter object's virtual table (`inputs.cpp`).
- Rockstar's NGX ApplicationId is `0x60E8AB7` (`RDR2VulkanFG.ini`), recorded from the game's
  `NVSDK_NGX_VULKAN_Init` call by `ngx_capture.cpp`.
- Camera: the default is a fixed 60 degree projection with identity reprojection
  (`neutral_camera.h`). A ScriptHook pose path exists but is not the default. Its notes say the
  script-thread pose could not be associated with the rendered frame.
- HUD: snapshots the swapchain image at the first `LOAD`-op render pass covering the swapchain
  extent after DLSS ran (`native_hud.cpp`). The maintainer notes say absent captures do not prove
  protection.
- Requires in-game DLSS. FSR3 is disabled; AMD is refused. Cutscenes that stop DLSS suspend
  generation; no fix is claimed.
- Streamline 2.14.1's Vulkan wrapper duplicates a queue family when the game exhausts it.
  Vulkan-only and not relevant to DX12.

## Reference: OptiScaler

Checkout `92337ebfbf07d533eabe3731fd8de1bb19437aea`. `misc/Quirks.h` disables FSR3 inputs and
DXGI spoofing for `rdr2.exe`: "Spoofing causes FSR2 inputs crash, DLSS inputs need OptiPatcher to
avoid artifacts/crashes anyway". The
[wiki page](https://github.com/optiscaler/OptiScaler/wiki/Red-Dead-Redemption-II) (0.9.4)
recommends DX12 with FSR2 inputs, says Vulkan is problematic and OptiFG is DX12-only. Reported
issues: its HUD heuristic flickers; RTSS needs Detours hooking to avoid a startup crash;
ScriptHookRDR2 V2's overlay can crash with XeFG. These are user reports, not measurements here.

## Reference: RenoDX rdr2vk (inferred UI boundary)

Checkout `da608c4d4feaa41b261e252440a73337613c2705`, `src/games/rdr2vk`. The "output" shaders are
UI element draws: vertex colour multiplied by an atlas texture, then the output 3D LUT and, in
HDR, a BT.709 to BT.2020 PQ encode, per draw. `output_main_0xA383C448` is a separate scene output
pass.

Inferred: UI is drawn straight into the swapchain image in output colour space, with no separate
UI texture. If DX12 matches, HUD-less colour is the backbuffer after the final output pass, and a
UI-only layer means redirecting UI draws into an owned premultiplied target. The hashes are SPIR-V
and do not identify DX12 shaders. Compositing in PQ space with the game's blend states is an HDR
risk to measure.

## What full FG needs from the game

| Need | Planned source | Status |
| --- | --- | --- |
| DX12 swapchain ownership before first use | DXGI factory/swapchain creation hooks | creation call site and DIRECT queue measured; FG ownership unimplemented |
| HUD-less colour, backbuffer format and colour space | backbuffer after final output pass | inferred |
| UI-only premultiplied layer | redirected UI draws | unknown |
| Depth, motion, jitter, MV scale, reset | NGX D3D12 or FSR2 dispatch parameters | call sites, resource descriptions and scalars measured; texel contents unmeasured |
| Motion coverage (sky, clouds, water, foliage, cloth, horses, particles) | captures | unknown |
| View/projection, previous, near/far, FOV, camera basis | FSR2 near/far/FOV; render-thread constants, checked against ScriptHook | near/far/FOV located; matrices unknown |
| One frame ID across simulation, render and Present | game Reflex marker frame IDs | markers and IDs observed; RSF frame association unimplemented |
| Latency markers and sleep point | native Reflex marker wrapper and Sleep caller | observed in the gameplay census |
| Eligibility (menus, map, pause, loading, Bink video, fades, photo mode) | ScriptHook natives, Bink hooks, render signals | unknown |
| Resource lifetime and queue use | captures | unknown |
| Pacing, VSync, triple buffering, RTSS | settings and present path | unknown |

## Open research, in order

1. Done: runtime dump of the decrypted image, analysed below.
2. Load timing done (before `WinMain`). Coexistence with Ultimate ASI Loader, RTSS in both hook
   modes and the Vulkan FG mod (refused) remains open.
3. Selection path and guarded forcing tested; census directly observed D3D12 device creation.
4. NGX D3D12 and FSR2 DX12 inputs across scenes: formats, flags, MV scale, jitter sequence,
   exposure, reactive and transparency masks, reset frequency, position relative to tonemapping.
5. Motion coverage in stationary, panning, riding and cut shots.
6. Render-thread camera matrices, validated against ScriptHook values.
7. DX12 HUD boundary and UI blend states, in SDR and HDR, across the map, weapon wheel, Dead Eye,
   photo mode, cinematic camera, letterboxed cutscenes and subtitles.
8. Marker call sites and IDs found statically; their threads, ordering per frame and behaviour on
   non-NVIDIA GPUs need the probe.
9. Eligibility signals with and without ScriptHook; Online detection.
10. Depth/motion reuse within a frame, async compute, VSync and frame cap behaviour, resize,
    fullscreen and HDR toggles.

## Runtime dump and static analysis (25 October)

Question: what does the decrypted image say about API selection, the upscaler calls, latency
markers, presentation and frame identity, before any hook runs?

Method: the ASI entry gained an opt-in dump (`[RDR2] DumpModule=1`) using the existing
`rsf_dump_when_decrypted` helper, plus module-list samples at 5, 15, 30, 60 and 120 seconds. The
user ran the game once to the story menu with Alexander Blade's ASI loader (`dinput8.dll` 1.0.0.2)
and ScriptHookRDR2 1.0.1491.17 installed. The image was analysed in IDA 9.3 through REA, with byte
reads, disassembly (capstone 5.0.7) and `.pdata` lookups done by local scripts on the dump file.
Addresses below are runtime addresses at load base `0x7ff7b9de0000`; RVAs are listed in
`.local/rdr2-tools/names.tsv`. They hold for the fingerprinted build only.

### Run evidence (measured)

- `rdr2.log`: code entropy 6.523 when the ASI loaded, so the image was already decrypted. The dump
  wrote 121,189,624 bytes, 11 sections, 502 imports and found 2,816 import-table call/jump
  references. Plugin prepare returned `RSF_ERROR_NOT_READY` as designed.
- The import slot for `KERNEL32!GetSystemTimeAsFileTime` resolves into the loader's `DINPUT8.dll`.
  The loader hooks that CRT-startup call to load ASI modules, so they run before `WinMain` and
  after the protector decrypted the code. Guarded writes can land before settings load and before
  device creation.
- Module samples: `vulkan-1.dll` at 5 s; the NVIDIA Vulkan driver and the Social Club, Steam and
  RTSS Vulkan layers at 15 s; NGX and `nvngx_dlss.dll` by 30 s. The renderer was created after the
  ASI loaded. `d3d12.dll`/`dxgi.dll` in the list come from the runtime DLL's static imports and say
  nothing about the game. The machine is a hybrid Intel/NVIDIA laptop (`igvk64.dll` present).
- The first `system.xml` records `<API>kSettingAPI_Vulkan</API>`, DLSS on and
  `kSettingReflex_On_Boost`.

### API selection and DX12 forcing

- `rage::fwSettings::eAPILevel` (parser enum at `0x7ff7bdac7650`, case-sensitive joaat name
  hashes) maps DX11=0, DX12=1, Vulkan=2.
- `rdr2_Settings_ApplyApiLevel` (`0x7ff7bca462e0`) maps the saved level to an adapter type
  (DX12 to 1, Vulkan to 3) and calls `rdr2_AdapterManager_SetApi`, but only when the string global
  `g_rdr2_ApiOverrideParam` (`0x7ff7bf58bf10`, null in this run) is unset.
- `rdr2_Settings_LoadFromXml` (`0x7ff7bca42fa0`) applies the same global after parsing: `"d3d12"`
  sets the settings API to 1 and `"vulkan"` to 2, unless `rdr2_IsWin7D3D12Path` is true.
- `rdr2_grcAdapterManager_Init_SelectApi` (`0x7ff7bc498e90`) reads the global (default
  `"detect"`) and enumerates D3D12 adapters for `"d3d12"` and Vulkan adapters for `"vulkan"`.
- `rdr2_DeviceSettings_ResolveAdapter` (`0x7ff7ba00a208`, site `0x7ff7ba00a60b`) switches a D3D12
  choice to Vulkan only when the Windows 7 D3D12 path is active (`12on7\D3D12.DLL` loaded or an OS
  below Windows 8) and the global is not `"d3d12"`.
- `rdr2_grcAdapterManager_EnumerateD3D12` (`0x7ff7bc470d94`) loads D3D12 and DXGI with
  `LoadLibraryA`, creates the factory through `CreateDXGIFactory2`, skips Microsoft's software
  adapter (vendor 0x1414, device 0x8C) and test-creates a device at feature level 11_0.

Inferred: pointing `g_rdr2_ApiOverrideParam` at a static `"d3d12"` string before settings load
uses the engine's own override and forces D3D12 through settings, adapter enumeration and device
resolution, without a code patch. The parameter's command-line name is not recoverable because
RAGE parameter names are hashed in this build. The write must be guarded by the fingerprint, the
section bounds and an expected null value, and announced. The later forcing runs below tested it.

### Upscaler calls

- FSR2: `rdr2_Fsr2Api_Dispatch` (`0x7ff7bc4cfe24`, from `rage/base/src/grcore/fsr2api.cpp`)
  builds an FSR 2.1.x dispatch description on the game's own command list
  (`ffxGetCommandListDX12` on D3D12) and calls the `ffxFsr2ContextDispatch` import. Resource
  order in the game's parameter block: colour, depth, motion vectors, output (as UAV), exposure,
  reactive, transparency-and-composition. Scalars: jitter, MV scale, render size, sharpening,
  sharpness, frame-time delta, pre-exposure, reset, camera near, camera far and vertical FOV. It is
  reached only through a thunk (`0x7ff7ba59a10b`) that checks colour and depth and may fill
  exposure. No static pointer, RVA or direct call to that thunk exists, so its caller needs a
  runtime stack.
- DLSS: the NGX SDK is statically linked (IDA recognised its functions).
  `ngx_D3D12_EvaluateDlss_Helper` (`0x7ff7bc4d3a3c`) sets every evaluate key, including
  TransparencyMask, ExposureTexture, `DLSS.Input.Bias.Current.Color.Mask`, the GBuffer slots,
  MotionVectors3D, IsParticleMask and AnimatedTextureMask, then calls
  `NVSDK_NGX_D3D12_EvaluateFeature_C` (`0x7ff7bcc7ce00`). It has one caller (`0x7ff7bc4d3f10`); the
  Vulkan helper (`0x7ff7bc4d46f8`) has one caller (`0x7ff7bc4d41f0`). Which optional slots the game
  fills was initially unmeasured; the gameplay census below records sampled optional resources.
- Render target names in `.rdata` include `DLSS Display Res Singlesample HDR Backbuffer 0/1`,
  `FSR2 Display Res Singlesample HDR Backbuffer 0/1`, `FSR2 Reactive Mask`, `DLSS VfxWeather Mask`,
  `DLSS Exposure`, `Working Singlesample HDR Backbuffer Pair`, `Working PostHud Backbuffer Pair`,
  `BackBufferRT for Pause`, `Velocity ResolvedClosest` and `GBUFFER_VELOCITY`, plus the colour
  stages `ColorLUT - POSTFX POSTHUD` and `LUTMask - PostHUD Stage`.

Inferred: the game produces its own FSR2 reactive mask and probably a DLSS transparency or bias
mask (VfxWeather), candidates for every provider's optional inputs. A post-HUD colour stage exists;
whether it alters the scene outside special effects decides whether a pre-HUD capture matches the
backbuffer's grading.

### Latency markers and frame identity

- NVAPI is statically linked as lazy stubs that resolve `nvapi_QueryInterface` once and cache the
  pointer (`SetSleepMode` at `0x7ff7bf922478`, `Sleep` at `0x7ff7bf922480`, `SetLatencyMarker` at
  `0x7ff7bf922490`).
- `rdr2_Reflex_SetLatencyMarker(type, frameId)` (`0x7ff7bc4b4674`) is the only caller of the marker
  stub. It returns early unless `g_rdr2_ReflexMarkersEnabled` (`0x7ff7bf4fbcc7`) is set, builds
  `NV_LATENCY_MARKER_PARAMS` (version `0x10058`) for D3D12 or calls `NvLL_VK_SetLatencyMarker` for
  Vulkan, and also emits the ReflexStats (PCL stats) ETW event.
- Every marker goes through it: SIMULATION_START (`0x7ff7b9f4545e`, `0x7ff7bc98b825`),
  SIMULATION_END (`0x7ff7bac6da77`), RENDERSUBMIT_START (`0x7ff7ba87f183`), INPUT_SAMPLE,
  TRIGGER_FLASH and PC_LATENCY_PING (`0x7ff7ba89cfd4`), OUT_OF_BAND_RENDERSUBMIT_START
  (`0x7ff7bc4bf43f`), and RENDERSUBMIT_END, PRESENT_START and PRESENT_END in both present paths.
- The simulation markers read their frame ID from one global, `g_rdr2_FrameCounter`
  (`0x7ff7bf20904c`); the present paths carry a copied ID in their frame record.
- Sleep has one caller (`0x7ff7bc467fb4`) and SetSleepMode one (`0x7ff7bc4b575c`).

Inferred: hooking the wrapper's entry yields every marker with the engine's own frame ID on any
GPU, because callers invoke it unconditionally and the enabled check is inside. One source can then
drive Streamline PCL/Reflex, XeLL and Anti-Lag 2, and the IDs link simulation, render submission
and Present.

### Presentation

- `rdr2_Present_D3D12` (`0x7ff7bc4bf760`) calls `IDXGISwapChain::Present` through vtable slot
  `+0x40` at `0x7ff7bc4bf94c` on the swapchain held in `g_rdr2_DxgiSwapChain` (`0x7ff7bf4fc7e8`).
  The sync interval comes from the frame record and the frame ID from record `+0x10`.
- `rdr2_Present_Vulkan` (`0x7ff7bc4bf458`) calls `vkQueuePresentKHR` through a pointer.
- RENDERSUBMIT_END is emitted after PRESENT_END in both paths.

### Camera (open)

The FSR2 path supplies near, far and vertical FOV. Shader variable names (`ProjParams`,
`NearFarPlane_ProjParams`, `prevViewProj`, `MBPrevViewProjMatrixX/Y/W`, `WorldViewProj`) mark where
the render thread uploads matrices. The view and projection producer is not located yet. The
planned route is the probe's call stacks at the upscaler call plus the motion-blur constants.

### Reference research summary

A delegated source review on 25 October found:
- OptiScaler (`92337ebf`) hooks only `ffx_fsr2_api_x64.dll` exports for RDR2 FSR2 on DX12, compiles
  against the FSR 2.1.2 layout and has no RDR2 code beyond the quirk entry. Its HUD fix copies the
  Nth swapchain-sized resource seen after the upscaler; its Reflex path wraps
  `nvapi_QueryInterface`.
- ScriptHookRDR2 V2 (kepmehz, Nexus 1472) is a closed alternative implementation of Blade's API;
  support for 1491.50 is unconfirmed. ScriptHookRDR2DotNet-V2 (`03f2264d`, zlib) requires Blade's
  hook and .NET 4.8.
- `alloc8or/rdr3-nativedb-data` (`e8558ab9`) provides pause, loading, fade, UI app, cinematic,
  network and final-rendered-camera natives. There are no near/far clip getters and no direct
  photo-mode or cutscene-active native; cutscenes are anim scenes.
- PCGamingWiki and Nexus refused automated access, so forum renderer facts remain unconfirmed.

### DX12 forcing run (25 October, measured)

The ASI wrote `"d3d12"` to the API parameter after checking the build and all four read sites
(`ForceD3D12 applied` in `rdr2.log`). The game started twice. The first process ended before its
120-second module sample; the second ran through it. The second run wrote
`sga_win32_60_final_init.d3d12PipelineCacheWindows` (53.9 MB) and `system.xml` now stores
`kSettingAPI_DX12`. The user confirmed the run. Inferred: the engine applied D3D12 and restarted
once, as it does after an API change. `vulkan-1.dll`, `nvoglv64.dll` and the Vulkan layers still
load in D3D12 mode, so their presence does not identify the active API; the pipeline cache and
saved setting do. The probe's device-creation records will confirm it directly.

The dump v2 sidecar from this run lists 33 threads, 124 RTTI classes and the `.pdata` function
table. RAGE is built without RTTI: every class belongs to the C++ runtime, Concurrency or
crashpad. The dump runs about one second after load, before the engine names its threads; most
engine threads start at the RAGE thread entry `0x25a7418` (RVA). Thread roles therefore come from
the probe's per-call thread records, not from the dump.

### DX12 ERR_GFX_D3D_DEFERRED_MEM crashes (25 October, measured)

Question: eight launches on forced DX12 died at the landing page with `ERR_GFX_D3D_DEFERRED_MEM`;
which component caused it?

Method: Rockstar's crashpad minidumps unwound with `cdb`; the vendor census with the D3D12
debug layer (`CensusFlags=8`, then `16` for debug layer only); module samples at 5/15/30/60 s;
then scripted launches through Steam (`.local/rdr2-tools/run-game.ps1`) with one variable
changed per run.

- Every dump has the same faulting path: a RAGE worker calls `ID3D12CommandAllocator::Reset`
  then `ID3D12GraphicsCommandList::Reset` on its lazily created per-thread list
  (`0x7ff7bc464414`, vtable slots 8 and 10); the list reset fails with a code that is not a DXGI
  device-removed code, and RAGE quits with error ids `0x976d3d1`/`0xb0c`, labelled "deferred
  memory". VRAM in use was 1.7 GB. The label is RAGE's name for a failed command list reset.
- The crash is deterministic at game time 31 to 39 s, frame 372 to 425, when the landing page
  first draws. The D3D12 debug layer recorded no message at the failing reset; it recorded
  error 904 (one command list writing several swapchain buffers) from RivaTuner's D3D11-on-12
  overlay and warnings from the game.
- Bisection, one launch each: census with all hooks (crash); debug layer only (crash); census
  off (crash); ASI disabled with the saved `kSettingAPI_DX12` (crash); the pre-run `system.xml`
  with DX12 (crash); **the three `sga_win32_60_final_init.*` D3D12 cache files moved out of
  Settings (175 s, no crash)**; the files the game then rewrote left in place (crash); only the
  `pipelineMarkerFile` removed, warm-up cache kept (crash). The pipeline library
  (`d3d12PipelineCacheWindows`) is deleted by the game itself after a crash, so the last two
  runs suggested a cache-dependent trigger but did not establish the cause (see correction below).
- Earlier suspects were wrong and are recorded as such: a 10 GB headless Ghidra job, VRAM, the
  census hooks, the Social Club and RivaTuner overlays. These exclusions were premature: surviving
  runs with overlays present do not exclude an intermittent frame-index mismatch. The later
  unfiltered error stack identifies the Social Club recording path; see below.

Correction after handoff: the warm-up-only diagnosis and the claim that unmodified DX12 always
crashes on this machine are withdrawn. Later run N crashed with the ASI absent and only a marker
remaining, while run O with both loader and ScriptHook removed was reported to run. Run P crashed
with the loader and ASI but without ScriptHook. The original successful DX12 session's cache was
not preserved, and the launch helper forcibly ended surviving runs unless `KeepRunning` was set.
These observations do not isolate cache content, clean shutdown, loader behavior or early runtime
imports. RSF poisoning the cache remains the user's working diagnosis to investigate, not a
disproved hypothesis. The cause and exact failing HRESULT remain open.

The rejected quarantine was removed from the ASI source: it no longer enumerates, deletes or moves
the game's cache files and no quarantine INI key is read. Existing files are left untouched. A
successful first launch is insufficient acceptance; repeated launches must reuse caches produced
with RSF loaded and RTSS active.

An opt-in `[RDR2] TraceD3D12=1` observes the game's HRESULT helper at RVA `0x268b5c0`. The first
16 expected bytes are `40 53 48 83 ec 20 8d 81 fb ff 85 77 8b d9 83 f8`, checked only after the
existing exact-build ForceD3D12 guards succeed. MinHook forwards to the unchanged helper; failed
results are appended to `ReScaleFrame/rdr2-d3d12-errors.log` with thread, tick, caller RVA and stack.
The helper preserves its input HRESULT and invokes the separate device-removed handler for
`0x887a0005..7` or `0x887a0020`. Both allocator Reset and list Reset call it at return RVAs
`0x2684499` and `0x26844af` respectively (helper return sites `0x26844a0` and `0x26844b6`).
This diagnostic observes failures without changing a device, command list, allocator or cache.
Static evidence: decrypted dump SHA256 `496fa1bd3cdfb9ba77841f17a3242a554e929bb704da1f6b309e94b39281b7cb`,
load base `0x7ff7b9de0000`, Capstone disassembly of the two functions.

Run Q (22:31:56 launch, no quarantine, census off, trace on) reproduced the crash at game time
31 s/frame 425. ASI SHA256 `f66258a06f720947a3ea955807ec6e539a8b7f473365922e41ab634c28002e79`.
On thread 68304, the first observed failure was **`Close` returning `E_INVALIDARG` (`0x80070057`)**,
logged at tick 280970906/helper return RVA `0x268c9c9`. Its recording owner calls `Close` at
`0x268c99d`, vtable `+0x48`. It ignores that non-device-removed error after marking the list as
discarded. At tick 280979750 the same thread failed `Reset`, helper return `0x26844b6`, with the
same HRESULT. No failing allocator Reset was logged. The marker was already present; no D3D12
pipeline-library or warm-up file was present or manipulated. Evidence is retained under
`.local/rdr2-tools/run-q-trace/`. This moves the investigation to the original invalid recording.
[Microsoft's Reset contract](https://learn.microsoft.com/en-us/windows/win32/api/d3d12/nf-d3d12-id3d12graphicscommandlist-reset)
states that a failed Close permanently prevents Reset and repeats its error.

The previous 16-frame error-904 stacks ended inside D3D12 and did not establish an RTSS caller.
Census diagnostics now retain 32 frames, reserve separate output capacity for them, and use
`D3D12_MESSAGE_CALLBACK_IGNORE_FILTERS` to observe messages hidden by application filters without
changing those filters. Each message records its device. The first failing recording's cause is
identified by the subsequent run below; the callback change is diagnostic, not a crash fix.

Run S (22:38:53 launch, census debug-only plus trace) reused run R's library and warm-up cache
after its clean exit (launcher exit code 0 at 22:37:56). R's census had failed to install because
the trace initialized the shared MinHook instance first; installation order was corrected before S.
R's cache hashes are `667cb12607cf737d590d05ead556f516e944baefde1da500f02d7beda2db0105`
(library, 53,935,416 bytes) and `7d1edbecade3b0348999446a68a780789200a51b7567beda5b9724164d08cfdc`
(warm-up, 245,043 bytes), preserved as evidence copies without changing the originals.

S identified the original invalid recording: debug error 904 at 22.003691 s/thread 100296 came
from **`SocialClubD3D12Renderer.dll+0x4d0e`**, through `socialclub.dll+0xd93f4`,
`socialclub.dll+0x13b747` and `RDR2.exe+0x27f0a2b`. It was a ResourceBarrier call at renderer RVA
`0x4d08`, vtable offset `0xd0`. Close then returned `E_INVALIDARG` on the same thread, followed by
Reset nine seconds later. The game quit at game time 30 s/frame 426. Evidence:
`.local/rdr2-tools/run-s-debug/`. RTSS was active; the invalid-recording stack does not contain it.

### Social Club frame ownership correction

The smaller overlay binaries were decompiled with REA/IDA after the game exited. Renderer version
2.4.0.240, SHA256 `974222795a2198b1b979c57257401a3ef967fb96a7e4f011eae02c1786819acd`, PE timestamp
`0x6a86d560`, image size `0x2e000`; `socialclub.dll` SHA256
`0bb2286df66ae4044437451638c2b4a692c7864e95e338be32638c6586ae1cdd`.

Renderer function RVA `0x4b70` takes an optional external command list as argument 4 and a buffer
index as argument 5. With object byte `+0x40c` set, it queries `IDXGISwapChain3::GetCurrentBackBufferIndex`
instead of using argument 5. That same index selects its backbuffer resources, RTV and fences.
With an external list it does not reset, close or submit that list. The caller at socialclub RVA
`0xd9390` obtains the list and index from adjacent game callbacks (vtable offsets `0x50`, `0x58`).
This is a recording-frame versus presentation-index mismatch candidate, not cache corruption.
Evidence IDs: `ev_e09801b098e0ee41409517642b205db40ab2c002cab552be4e5e09369834c4c0`,
`ev_c41e10443873dc9861fb164505c7bf98fce11fba778a1b75688343fdd0d38cc9`,
`ev_2ce105616e27cc1731538dab19f524d3dfb21b9673dbce02eec4ab3fc0477d7a`.
Retained REA bundles: `.local/rdr2-tools/socialclub-{renderer,caller}-evidence.json`.

The candidate repair hooks RDR2's Social Club render dispatcher at RVA `0x27f09e8` after validating
bytes `40 53 48 83 ec 30 48 83 79 08 00 48 8b d9 75 29`. Before dispatch, it pins the already-loaded
renderer, validates its PE identity and the bytes at `0x4b70` and `0x4b89`, then patches the seven-byte
comparison `80 b9 0c 04 00 00 00` at `0x4b89` with a MinHook detour. The gate preserves all registers
and the stack: for non-null R9 (external list) it sets ZF and resumes at `0x4b90`, so the existing
branch selects argument 5. For null R9 the gate executes the original comparison and resumes at
the same address. The complete gate is made executable before registering the hook, so concurrent
census hook enables cannot enter a partially initialized gate.
The overlay's owned-list path keeps its original selection behavior. Module and trampolines remain
owned until process exit. With `TraceD3D12`, an entry trace records supplied/current indices and
mismatches. No cache or overlay setting is changed. Runtime validation of this candidate follows.

Run T (22:47:11 launch), ASI SHA256
`e94aab346b33f929d4d71db0ad94ec86b4029a4cb02e6faa07ce358a8344b616`, confirmed the mismatch in-game:
overlay call 124, tick 281884546, external list `0x2ae3c3c2b60`, game-supplied index **1** versus
DXGI current index **0**. Both the dispatcher and selector patch reported `MH_OK`. The corrected
recording used index 1. At 74 s process time, the unfiltered debug callback had recorded 124
warnings and **zero errors**; the HRESULT trace had no new failures. This is direct evidence of
the frame-ownership defect, rather than evidence that the on-disk library is corrupt. Long-run
and repeated-cache acceptance are recorded separately below.

At 22:54:27, run T's census had reached 423,147 ms with zero errors and zero dropped records;
the HRESULT log contained no failures from this run. RTSS and SocialClubD3D12Renderer remained
loaded. New cache evidence copies are under `run-t-fix/cache-evidence`: library SHA256
`05f540e16436a1e76108c7976ab05c0552b91e2f942fd50b1c03288f5af3d016`, warm-up SHA256
`6a47b075a5bf0649568bcdddc59ddd8537c00bf41d7c853b82daf614b9a453bf`.
No original cache was moved, invalidated, restored or deleted by this investigation.

The final gate-publication correction passed `eng/verify.ps1 -Configuration Release -VS2026`
(57 passed, eight opt-in skips, formatting and Clippy). Built ASI SHA256
`a5487cf002b2fcac9bf95f1ecf4f177273ed8b4d06145141da06b76ff160c38e`; it is not deployed yet:
run T still owns the earlier `e94aab...` ASI. A normal window-close request at 22:53:02 was accepted,
but PID 108472 was still alive at 22:54:27. Do not replace its files or force-stop this cache-producing
run. The next steps are clean exit, hash-verified final deployment, repeated launches reusing these
caches with the debug layer off, then full census in Story mode with DLSS and FSR2. Those acceptance
steps remain open; the first stable run alone does not close the task. All source remains uncommitted
in the inherited tree, based on HEAD `0063819c639da5d2fac1bda0d36518852780bb1f` plus existing work.

### Final deployment and gameplay census, 25 October

After the user exited normally (launcher code 0 at 23:22:38), the final ASI `a5487cf0...` was
deployed with a matching SHA256. The prior ASI, INI and logs were backed up under
`.local/deploy-backups/rdr2-20261025-232311-full-census/`. The library and warm-up files at launch
were read and copied for evidence only, not replaced: hashes `4cbbe72c5c9e24fe535f9e62d128605ecd5b9099a741e03be6d71dbd8e7ce941`
and `743ced97bd8a09a6dd2062dcc334b993449c9ed009016f75f3f277d4b339d284` respectively.

Run U started at 23:23:32, PID 101236, forced DX12, full census (`CensusFlags=0`, no debug layer),
HRESULT trace on, RTSS in Detours mode. The user rendered Story mode with DLSS and FSR2 and
confirmed both completed, then exited normally at 23:27:30 (launcher code 0). The patch corrected
two observed index mismatches during startup. No new failing HRESULT or crash dump was recorded.
The swapchain-creation stack includes both Steam and RTSS, and the queue submission records
also include RTSS. This is a real game session with the previously written caches retained.

Evidence: `.local/rdr2-tools/run-u-census-20261025-232311/`, including `census/census.jsonl`, its
summary, launch/exit cache copies, INI and logs. The last periodic summary covers 223,736 ms,
13,267 Presents, 4,900 NGX evaluations, 4,412 FSR2 dispatches and 83,549 Reflex marker calls.
It reports 1,135 full/sample records, zero queue/budget/size drops, 122 sites and 87 refused
second-implementation bindings. No installed detour reports overwritten entry bytes. Those
binding conflicts bound coverage; they are not 87 crashes or proof of complete vendor coverage.
Full records are sampled after the configured per-site cap, so counts exceed retained argument
samples. The last periodic summary is not an exact process-exit accounting.

| Observed operation | Game call site / stack | Evidence |
| --- | --- | --- |
| D3D12 direct queue creation | `RDR2+0x26beb46` | priority 0, node mask 1; queue also supplied to swapchain creation |
| CreateSwapChainForHwnd | `RDR2+0x26bec9d`, through RTSS and Steam | 2560x1600, B8G8R8A8_UNORM, three buffers, swap effect 4, flags 0x2 |
| Present | `RDR2+0x26df94f`, through Steam | interval 1, flags 0, colour space 0 |
| NGX CreateFeature | `RDR2+0x2e9cd0e`, game wrapper `+0x26f391e` | SuperSampling, MaxQuality, flags 0x2b; two creations and one release in the summary |
| NGX EvaluateFeature | `RDR2+0x2e9c900`, `+0x2e9ce30`, game wrapper `+0x26f4199` | 4,900 evaluations, sampled return 1 (NGX success) |
| FSR2 ContextCreate / Dispatch | `RDR2+0x26f1c74` / `+0x26f050a` | context flags 0x29, layout 2.1.2 (1,352-byte dispatch description), sampled return 0 |
| Reflex markers | `RDR2+0x1ea2c`, wrapper `+0x26d46d6` | version 0x10058, game frame IDs; simulation, render-submit, present, input and out-of-band markers observed |
| Reflex Sleep / SetSleepMode | `RDR2+0x1e963` via `+0x2687feb`; `+0x1e88c` via `+0x26d57cd` | 10,847 sleeps and 17 mode calls; no latency reduction measured |

The DIRECT queue `0x28c7bfba740` is the swapchain queue in this session. A COPY queue
`0x28c7bfba9c0` has one recorded submission per Present; a COMPUTE queue was created but has no
recorded submissions. Immediate callers can be overlay trampolines, so the full stack must be
used to identify the game producer. This census does not establish every resource's queue lifetime.

| Input | DLSS samples | FSR2 samples |
| --- | --- | --- |
| Render / output extent | MaxQuality 1707x1067, later Balanced 1485x928 / 2560x1600 | 1706x1066 / 2560x1600 |
| Colour and output | R16G16B16A16_FLOAT | R16G16B16A16_FLOAT |
| Depth resource | R32G8X24_TYPELESS, inverted-depth create flag | same, inverted-depth create flag |
| Motion | R16G16_FLOAT, MV scale 1/1, low-resolution create flag | R16G16_FLOAT, MV scale 1/1 |
| Exposure | 1x1 R32_FLOAT texture, pre-exposure 1 | null exposure texture, AutoExposure create flag, pre-exposure 1 |
| Optional masks | R8_UNORM bias-current-colour mask; null transparency mask in the first sample | R8_UNORM reactive mask present in 4 of 40 retained dispatch samples; transparency/composition null in all 40 |
| Camera / timing | NGX FrameTimeDeltaInMsec was 0 in the first sample | near 0.14999111, far 10000, vertical FOV 0.659771383 radians; first frame delta 15.1471996 ms |

Both first evaluation samples had jitter `(0.25, -0.388888896)` and reset false. No FSR2 reset was
true in the 40 retained dispatch samples; that does not prove the game never emits one. These are
resource descriptions and scalar contracts, not captured texels or motion-coverage/colour-space
proof. The one-pixel render-size difference between providers must be preserved. Resource pointers
are session-local and borrowed only during the intercepted call. The decoder labels NVAPI layouts
assumed. Source cross-check against the NVAPI headers in Luma-Framework revision
`5d7ee5470d8f09e27dfe529a386af46d8c6f8e33` confirms its decoded offsets: NvBool is one byte;
SleepMode mode/boost are at 4/5, minimumIntervalUs at 8 and markers at 12; marker frameID is at 8
and markerType at 16. The sample versions are 0x1002c and 0x10058 respectively. This is header/layout
validation, not a measured latency benefit.

Run V started at 23:29:13, PID 82280, using the same final ASI with `Census=0`, `TraceD3D12=0`
and `ForceD3D12=1`. It retained U's cache files after U's clean exit: library SHA256
`4e205afa055c54bfcd8ce1a161c96ba91e0b418e635d98d25305e5f422e0d9c8`, warm-up SHA256
`8c850fda272dfecaf1541750201650d9c3601f5380064b4b1d8a7ce846a006da`. The first launch request arrived
during launcher teardown and started no game; one later request started this process. The guarded
Social Club correction reported `MH_OK`; its 60-second module sample includes RTSS and Social
Club. The game exited cleanly at 23:31:13 (launcher code 0), about 120 seconds after launch,
without a new crash dump. Evidence:
`.local/rdr2-tools/run-v-reuse-20261025-232735/`. This closes the requested startup/cache-coexistence
and census scope for the measured build and machine. It does not claim full SR/FG integration or
indefinite gameplay stability. The game is closed and no source was committed.

### SR/FG implementation sequence informed by the census

1. Implement the game plugin's NGX and FSR2 input adapters at the observed wrappers. Use actual
   per-call extents, resource states and optional-mask availability; do not infer them from quality
   labels. Reject Online before activation.
2. Execute shared DX12 SR on the leased game command list and reinsert its output at the original
   upscaler boundary. Recreate backend state on provider, dimensions and context changes; a new
   context requires history reset even when the first game dispatch says reset false.
3. Locate the render-thread camera/motion producers and capture moving scenes. FSR2 near/far/FOV
   supplements that work; it does not supply current/previous matrices or prove velocity coverage.
4. Find the completed HUD-less colour boundary and UI blend/resource ownership. The SR input is
   earlier in the pipeline and is not the FG colour input. Preserve Social Club's recording-frame
   buffer index and test the existing RTSS/Steam hook chain when adding shared FG presentation.
5. Bind the observed swapchain DIRECT queue to the orchestrator's presentation path, establish
   COPY/COMPUTE dependencies and fence retirement, then add DLSS/FSR/XeSS FG and runtime controls.
   The census supplies call sites; it does not yet establish all resource leases or queue ordering.
6. Hook the native Reflex wrapper before its enable predicate for cross-vendor CPU/frame identity,
   verify the parameter layouts, and route shared latency support. Validate runtime SR/FG/provider
   switches, resize/HDR, pause/loading/cutscenes, motion coverage and measured pacing/latency in game.

Tooling corrections made during this investigation: census records carry 16-frame call stacks;
optional D3D12 debug layer and DRED (`RSF_CENSUS_FLAG_D3D12_DEBUG`); a fatal-exception flush;
hook-family flags now also govern vtable detours; bisection flags for the loader detours;
`rsf_census_present_coexistence` reproduces the census next to RivaTuner on a real GPU without the
game (no errors with or without the census).

### Camera producer investigation, run W, 25 October

Question: which view belongs to the job that evaluates SR, and does its camera history agree
across workers? Following the postfx caller is preferable to sampling the update-thread camera:
the FSR2 branch itself reads the active worker's view when constructing its dispatch parameters.
The installed executable was rehashed and still matches `b56c9548...465e`.

Static disassembly of dump SHA256 `496fa1bd3cdfb9ba77841f17a3242a554e929bb704da1f6b309e94b39281b7cb`
is retained in `.local/rdr2-tools/camera-static-20261025.json`. Its dump base is
`0x7ff7b9de0000`; run W instead loaded at `0x7ff7bc8c0000`. Use RVAs, not those session addresses.
The postfx function starts at RVA `0x5c6e44`. Its calls return at `0x5c7d89` (DLSS)
and `0x5c8057` (FSR2). Both enter through three-argument game wrappers:

| Observation site | RVA | Expected entry bytes |
| --- | --- | --- |
| FSR2 evaluate thunk | `0x7ba10c` | `48 83 3a 00 4c 8b ca 74 27 48 83 7a 18 00 74 20` |
| DLSS evaluate thunk | `0x7bb60c` | `8b 81 a8 00 00 00 4c 8d 0c 80 4a 8d 0c c9 48 83` |

The earlier FSR thunk note's `0x7ba10b` was one byte before the actual instruction boundary.
`CameraProbe` validates the PE timestamp/image size and both entries before installing pass-through
hooks. It copies each callback's parameters and view, including caller RVA, thread, QPC and the
observed global simulation counter. That global counter alone is **not** a final Present lease.

The TLS index is the DWORD at RVA `0x5b403e0`. Read `GS:[0x58]`, select its indexed TLS block,
then read the view pointer at block `+0x14d8`. Setter RVA `0x1eb420` installs this pointer,
returns the prior binding and propagates reverse-depth/viewport state to the draw context.
At `0x5c7f64`, `0x5c7f7c` and `0x5c7f94`,
the postfx FSR2 branch loads near at view `+0x630`, far at `+0x634`, and vertical FOV in degrees
at `+0x640`; multiplying FOV by `0.01745329238474369` converts it to the API's radians.
The render job worker at `0x2d2687c` clears the active-view TLS slot before executing its job
at the call ending `0x2d26a07`. This is a job-local binding, not a single global camera pointer.

Run W used ASI SHA256 `1aa45643b804d3374b881208b04139d296bc081b2e3bbe0e4f6472bfe1ee3d07`,
forced DX12, census/debug layer off and RTSS active. The user entered Story mode after computer-use
input failed to activate Story. Two 64-frame captures are under
`.local/rdr2-tools/camera-run-w/`; `analysis.json` is generated by
`tools/analyze-rdr2-camera-probe.py`. Both captures are DLSS, not FSR2. The callback return RVA is
`0x5c7d89`; eight job threads use eight distinct view pointers. Frame counters increase by one
within each capture. No pointer is retained for later dereference by the probe.

| View offset | Interpretation and evidence |
| --- | --- |
| `0x40` | Current world-to-view, row-vector convention; paired with `0xc0` |
| `0x80` | Current perspective projection including jitter, negative camera Z forward, reversed depth |
| `0xc0` | Current camera-to-world; rows 0/1/2 are right/up/back, row 3 is position in game world units |
| `0x140`, `0x1c0` | Previous view and camera-to-world; previous view equals the preceding captured current view exactly |
| `0x180` | Projection using previous camera scalar parameters, but current jitter in W; do not mistake it for the preceding frame's jittered projection |
| `0x280` | View rotation with translation removed; distinct from the world-space view |
| `0x630`, `0x634`, `0x640`, `0x66c` | Near, far, vertical FOV degrees, aspect |
| `0x65c`, `0x660` | Base projection offsets; zero in W |
| `0x664`, `0x668` | Additional projection offsets; current jitter in W |
| `0x674` through `0x688` | Viewport x/y/width/height/min-depth/max-depth; W is 0/0/1485/928/0/1 |

W measures near `0.15000000596`, far `10000`, FOV `37.801094055` degrees /
`0.65975355212` radians, aspect `1.600000143`. These are fresh values, not a repetition of U's
FSR2 `0.14999111 / 10000 / 0.659771383`. For all 128 frames, the view-times-camera inverse maximum
absolute error is `0.000176135` (world translation is about 900 units); basis determinant is
approximately +1 and the rotation orthogonality error is below `2.3e-7`. Projection times an
**independently constructed analytic inverse** from near/far/FOV/aspect/offsets differs from
identity by at most `1.22445e-8`; FOV reconstructed from projection differs by `7.50491e-9` radians.
This inverse is derived, not a discovered stored engine inverse. An initial guess that `+0x520`
was inverse view-projection failed by more than 500,000 and was rejected; it must not be supplied
as an inverse matrix. Its derived-plane semantics are not yet fully named.

For the row-vector projection, `P[2][0] = baseX - 2*jitterX/renderWidth` and
`P[2][1] = baseY + 2*jitterY/renderHeight`. The pixel-jitter setter at RVA `0x26f7b1c` rounds
viewport width/height (`+0x67c`, `+0x680`) with `floor(value + 0.5)`, writes `+0x664/+0x668`,
then rebuilds projection at `0x26f63fc`. W's same-call viewport is 1485x928. The matrix offsets
agree with the upscale-call jitter across all 128 samples within `1.97616e-8` pixels, so this
dimension/jitter relationship is measured independently of the earlier census.
W capture 2 sample 1 contains the requested
jitter `(0.25, -0.3888888955)`. Current jitter must be removed from `+0x180` before applying an
independently retained preceding-frame jitter; its current jitter is not the preceding sample's.

Static producer sites: perspective builder `0x26f63fc` writes projection `+0x80` and `+0x180`,
derived-state builder `0x26f5e40`, current/previous camera setter `0x26f6dfc`, current camera setter
`0x26f6ff4`, world-transform setter `0x26f7eb4`, reverse-depth selector `0x26f7474` (view byte
`+0x671`). Their entry bytes and complete disassembly are in the static evidence JSON. Their
execution was not directly traced by W; the consumer snapshots and algebra validate the stated
field subset. The world-transform at `+0` and camera at `+0xc0` are different objects semantically.

Still open: direct FSR2 comparison, current/previous unjittered projection across
FOV/cut transitions, exact frame-to-Present association, motion-vector texel reprojection,
ScriptHook final-camera comparison, and the entire pixel-verified HUD boundary in SDR/HDR and
special modes. W did not capture depth/motion/colour texels or establish UI blend states. The
new optional `FrameProbe` uses the existing RenderDoc API wrapper for the next capture run;
its presence is not evidence of a successful or replayable frame capture.

### Public modder research cross-check, 26 October

The user pointed out that public RAGE research should be checked before continuing local
discovery. The initial pass had used the existing local mod references but had not searched
online for the viewport and HUD question. That omission is corrected here. Reference code was
read only; no mod binaries or replacement shaders were installed.

- Cfx/RedM revision `46de79bc1d4dd7c64f16c5cfbadf94612b5ab3d9`,
  [GamePrimitives.h](https://github.com/citizenfx/fivem/blob/46de79bc1d4dd7c64f16c5cfbadf94612b5ab3d9/code/components/extra-natives-rdr3/include/GamePrimitives.h)
  and [GraphicsNatives.cpp](https://github.com/citizenfx/fivem/blob/46de79bc1d4dd7c64f16c5cfbadf94612b5ab3d9/code/components/extra-natives-rdr3/src/GraphicsNatives.cpp),
  describes `grcViewport` with world, world-view, projection and inverse-view at 0/0x40/0x80/0xc0.
  The latter three agree with W at the upscale boundary where world is identity. Its `m_view`
  name at 0x140 must not override W's observed previous-frame equality. Its viewport-global
  signature matches once in this dump at RVA `0x1507b2a`, resolving global `0x3ec2610`.
  That global's relation to the queued job view remains unmeasured. RedM's separate
  [GET_CAM_MATRIX implementation](https://github.com/citizenfx/fivem/blob/46de79bc1d4dd7c64f16c5cfbadf94612b5ab3d9/code/components/extra-natives-rdr3/src/CameraNatives.cpp)
  selects script-camera matrices at +0x20/+0xf0 using a camera selector; it is a different owner.
- PotatoOfDoom/CyberFSR2 revision `ed00f4a0022e273e8e4e938b70781d481d3b868d`,
  [ViewMatrixHook.cpp](https://github.com/PotatoOfDoom/CyberFSR2/blob/ed00f4a0022e273e8e4e938b70781d481d3b868d/CyberFSR/ViewMatrixHook.cpp),
  provides an RDR2 scalar-camera signature. It matches once at `0x3212b7`, resolving `0x3ec2450`,
  with its three scalar fields at +0x60. It does not supply previous matrices or establish
  job-frame ownership. Local copies of these sources are in `references/rdr2-cfx-camera/` and
  `references/rdr2-cyberfsr-camera/`.
- ReneSchimmelpfennig's [ultrawide camera research](https://github.com/ReneSchimmelpfennig/rdr2-ultrawide-proper-black-bar-fix/tree/0a000d93f9dbb56633a34ffc64cbf1ed7750a2b4)
  explicitly targets 1.0.1491.50 and records camera-finalization, lens-clamp and 2D-layout
  investigations. Its signatures match this dump once each: camera-state application at
  `0x170028`, letterbox anchor store at `0x320545` targeting `0x39751b4`, and UI boxing at
  `0x1cbd30`. Its author reports that a convenient FOV getter feeds peripheral consumers, that
  many camera states are blended, and that script-drawn subtitles/credits use a separate layout
  path. These are useful producer and special-mode leads, not proof of the FG colour boundary.
  The pinned checkout is `references/rdr2-ultrawide-camera/`.
- The [DX12 RenoDX fork](https://github.com/jakeypiez/rdr2-renodx-dx12/tree/f20c3c3003c29fc17697a7ab2ddf1cdc9389b0b0)
  provides nine tonemap/LUT and ten output/PQ shader candidates, with bindings and reconstructed
  source. It explicitly reports no in-game testing. In the output family, `cb22[9].y` gates HDR,
  `cb22[10].x` gates a colour-space LUT in the textured variant, and bindings include `t32`,
  `t35`, `b22` and `b23`. This narrows shader identification; it does not establish which draws
  run in this build, their blend state, post-HUD effects or captured-pixel equivalence.
  The checkout and all shader candidate hashes are in `references/rdr2-renodx-dx12/`.

The exact public-pattern bytes and current-dump matches are retained in
`.local/rdr2-tools/public-camera-signature-matches.json`. All matches above are static evidence.
The [2020 RDR2 frame study](https://imgeself.github.io/posts/2020-06-19-graphics-study-rdr2/)
also reports RG16F velocity and reversed D32S8 depth, consistent with the census, but explicitly
does not cover the full post-processing chain. It cannot close HUD/POSTHUD acceptance.

### Capture preparation and outstanding blocker

The final diagnostic ASI was built and deployed with matching SHA256
`de3cdc73dd7c027009b2ea91d9ca7f02e192ecc1b0ee664b0d04e2fbe8a3ddd1`.
`eng/verify.ps1 -Configuration Release -VS2026` passed 57 tests with eight opt-in skips,
formatting and Clippy (`.local/rdr2-tools/capture-final-verify.log`). It adds version-2 raw
snapshots with render extent/job metadata, an optional ScriptHook reference fiber, and an
optional RenderDoc frame request. With `FrameProbe` enabled, hold F10 for one second in the
foreground game to request one GPU frame and 64 camera samples. All diagnostics default off.
ScriptHook export names and vector result stride were checked against the installed DLL and
the pinned ScriptHookRDRDotNet interop source; the reference callback has not run in-game yet.

The next capture configuration is saved in `.local/rdr2-tools/frame-run-x-20261026/`, but no
run-X process or `.rdc` capture was observed before returning the blocked result. Windows
computer-use input did not activate Story or pause in W, so the user supplied Story entry and
normal exit. Further camera/FSR2/reference validation and every HUD pixel/mode/HDR result remain
blocked on the requested first F10 capture. A configured capture tool is not a captured frame.

The original INI and graphics settings were restored byte-for-byte from
`.local/deploy-backups/rdr2-camera-20261025-2348/`; the game was closed during restoration.
The verified diagnostic-capable ASI remains installed with diagnostics off. Forced DX12 and the
Social Club selector correction are retained, RTSS settings were not changed, and no cache file
was manipulated. No source was committed. This camera/HUD investigation is **partial**, not FG
input acceptance or a completed HUD separation implementation.

### Run X: user captures and replay limits, 26 October

After restarting the agent with administrator rights, the user captured five frames and exited
normally. Run X, PID 116536, used diagnostic ASI `de3cdc73...`, forced DX12, RTSS, ScriptHook and
RenderDoc 1.46. Evidence is `.local/rdr2-tools/frame-run-x-20261026/`. The captured frames are
7695 (world), 8173 (Eagle Eye), 9323 (Dead Eye), 9691 (map) and 10784 (pause). Their embedded
thumbnails identify these states; weapon wheel, photo mode, cinematic camera, letterboxed
cutscenes and HDR are not established by these five files. The user subsequently clarified that
DLSS supplies every RSF backend; native FSR2 evaluation is not a validation requirement.

All 320 camera snapshots are from the DLSS thunk. `camera-analysis.json` measures exact
previous-view agreement, maximum view/inverse error `0.000186335`, projection/analytic-inverse
error `1.30246e-7`, and FOV error `9.32591e-8` radians. In captures 1-4, the 1706x1066 viewport
and resource extent agree, and projection jitter matches the DLSS parameters within `1.91e-8`
pixels. In pause, the view projection is frozen while DLSS continues advancing jitter; the error
reaches `0.84375` pixels. Do not treat a continuing DLSS call as proof that the view was updated.

`script-comparison.json` compares 192 samples against ScriptHook's final rendered camera. Equal
game-frame counters give maximum position error `5.0e-7` world units, FOV error `5.0e-8` degrees,
and forward-axis error `1.92e-7`. Offsetting the script frame by one introduces errors as large
as 0.278 world units and 0.104 in the forward vector. This validates the same-frame camera
association in those samples; it does not establish a GPU resource lease through Present.
No script reference samples were produced for map/pause.

The API capture opens and exposes shaders, PSOs, resources and draw order on the same NVIDIA
GPU. Actual replayed pixel inspection then revealed that the 3D scene becomes black while UI
draws reproduce. The earlier statement that the frame replayed successfully described API
opening only and is not pixel replay acceptance. The capture log explicitly warns about
unsupported NvAPI extensions. Exact cause within the vendor path is not isolated. Preserve
the captures for topology and shader evidence; do not use their black scene as HUD-less colour
or claim that unchanged black pixels prove that a post-HUD stage preserves the scene.

Measured DX12 examples in frame 8173:

| Stage | Event | PS CRC32 | Target / format |
| --- | --- | --- | --- |
| Tonemap/LUT | 74162 | `4FF4CC58` | Resource 50791, 2560x1600 R11G11B10_FLOAT |
| Last scene output, with further colour-processing code | 74185 | `DBCF46F8` | Backbuffer 391, B8G8R8A8_UNORM |
| Offscreen minimap drawing | 74231-74304 | includes `69CF07C8`, `CA8E18BF` | Resource 50609, 512x512 RGBA8 |
| First backbuffer UI draw | 74313 | `CA8E18BF` | Backbuffer 391 |
| Later UI | through 74413 | `2E866023`, `EE341D7A`, `1A0D957F`, `CA8E18BF` | Backbuffer 391 |

The first UI draw uses RGB SrcAlpha/InvSrcAlpha and alpha One/InvSrcAlpha. Captured UI shaders
include optional source premultiplication, LUT and PQ branches; do not classify every variant
from one blend state. The normal world frame instead uses `1D1EEAC6` followed by simple blit
`BB74CEA4`. These identities match parts of the public DX12 shader list. PSO/resource state and
shader bytecode/disassembly are retained in `world-tail/` and `eagle-tail/`.

To obtain original scene pixels, an opt-in native probe now copies after draws targeting the
actual game swapchain buffers, restores render-target state, and waits for a fence signalled
after the game submits each copied command list. It retains at most 64 readbacks and releases
backbuffer references after retirement so diagnostic ownership does not block resize. The
first native launch (Y, PID 111620, ASI `cefaf132...`) refused census installation (`-6`) because
the new probe initialized MinHook before the census. That initialization was removed: the
census remains the owner. The corrected source passed the Release gate; live native pixel
capture remains pending. Computer-use disconnected during that launch and failed its bounded
retry/reset recovery. No cache was changed and no running game binary was replaced.

### Run AA and native SR adapter, 26 October

The preceding pending capture statements are historical. Run Z completed 12 SDR captures and
586 original draw readbacks, and the complete AA bundle subsequently captured 27 manifests:
five loading screens without temporal input, then 22 bundles with four exact-call camera,
colour, depth, motion, exposure and output sets each. Evidence is
`.local/rdr2-tools/native-run-aa-20261026/`: `bundle-analysis.json`,
`camera-adapter-validation.json`, `static-rock-reprojection.json`, `native-analysis.json`,
`draw-summary.txt`, all original binaries/metadata, and `contact-1.jpg` through `contact-3.jpg`.
AA ran PID 121448 with ASI SHA256
`10eb25faeecc5d6ecb8a104c3f758f0f5dc73a0b961f245cbad94a5267e56394`, forced DX12,
Social Club's selector correction and RTSS retained. Every captured draw now has a shader
identity, including PSOs created through pipeline streams. No game cache was manipulated.

The active worker view is copied through TLS index DWORD `+5b403e0`,
`GS:[58][index] +14d8`. Draw context is `+14f0` and job is `+1ce8`. All offsets below belong
to the matching executable fingerprint, not stock RAGE or another RDR2 build. Matrices are
row-vector, row-major, world Z-up, negative view Z forward. Camera-to-world rows contain
right/up/back and position. Positions and clip distances are game world units; no independent
physical metre calibration was performed. FOV at `+640` is degrees, converted to radians.

| View offset | Meaning |
| --- | --- |
| `+40`, `+80`, `+c0` | Current world-to-view, jittered projection, camera-to-world |
| `+140`, `+180`, `+1c0` | Previous view, previous scalar projection rebuilt with current jitter, previous camera-to-world |
| `+630`, `+634`, `+640`, `+66c` | Near, far, vertical FOV in degrees, aspect |
| `+65c/+660`, `+664/+668` | Base projection offsets, additional current jitter offsets |
| `+671`, `+674..+688` | Reverse-depth flag, viewport x/y/width/height/min/max |

`P[2][0] = baseX - 2*jitterX/round(viewportWidth)` and
`P[2][1] = baseY + 2*jitterY/round(viewportHeight)`; rounding is `floor(value+0.5)`.
Remove the current extra offsets from both scalar projections before forming unjittered
reprojection. Keep previous jitter separately. The `+520` inverse-view-projection hypothesis
is rejected (residual over 500,000). All 88 AA views pass the compiled camera converter, maximum
projection/inverse residual `4.68364e-8`; `+671` is one in all 88. Normal near/far/FOV are
`0.15000000596 / 10000 / 37.801094 degrees` (`0.65975355 rad`), close to the earlier native
FSR dispatch's `0.14999111 / 10000 / 0.659771383 rad`; these are observed values, not an exact
bitwise equality. W sample 1 agrees with upscale jitter `(0.25,-0.3888888955)`.
Run X's 192 same-frame ScriptHook comparisons remain the independent camera check described above.

A visually selected static foreground-rock region in AA6/7 excludes Arthur, foliage, sky and
river: rectangle `[85,896,938,128]` in the 1706x1066 active render extent. Each of eight input
sets samples 7,520 texels. Predicted median camera motion is 0.840-6.102 render pixels;
previous-minus-current reprojection residual medians are 0.0427-0.1700 pixels, versus
1.646-11.996 for the opposite sign. Thus static geometry validates direction and scale with
nonzero camera motion. This does not prove independently moving/skinned objects, foliage,
water, particles or complete motion coverage. NGX MV scales are `(1,1)` and create flags 43
mean HDR, low-resolution MV, reversed depth and sharpening, without jittered MV.
Pause AA25/26 freezes projection while NGX jitter continues; the adapter refuses these stale
projection inputs. Z includes a real reset where previous view equals current view.

Producer sites came from matching dump instructions; runtime consumption is proven by the
same-call snapshots. Producer entry execution was not separately traced:

| Producer RVA | Expected first 16 bytes |
| --- | --- |
| `26f63fc` perspective | `48 8b c4 48 89 58 08 57 48 83 ec 70 0f 29 70 e8` |
| `26f5e40` derived state | `48 89 5c 24 08 48 89 7c 24 10 55 48 8d 6c 24 b0` |
| `26f6dfc` current/previous setter | `48 8b c4 48 83 ec 78 41 0f 10 10 41 0f 10 58 10` |
| `26f6ff4` current setter | `48 8b c4 53 48 83 ec 70 0f 10 12 48 8b d9 0f 10` |
| `26f7eb4` world transform | `48 8b c4 48 89 58 08 48 89 78 10 55 48 8d 68 a1` |
| `26f7474` reverse depth | `48 83 ec 28 8b 05 62 8f 44 03 65 4c 8b 0c 25 58` |
| `26f7b1c` jitter | `48 83 ec 28 80 b9 70 06 00 00 00 74 5f f3 0f 59` |
| `1eb420` worker view | `48 89 5c 24 08 48 89 74 24 10 57 48 83 ec 20 65` |

The normal final scene blit is PS `BB74CEA4` (VS `DB61C169`) through
`5c1254 -> 5c11b9 -> 5f1ef5 -> 5cad9b`. In AA6/7 this draw shares the exact NGX
command-list recording generation. UI commonly records on another list through `596828`
(draw return `596896`), so latest CPU counter alone cannot associate final presentation.
AA25/26 pause has scene-wide processing `A7D93689`, `A85DBFA9`, `2EACAA61`, then final
`BB74CEA4` before UI. Map AA21 has 60 draws, log AA22 94, weapon wheel AA23 77, item wheel
AA24 68, pause AA25 65, player selection AA26 62 and settings AA27 80. Some armed captures
begin after scene output (AA13 starts at UI `CA8E18BF`); their first copy is not HUD-less.

Original Z pixel differences establish that normal gameplay has a scene-only image followed
by localized HUD/RTSS changes, but wheel/pause/log/Dead Eye have scene-wide effects after the
first scene output. Therefore names such as `ColorLUT - POSTFX POSTHUD`, `LUTMask - PostHUD
Stage` and `Working PostHud Backbuffer Pair` cannot establish a universal earlier boundary.
AA's full sequence resolves the former 64-draw truncation. It does not prove a reusable
mode-independent separation hook. The captured SDR swapchain is B8G8R8A8_UNORM with signalled
`RGB_FULL_G22_NONE_P709` colour space; temporal colour/output are R16G16B16A16_FLOAT, depth
R32G8X24_TYPELESS (copied depth plane R32_TYPELESS), MV R16G16_FLOAT and exposure R32_FLOAT.
This distinguishes linear scene HDR from an SDR presentation output.

UI families include `CA8E18BF`, `2E866023`, `EE341D7A`, `1A0D957F`, `737584AF`.
Measured ordinary UI uses RGB SrcAlpha/InvSrcAlpha and alpha One/InvSrcAlpha. Drawing these
same passes into transparent black can accumulate a premultiplied UI layer, provided every
scene-wide effect remains in the scene path and all shader variants retain their measured
LUT/PQ conventions. X's controlled replay produced such a layer (148,170 nonzero-alpha pixels,
maximum RGB-minus-alpha zero), but its scene replay was black, so this is an extraction
experiment, not native-game UI-only-layer acceptance. RTSS uses PS `CB54F2A6`/`D70DAF42`,
VS `88A78FCD`, after scene/UI. Social Club can draw into the game's own list; the existing
supplied-buffer selector correction remains necessary and is preserved. A visible Social Club
overlay, photo mode, cinematic camera, letterboxed cutscenes and their full colour contracts
are not proven by AA. The human has no HDR display: HDR pixels remain untested.

The implemented SR adapter keeps game hooks in `games/rdr2/src/native_renderer.cpp` and vendor
contexts/settings/retirement in `runtime/orchestrator/src/rdr2_sr_host.cpp`. Game ABI 17 adds
synchronous acceptance and before/after actual submission and Present callbacks. DLSS helper
`26f3a3c` expects `48 89 5c 24 08 48 89 6c 24 10 48 89 74 24 18 57`.
Hooking inside its wrapper preserves renderer cache restoration at `26ce948`, called from
`26f41cc`. Native FSR's checked thunk `2ea0af5` expects `ff 25 9d 1f 19 03` and reads IAT
`6032a98`; only calls returning to `26f050a` normalize into the same DLSS-shaped descriptor.
The native FSR implementation is skipped only after successful RSF output. Its resource
state declarations and 1,352-byte FSR 2.1.2 layout are statically checked; live routing is pending.
**Correction, 30 October:** this import-slot patch never routed a dispatch on an Intel Arc B370
while the game ran its FSR2 carrier. The route now detours the DLL export, which is what the
census measured (see "FSR2 input route on Intel" below).

Queue observation hooks `26c97d4` with expected bytes
`48 89 5c 24 08 48 89 74 24 10 57 48 83 ec 20 48`, filters caller `26df79b`, and preserves
the real queue's existing Execute entry. The actual swapchain global is `571c7e8`; its Present
observer preserves the existing Steam/RTSS entry. Native frame identity mirrors the game's
render-submit producer: TLS flags `+61c & 6` select the counter pointed to by `5429058`,
otherwise direct `542904c`. SR does not assert this alone as input-to-Present FG ownership.
Engine frame-time source is float `59b393c`, statically multiplied by 1000 at `5c7fca`;
NGX's frame-time key was zero in AA. Direct runtime timing sampling remains pending.

The adapter restores measured DLSS colour/MV state 192, depth 32 and output/exposure 8 after
shared processing. It passes actual exposure, camera, rectangles and previous-minus-current
pixel motion; shared normalization consumes `motion_to_uv = (-scaleX/width,-scaleY/height)`.
Native and RSF histories reset across fallback. Three GPU descriptor slots retain even failed
recordings and retire only after actual queue submission/fence completion; an unsubmitted
slot never causes its producer to wait for submission. Hook/context ownership remains pinned
for the process lifetime. `SR=0/1`, `SRBackend=1/3/4/5` and `DLSSPreset` are live-polled INI
controls; omitted quality follows the game ratio. The render-resolution producer and native
menu replacement remain unfinished, as do FG and PCL translation.

GPU host fixtures on the RTX 4070 Laptop GPU passed DLSS, FSR3 3.1.5, FSR4 4.1.1 INT8 and
XeSS 2.0.2, including external exposure, 426x240 to 640x360 original output pixels, three
unsubmitted recordings/fourth refusal, retirement/reuse and actual DXGI Present bookkeeping.
Logs: `.local/rdr2-tools/sr-host-gpu-exposure-{1,3,4,5}.log`. They caught a dropped DLSS
application ID and missing standalone manual-Present bookkeeping; both were corrected.
Release VS2026 passed 67 executed tests, 11 opt-in skips, fmt and Clippy
(`.local/rdr2-tools/native-adapter-verify.log`). The backed-up deployment receipt is
`.local/rdr2-tools/native-adapter-deployment.json`, ASI hash `28f950b6...406209e7`.
Diagnostics remain off. Current adapter game execution, RTSS compatibility under replacement,
runtime transitions, complete FG inputs and UI-only extraction remain explicitly unaccepted.

### Live SR, NGX lifetime correction and scene producer continuation

Run AB, PID126676, actually executed RSF DLSS, FSR3, FSR4 INT8 and XeSS through the native
DLSS input path, then returned to DLSS without restarting. Twenty fenced original outputs at
1706x1066 -> 2560x1600 are finite and write the full bottom/right edges; this is GPU/game data,
not complete visual or performance acceptance. Native frame24582 records 19.5161018ms from
the engine timing source. Evidence: `.local/rdr2-tools/native-adapter-game-ab-20261026/`,
`attach.log`, `pixels/`, `sr-pixel-validation.json`. FSR4's first full-size evaluation took
approximately56.8s; the human identified the FSR4/XeSS first-use pauses as model loading.

AB then crashed on Pause. Its minidump proves a freed native NGX handle, not a menu draw
failure: `_nvngx.dll+687c3` indexes the feature type from handle+4, but native handle
`0x202cd8c6c40` now contains `{"api":{"build":1,...`. Type `0x7b3a2269` produces invalid
index `0x492a846e58` and read `0x24bca203320`. The stack returns through native NGX evaluation
and the game's DLSS wrapper after frozen-jitter fallback. Global Streamline/NGX shutdown on
RSF context destruction had invalidated the native handle that the game retained. Minidump,
exception-context stack and driver disassembly/facts are retained in that AB directory.

The runtime now retains a single DLSS registration across per-feature SR context changes and
continues its manual Present bookkeeping while other SR providers/native fallback run. It
releases only RSF feature resources on a switch, with GPU completion established. The focused
GPU fixture passes DLSS->FSR3->DLSS while the same registration remains loaded. The Release
VS2026 gate passed 67 executed tests, 11 opt-in skips, fmt/Clippy. Source and logs are linked in
`games/rdr2/RESEARCH-HANDOFF.md`.

The backed-up corrected build was cold-launched as AC, PID126008, Game DLL SHA256
`32b2ca8ec7d536078806383cb8daec6693c4ef38d21897e86e7bd00d9c22eff1`, Runtime
`61dcb4e20824305be5789bc0b9d15c4e23fae7b9677694e16b5fad91922dae8e`. AC collected twelve
outputs over DLSS->FSR3->DLSS, then survived Pause/Settings with repeated native NGX fallback
result1. This accepts the specific lifetime correction and preserves RTSS; FG remains separate.
Its deployment receipt and pixel evidence are under
`.local/rdr2-tools/native-adapter-game-ac-20261026/`.

Subsequent source work resolves the engine render-target owner: context+`ca48` is the colour
view array, +`ca94` its count, +`ca88` the depth view. `26d4964` writes the colour cache;
`26ddf80` reads view+8 as the texture and uses its dimensions. The live DX12 renderer's vtable
+248 resolves `26b4250`, complete bytes `48 8b 42 48 c3`, returning texture+48 as the native
resource. Thus the scene callback can match the actual target to swapchain buffers without
choosing the recording worker's current swapchain index.

`scene_boundary.cpp` now brackets the three postfx-node calls to output helper `5f1e08`
(`5caafc`, `5cad96`, `5cadec`), then observes inner blit `5c1254` before its caller restores
bindings. This excludes unrelated uses of the generic blitter. Expected output-entry bytes
are `48 89 5c 24 08 48 89 74 24 18 57 48 83 ec 50 41`; expected blit entry is
`4c 8b dc 49 89 5b 08 49 89 6b 10 49 89 73 18 49`. A single-target, mip0, exact-backbuffer,
same-frame/same-command-list match is required. Multiple upscale views refuse association.

The marker observer at `26d4674` expects
`48 89 5c 24 08 48 89 54 24 10 55 48 8d 6c 24 a9` and preserves native Reflex behavior.
It associates actual native input/simulation/render marker IDs before publishing source
identity, and compares the recorded scene resource with the actual buffer at native Present
marker4. The runtime records bounded original scene pixels using the accepted SR slot's
copied camera and its actual submission fence. These hooks and the input-to-Present match are
**new source, not yet live measured**. The GPU fixture accepts the later RT-state scene copy,
identical pixels to output reinsertion, correct retirement and debug-layer validation
(`.local/rdr2-tools/scene-producer-fixture.log`). No FG backend execution is claimed.

The human's Settings screenshot shows all native AA rows greyed. A read-only snapshot resolves
21 groups/221 metadata items, including the exact DLSS/FSR rows, but their disabled state is
not an RSF menu edit. The current XML still has both native selectors nonzero; restoration of
the earlier probe's selector changes must retain HDR=false. Native row enable/value/apply
handlers remain unproven. [ScriptHook-first menu research](../../games/rdr2/NATIVE-OPTIONS-RESEARCH.md)
records the concrete sites, live row identities and limits.

### AD game evidence and permanent skipped-input correction

AD PID114512 initially exhausted the six-token Streamline pool while loading. A read-only
copy of the exact loaded Runtime83f91dba proves last_begin1128 with input1123/1125 still held
after the game reached22858. Those simulated inputs never reached Present. Cleanup only after
a valid scene cannot recover: shared SR itself needs a token before that scene can be produced.
Source now retires skipped native CPU frames at ordered actual PresentEnd and clears the
authoritative SDK token pool through the completed input watermark. This also covers CPU-ring
wrap. The regression issues140 inputs without an image, then Present and40 valid SR/FG frames;
39 tags and D3D12 debug validation pass. Full Release VS2026 passes67 executed tests/11 skips,
fmt/Clippy (`rdr2-skipped-input-recovery-gpu.log`, `rdr2-skipped-input-final-verify.log`).

A separate diagnostic, guarded against that exact Runtime entry/global-load instruction,
retired1123/1125 at actual PresentEnd46160 without replacing loaded game files or starting a
second runtime. Shared SR, scene/submission/Present matching and real input pacing immediately
recovered. Foreground gameplay then reports DLSS active1 with driver multiplier2, live4x,
Off and resumed2x. DLSS->FSR3->DLSS SR also retains active2x FG. RTSS and Social Club remain
loaded. These are actual game/SDK activity and transition checks, not generated-image quality,
visible overlay acceptance or physical latency measurements.

The actual hooked normal SDR scene and same-frame final2560x1600 original PPMs differ at116394
pixels, entirely within minimap/prompt ROIs. Every pixel outside is identical. Thus the new
producer, not only the historical PSO census, has pixel proof of the normal SDR HUD boundary.
Eight accepted SR outputs are finite; projection/inverse residual is at most3.93482635e-8.
Evidence is `.local/rdr2-tools/native-adapter-game-ad-20261026/`: `live-tokens.json`,
`token-fix.log`, `hud-pixel-comparison.json`, `sr-pixel-validation.json`, original pixels and
the DLSS count/Off/resume/provider logs. No broader mode/HDR or UI-only layer acceptance follows.

After normal exit, the permanent Runtime
`9a9d0fccb82b53dece1ab7c3595e37d2d1e8077f5821fdf2f3deb953b7d055ac` was backed up and
hash-deployed at18:13 with diagnostics off and2x defaults, `permanent-deployment.json`. Its cold
game run remains unverified. The game-control native pipe is unavailable. The human now
authorizes normal RDR2 closure without another question. Engine resolution ownership, native
FSR selection, stock RSF menu and full-mode HUD/UI-only contracts remain open.

### ABI18 native pacing and cold FG presentation path

The runtime now installs shared FG12 before graphics creation for the real `sgaWindow` class.
The plugin submits exact native latency events through appended `process_latency` rather than
inventing CPU events at upscale. Sleep helper `2687fb4` expects
`40 53 48 83 ec 50 80 3d 06 3d 09 03 00 0f 84 c4`; its input ID mirrors the game's
INPUT_SAMPLE producer, DWORD `542904c` plus1. Native mode wrapper `1e800` expects
`48 89 74 24 10 57 48 83 ec 20 48 8b fa 48 8b f1`. The previously identified marker wrapper
forwards IDs0/1/2/6; shared actual Present emits RenderSubmitEnd/PresentStart/PresentEnd and
suppresses the corresponding native duplicates. RAGE's native RenderSubmitEnd comes after
PresentEnd. An operation is replaced only after runtime ownership accepts it.

The initial raw-queue experiment crashed on first ExecuteCommandLists, with a stack through
D3D12Core and its debug layer to Runtime+3696e. Source inspection of Streamline2.14.1 confirms
that slUpgradeInterface cannot upgrade an existing command queue. Its upgraded device must
create the proxy queue to unwrap proxy command lists and register queue hooks. The existing
bridge retains the actual engine queue and joins source work, presentation preparation and
retirement with GPU fences. This required queue separation is retained; substituting the raw
engine queue for StreamlineHost.queue is rejected. Failed log/debugger stack are preserved in
`.local/rdr2-tools/rdr2-cold-fg-gpu.log` and `rdr2-cold-fg-debug.log`.

Forty-frame RTX4070 Laptop fixtures exercise the actual RDR2Host callbacks with DLSS SR and
DLSS, FSR3 or XeSS FG, including real live-INI provider selection and generation2 transitions.
Each finishes with39 tags, Fixed mode1, result0 and D3D12 debug validation passed. FSR3 reports
generation callbacks; hidden DLSS reports active0, so no displayed/generated-pixel quality is
claimed. Logs are `.local/rdr2-tools/rdr2-cold-fg-{pixel,fsr3,xess}-gpu.log`.
Opt-in SRCaptureDirectory records an original final/HUD-less pair once per generation after30
samples, using the vendor retirement fence. The UI-free fixture's pair has matching SHA256
`71d0931056f1dd54e8a7d1df7279213097d7a8b243290d379409e8ca4cf00cf2` in
`cold-fg-original-pair/`. This proves copying and lifetime, not RDR2's actual HUD boundary.

Release VS2026 passes67 executed tests/11 opt-in skips, fmt/Clippy, log
`.local/rdr2-tools/rdr2-fg-pixels-final-verify.log`. The candidate is staged with diagnostics off
and bounded original pixels. After the human closed AC normally, it was backed up and
hash-verified into the game at17:38, receipt `native-adapter-game-ad-20261026/deployment.json`.
Cold game acceptance is pending. Native hook execution, generated game output, RTSS under FG, all-mode HUD separation,
the resolution producer and stock-menu replacement remain unproven or unfinished.

### FSR2 input route on Intel, 30 October 2026

**Question.** Why does RSF never evaluate SR in RDR2 on an Intel Arc B370 (vendor `0x8086`,
device `0xb083`, driver 32.0.101.9150, 1920x1200)?

**Evidence.** The installed 0.1.1 package (`manifest.json` source `150ff92`) ran three sessions.
The settings owner pinned the FSR2 carrier (`vendor=8086 DLSS=0 FSR2=0->2` and `->3`). The game
refreshed its dimensions to 1130x706 into 1920x1200, the FSR2 Balanced ratio, and
`system.xml` has `fsr2Index=2`. Queue and Present observers attached and the overlay drew.
`rdr2.log` has no `native FSR fallback`, `SR native fallback` or `SR context` line. Each of those
is logged on the first call that reaches `fsr_dispatch` with the expected caller. Therefore no
dispatch reached the replaced pointer in import slot `+6032a98` with return address `+26f050a`.
FG reported `source=0` throughout because its source identity comes from an accepted SR pass.

`RDR2.exe` imports `ffxFsr2ContextDispatch` statically (no delay-load directory; slot
`+6032a98` in `.idata`). The file on disk is encrypted, so the caller was not re-read. The
census that measured 4,412 dispatches and the `+26f050a` return detours the
`ffx_fsr2_api_x64.dll` export by name. OptiScaler `e7e3872d` does the same in
`inputs/FSR2_Dx12.cpp` (`HookFSR2Inputs`). Which mechanism bypasses the import slot is
unresolved.

**Change.** The adapter detours the export in the Rockstar-signed `ffx_fsr2_api_x64.dll`,
SHA-256 `887d11aaef717aa2d4713f4b6fb56b3d4c75737af51042de22f70f0a3fb89f83`, PE timestamp
`64907ede`, image size `0x10000`. `ffxFsr2ContextDispatch` is at `+3c70`, entry bytes
`48 85 c9 74 36 48 85 d2 74 31 8b 41 04 39 82 20` (`+0x520` is `renderSize.width`, matching
the 1,352-byte layout). `ffxFsr2ContextCreate` at `+3ab0`, bytes
`48 89 5c 24 08 57 48 83 ec 20 48 8b da 41 b8 60`, is detoured for evidence only. Calls from
the RDR2 image use the existing normalization. The log records the first caller RVA, any second
caller, whether the import slot resolves to the export, and up to four context creations.

**Static check.** The decrypted 25 October dump (`dumps/rdr2-20261025-dx12/RDR2.exe.dump`)
has `+26f0505 call 2ea0af5`, `2ea0af5 jmp [rip+3191f9d]` = slot `+6032a98`, and the slot holds
`ffx_fsr2_api_x64.dll+3c70` at ASI load. The wrapper `+26efe24..+26f05d4` has no vendor test.
The import-slot route was therefore correct statically; its runtime displacement is unexplained.

**Game result, 30 October.** Built with VS 2026 Build Tools on the Intel device and deployed.
The context-create detour logged `result=0 flags=0x29 max_render=960x600 display=1920x1200` at
startup and after a quality change. On entering gameplay the dispatch detour logged
`RDR2.exe+26f050a (measured wrapper)`, followed by `SR context backend=5 quality=3 input=960x600
output=1920x1200`, evaluation 0 and accepted output. XeSS FG then reported `active=1` with
advancing source frames. The dispatch detour now routes every caller and names the caller module,
because a forwarding hook on the import slot would not return into `RDR2.exe`; that case was not
observed. A menu-only run shows context creation without dispatch, which explains a session
with an FSR2 carrier but no SR line. Image quality on Intel is not assessed.
