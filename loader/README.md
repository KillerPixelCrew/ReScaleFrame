# Loader and research proxy

`rsf_bootstrap` exports a version only; product launching and frontend IPC remain pending.
Current game loading uses the AC7 DirectInput carrier and shared Unity version shim. Runtime-owned
plugin preparation/start/quiesce/stop and renderer/CPU callbacks are implemented. The published
v0.1.0 AC7 ZIP is SR-only; FG/Reflex below describe current source/development deployments.
See [current implementation and validation](../docs/current-status.md).

## Source map and ownership

| Source | Responsibility and boundary |
| --- | --- |
| [`src/bootstrap.cpp`](src/bootstrap.cpp) | Immutable version export; no loading handshake |
| [`proxy/src/dinput8_proxy.c`](proxy/src/dinput8_proxy.c) | AC7 DirectInput forwarding, flat settings/defaults, decryption worker, guarded engine patches, persistence actions and crash diagnostics |
| [`proxy/src/dlss_bridge.h`](proxy/src/dlss_bridge.h), [`dlss_bridge.c`](proxy/src/dlss_bridge.c) | Private carrier/runtime bridge: native plugin preparation, graph callbacks, compatibility discovery/reinsertion, frame-boundary actions and shutdown draining |
| [`proxy/src/overlay_host.h`](proxy/src/overlay_host.h), [`overlay_host.cpp`](proxy/src/overlay_host.cpp) | Rust panel loading, window input, D3D11 drawing and scoped target references |
| [`proxy/src/preferences.h`](proxy/src/preferences.h), [`preferences.c`](proxy/src/preferences.c) | Per-user `[Rendering]` enabled/preset profile, with synchronous Win32 reads/writes |
| [`proxy/src/shim_loader.cpp`](proxy/src/shim_loader.cpp), [`version_forwarders.asm`](proxy/src/version_forwarders.asm) | Generic Windows version forwarding and worker startup, with preserved x64 argument ABI |
| [`diagnostics/include/rescaleframe/module_dump.h`](diagnostics/include/rescaleframe/module_dump.h) | PE64 dumping, entropy polling and researched code/cvar writes; no renderer activation |
| [`diagnostics/include/rescaleframe/frame_capture.h`](diagnostics/include/rescaleframe/frame_capture.h) | Optional process-global RenderDoc capture API; configured before device creation |

AC7 attachment starts the preparation worker. The worker installs observer and cold-presentation
hooks before waiting for stable decryption, applies guarded patches, prepares the game DLL and
publishes readiness. Present starts SR after its device/extent exist. Native callbacks evaluate on
the queued graphics execution stream; compatibility mode instead observes D3D bindings and builds
a bounded tail plan. Window callbacks collect input; Present applies resource/settings changes.

Native pass pointers are borrowed under the SDK leases. Compatibility snapshots retained beyond
a callback take their own COM references and release/replace them when ownership ends. Retention
keeps an allocation alive without proving its current engine role. Shutdown quiesces native
producers and retries stop while queued work remains, keeping callback code/backend state alive
until it drains. Loader-lock notifications do not perform that graphics teardown.

## Research setup

Place the proxy beside `Ace7Game.exe`. Under Proton, select it with `WINEDLLOVERRIDES="dinput8=n,b"`. It forwards AC7's imported `DirectInput8Create` to the system DLL.

Deploy `dinput8.dll` and the Rust `rescaleframe_overlay.dll` together. Their overlay ABI must match;
native verification uses a panel fixture and does not build the Rust release DLL. The 30 September
capture build exposed this when the proxy expected ABI 4 and the installed overlay still used ABI 3,
leaving Insert unavailable. After configuring/verifying the native build, use the explicit local
deployment command with AC7 closed:

```powershell
./eng/deploy-ac7-proxy.ps1 -GameDirectory 'D:/SteamLibrary/steamapps/common/ACE COMBAT 7'
```

It builds the dedicated AC7 carrier and matching Rust overlay, checks the real overlay's ABI,
Insert handling, rendering and resize, then backs up and deploys the proxy, game plugin, overlay
and SR runtimes. Routine `eng/verify.ps1` does not touch the game installation.

Keep the separately obtained NVIDIA runtime in a directory visible to the game, for example `ReScaleFrame/streamline/` beside the executable. The tested set includes `sl.interposer.dll`, `sl.common.dll`, `sl.dlss.dll`, `sl.pcl.dll`, and `nvngx_dlss.dll`. See [dependencies](../docs/dependencies.md) for versions and terms.

Example Steam launch options, on one line, with both paths replaced by Windows paths visible inside Proton:

```text
PROTON_ENABLE_NVAPI=1 RSF_OBSERVE=1 RSF_ENABLE_JITTER=1 RSF_DUMP_DIR="Z:\path\to\captures" RSF_STREAMLINE_BIN="Z:\path\to\ReScaleFrame\streamline" WINEDLLOVERRIDES="dinput8=n,b" %command%
```

## Playing

DLSS and reinsertion start automatically after the executable is ready and the presenting device
exists. The first few rendered frames identify the scene and its composition path. Startup does
not require a capture directory or a developer hotkey. Missing backend support leaves DLSS inactive
and reports the reason.

**Insert** opens or closes the overlay. Current controls include **Enable upscaling**, SR backend
and five presets, plus independent FG provider/multiplier, Reflex and FPS controls where available.
The old consumer function-key actions are removed; opt-in F9 motion capture remains.
Disable restores 100% scene resolution, stops reinsertion
and closes the forced jitter gate. Native selects DLAA with reinsertion at output resolution.

The selected SDK supplies the render sizes at startup and on quality changes. Native mode sizes
engine-owned views; the compatibility setter accepts the last applied percentage or the game's
reset to 100%. A refused compatibility write rolls the mode back. Changes reset history and wait
for a fresh accepted frame before rebuilding reinsertion. The backend stays loaded while disabled.

Enable, quality and FG provider choices persist in `%LOCALAPPDATA%\ReScaleFrame\AC7.ini`,
independently of the installation's advanced settings. SR backend choice remains per session.
Without saved choices, DLSS starts enabled with Performance selected.

The briefing layer targets 100% of output resolution at every scene quality, using the
existing expected-byte-checked allocation-scale patch. It uses unjittered views across VS, PS, HS, DS and GS
and is recombined after scene SR, before tonemapping. This replaces the default secondary DLSS
integration. See [the implementation and validation limits](../docs/research/ac7-consumer-session.md).

## Settings

AC7 settings live in `ReScaleFrame.ini` beside the proxy, one unquoted `NAME=value` per line.
The first lookup caches at most 64 entries; keys are case-insensitive, the first duplicate wins,
and `#` or `;` anywhere on a line starts a comment. Malformed/oversized entries are skipped.
[`ReScaleFrame.ini.sample`](ReScaleFrame.ini.sample) is a starting point. A nonempty environment
variable of the same name takes precedence. Saved enable/quality/provider choices then override
their installation defaults. Paths resolve beside the DLL, independently of the working directory.

Numbers use unsigned base-zero parsing, including decimal and `0x` hexadecimal. Explicit zero is
preserved, so `RSF_DECODE_MOTION=0` disables decoding and quality `0` selects Native. Empty text
or text without a numeric prefix uses the fallback. Trailing text and overflow are not separately
validated by this diagnostic parser.

The vendor runtime is found at `ReScaleFrame\streamline` beside the proxy, and `renderdoc.dll`
beside the proxy, so neither path normally needs setting at all.

## AC7 environment settings

These keys belong to the AC7 carrier. Several observer/promotion overrides describe the retained
compatibility path; the default native plugin sizes engine views/targets and inserts SR in its
post-process graph. Do not apply this flat AC7 configuration format to Unity's `[UnitySR]` INI.

| Variable | Default / purpose |
| --- | --- |
| `RSF_DUMP_DIR` | Logs/captures; defaults to `%LOCALAPPDATA%\ReScaleFrame\AC7` |
| `RSF_DUMP_MODULE` | `0`; opt in to executable dumps and module sampling |
| `RSF_DLSS_ENABLE` | `1`; initial enable choice before saved preferences |
| `RSF_OBSERVE` | `1`; install the D3D11 observer |
| `RSF_OBSERVE_FORMAT` | `35`, `R16G16_UNORM` |
| `RSF_OBSERVE_MIN_WIDTH` | `512` |
| `RSF_OBSERVE_CAPACITY` | `8` retained textures |
| `RSF_VIEW_CB_MIN`, `RSF_VIEW_CB_MAX` | `1024`, `8192` bytes |
| `RSF_RENDERDOC_DLL` | Explicit Windows RenderDoc DLL path |
| `RSF_CAPTURE_PREFIX` | RenderDoc output prefix |
| `RSF_UI_CLASSIFY` | `1`; name pipeline objects as the game creates them and classify the draws made from them. Reports through the `ui:` lines and changes nothing |
| `RSF_UI_SHADER_FORCE`, `RSF_UI_SHADER_SKIP` | hex hash lists naming shaders the rules got wrong, in either direction. The hashes are printed by the `ui draw:` trace lines |
| `RSF_UI_ENCODE` | `1`; which transfer function the composite applies to the extracted interface. `0` none, `1` sRGB, `2` gamma 2.2 |
| `RSF_ENABLE_JITTER` | Default `1`; `0` off; `1` patches the AA gate but opens it only while a reconstruction runs; `2` opens it from decryption to exit, which is what every result before this used |
| `RSF_JITTER_RVA` | Default address `0x112b1f3` |
| `RSF_TRANSLUCENT_VELOCITY` | `0`; set `1` to let translucent draws reach the velocity pass |
| `RSF_TRANSLUCENT_VELOCITY_RVA` | Default address `0x11823de` |
| `RSF_SCREEN_PERCENTAGE` | Legacy diagnostic setting; consumer startup uses the selected DLSS preset |
| `RSF_CONSOLE_SINGLETON_RVA` | Default address `0x3a8b290` |
| `RSF_CONSOLE_FIND_SLOT` | Default byte offset `0x90` |
| `RSF_STREAMLINE_BIN` | Directory containing the vendor runtime |
| `RSF_DLSS_QUALITY` | `3`: Performance; backend enum is Native=0, Quality=1, Balanced=2, Performance=3, Ultra Performance=4 |
| `RSF_DLSS_TONEMAP` | `1`: DLSS receives an invertible display-range encoding with HDR input off, decoded back to linear; removes the dark banding of DLSS 310's auto-exposing presets on any preset or driver override. `0` sends linear HDR, for comparison only |
| `RSF_DLSS_COLOUR_CORRECTION` | `0`: the post-DLSS colour correction stays off. `1` re-enables it for comparison; it adds edge shimmer in the hangar |
| `RSF_DLSS_OUTPUT_WIDTH`, `RSF_DLSS_OUTPUT_HEIGHT` | Observer's presented dimensions |
| `RSF_DECODE_MOTION` | `1`; controls decoded diagnostic dumps |
| `RSF_OVERLAY_DLL` | Explicit path to `rescaleframe_overlay.dll`; otherwise looked for beside the proxy |
| `RSF_RENDERDOC` | `0`; set `1` to load RenderDoc at attach for a capture session |
| `RSF_FULL_TRANSLUCENCY` | `1`; patches the separate translucency halving out and carries the scale as a rewritable immediate |
| `RSF_FULL_TRANSLUCENCY_RVA` | Default address `0x10be329` |
| `RSF_TRANSLUCENCY_TARGET` | `100`; percent of output, independent of the scene preset. `0` explicitly matches the scene |
| `RSF_TRANSLUCENCY_UNJITTER` | `1`; unjitter VS/PS/HS/DS/GS and bypass layer DLSS. `0` retains the experimental 1:1 temporal route |
| `RSF_TRANSLUCENCY_SCALE` | `0`; a direct multiplier in percent overriding the target |
| `RSF_REINSERT_DEPTH` | `0`; what reinsertion does when a promoted target meets the game's render-resolution depth: drop, keep, refuse |

Additional implemented diagnostics include `RSF_UI_UNJITTER=1`, `RSF_JITTER_ALL_VIEWS=0`,
`RSF_MOTION_CAPTURE=0` and optional `RSF_BRIEFING_CAPTURE_PREFIX`. The capture prefix enables
bounded shader/constant/route output; it is a developer diagnostic and can stall rendering.
Provider directories are listed under SR/FG switching below.

Quality selection derives input dimensions from the selected SDK. Native mode sizes the renderer
through engine-owned views; compatibility mode applies the planned percentage. `RSF_SCREEN_PERCENTAGE`
is a legacy diagnostic override, not a second independent consumer quality setting.
The [representation plan](../docs/representation-plan.md) retains the design history.

## Startup and diagnostics

The worker waits for decrypted code before applying expected-byte-checked patches. The Present
callback starts the backend once the device and dimensions are known; it does not repeatedly load
a failing vendor runtime. Use Enable DLSS to retry after a failure. Renderer maintenance runs on
the render thread. Module dumps are opt-in; vendor informational spam is not forwarded to the log.
Warnings, errors and state changes remain available in `rsf-dump.log`.

The carriers implement current plugin lifecycle loading, while the general product bootstrap and
installer remain incomplete. Recorded game acceptance and synthetic/device checks have separate
scope; neither a build nor a successful deployment proves new game image quality.

Crash diagnostics register a vectored handler that reports one fatal exception and continues the
game's exception search. A separate worker performs the symbol walk/minidump; the handler waits
at most 30 seconds. Its context/report are borrowed, and timeout does not cancel the worker, so
this implementation does not guarantee safe worker completion after that wait expires.

The compatibility UI-extraction path caches a backbuffer RTV. That view retains the texture and
can block `ResizeBuffers`; releasing the temporary `GetBuffer` reference alone does not release
the view dependency. The shared overlay host acquires/releases its draw targets within each call.

## SR backend switching

The current development overlay offers DLSS, FSR2, FSR3, FSR4 and XeSS after startup. Selection is
applied on the render thread; a refused backend keeps the active one. Existing startup is DLSS-first.
The corrected published v0.1.0 package includes DLSS, FSR2/3/4 and XeSS SR. The optimized bridge
and FSR4 INT8 SR path received user acceptance on 2 October. New source work does not update
that published ZIP; later FG/Reflex changes are separate.

`RSF_FFX_BIN` selects the absolute directory containing `amd_fidelityfx_upscaler_dx12.dll`.
It defaults to `ReScaleFrame\fidelityfx` beside the proxy. SDK 2.3.0 can supply all three FSR
families. `RSF_FSR2_BIN`, `RSF_FSR3_BIN` and `RSF_FSR4_BIN` optionally select separate SDK runtime
directories. `RSF_XESS_BIN` selects the directory containing `libxess.dll`, defaulting to
`ReScaleFrame\xess`. Settings use the existing INI/environment precedence. No new vendor binaries
are installed by building or testing; preserve their distribution terms when preparing a package.

[Architecture, SDK revisions, device checks and remaining validation](../docs/research/orchestrator-sr-switching.md).

## AC7 frame generation

Set `RSF_FG_ENABLE=1` in `ReScaleFrame.ini` before starting AC7. This selects the cold D3D12
presentation/SR host while keeping the game-facing D3D11 renderer. `RSF_FG_MODE=0` starts Off;
`1` requests fixed generation. `RSF_FG_GENERATED=1` requests 2x, clamped to the SDK maximum.
`RSF_FG_BACKEND=1/3/4/5` selects DLSS-G, FSR3 FG, FSR4 FG or XeSS FG before startup,
independently of SR. The default is DLSS-G. FSR uses `RSF_FSR3_BIN` / `RSF_FSR4_BIN`, then
`RSF_FFX_BIN`, then `ReScaleFrame/fidelityfx`; XeSS uses `RSF_XESS_BIN`, then
`ReScaleFrame/xess`. Unsupported creation preserves original presentation. Change provider
in the overlay during play. FSR3/4 are 2x in the pinned swapchain API; XeSS limits come from the SDK/hardware query.
`RSF_REFLEX_MODE=0/1/2` selects Off/On/On + Boost. Sleep and PCL markers remain integrated in Off mode.
Reflex controls apply to DLSS-G. XeSS uses XeLL. FSR does not apply the Reflex frame limiter.

Insert exposes requested/effective/active FG and runtime Reflex controls. These FG/Reflex
settings include Show FPS overlay. The compact top-right HUD remains visible with the panel
closed and separately displays real application/rendered FPS and SDK aggregate presented FPS.
It never multiplies the rendered rate by a requested FG setting; the badge reports actual activity.
Reported presents are SDK/DXGI counters, not physical scanout or input-latency measurements.
FG provider choice is saved per user; mode, multiplier, Reflex and frame-limit changes are session
settings. `RSF_FRAME_LIMIT_FPS` limits rendered frames per second before
generation, through Reflex's limiter; the panel's Frame limit control changes it while running.
With 2x generation a limit of 80 presents up to 160. `RSF_REFLEX_LIMIT_US` is the same limit
written as a microsecond interval between rendered frames.
Choose Off in Insert to stop generation through the stable facade. For a cold start, clear the
saved `[Rendering] FrameGeneration` choice to 0 as well as `RSF_FG_ENABLE=0`; saved provider choice
can override that default. Live provider changes no longer require restart. Default runtime mode is
production; development requires `RSF_FG_DEVELOPMENT=1` and `RSF_STREAMLINE_BIN` pointing
at development DLLs. `RSF_FG_DEBUG=1` records a bounded marker trace after tagged frames begin.
The Reflex sleep waits for the previous frame's Present and still precedes input.
`RSF_REFLEX_ASYNC=1` restores the earlier sleep that overlaps the previous frame, for comparison.

The saved FG provider in the per-user profile takes precedence over the INI fallback. In the
current development source, presentation interception installs with runtime switching enabled,
including a requested Off provider; `RSF_FG_ENABLE=0` therefore selects the fallback rather than
guaranteeing interception is absent. The checked-in [sample](ReScaleFrame.ini.sample) lists startup
mode/count/provider, development/debug and rendered-frame limiter keys with their defaults.

The user accepts deployed DLSS-FG. FSR3/XeSS pass synthetic D3D11/D3D12 generation checks;
their AC7 moving-scene quality, HUD and pacing have not been game-tested. Cuts, unmatched inputs
and unsupported VSync suspend generation. Compatible native AC7 family
surfaces now supply optional pre-Slate HUD-less guides, with unclassified UI fallback retained.
That does not establish complete HUD separation or new vendor game acceptance.
[Evidence, dependencies and limits](../docs/research/ac7-dlss-fg-20261003.md).

Unity Mono uses `[UnitySR] FrameGeneration=0/1/3/4/5` and `GeneratedFrames=1` in
`ReScaleFrame.ini`, independently of `Backend`. Install the matching vendor DLLs with
`eng/deploy-unity-sr.ps1 -FrameGeneration FSR3` (or `DLSS` / `FSR4` / `XeSS`).
The shared runtime consumes the completed backbuffer and same-frame normalized depth/motion.
Its CPU callbacks bracket EarlyUpdate through PreLateUpdate. SR Off/FSR1 also supply normalized
inputs. The user accepted XeSS/DLSS-G and the final FSR correction after the recorded runtime
switch/input/pacing repairs. General scene/resize coverage, higher MFG and Claw remain unverified.
[Shared implementation evidence](../docs/research/shared-fg-20261004.md).

The generic version shim uses sectioned Win32 INI parsing, separate from AC7's flat parser.
An existing `ReScaleFrame.ini` enables its worker unless `[UnitySR] AutoStart=0`. It loads
`ReScaleFrame/ReScaleFrame.Runtime.dll` relative to itself, installs generation interception before
waiting for Mono, and retries SR startup while the runtime reports not ready. System version API
forwarding resolves independently from `System32/version.dll`; runtime/library references remain
loaded for the process lifetime. AC7 names dispatch into the AC7 entry and Unity crash reporters
receive forwarding only. The MASM forwarding surface is currently MSVC-only.

Insert now exposes the frame-generation provider separately from the upscaler. Choose Off,
DLSS-G, FSR3, FSR4 or XeSS in Unity. Requests replace the provider after a drained Present,
while the engine retains its render buffers. The selector shows the current and requested
providers separately; GPU compatibility is checked when switching. Unity saves this choice in
`ReScaleFrame/preferences.ini`, outside the deployment-owned INI. AC7's compiled selector
also includes DLSS-G and uses its existing per-user preferences file. Unity live switching and
final corrections have recorded user acceptance; new AC7 FSR/XeSS FG acceptance remains pending.
Active DLSS-G applies effective Reflex at least On even if requested Off; XeSS uses XeLL and FSR
owns its pacing without an NVIDIA SR sleep. FSR3/4 stay 2x in the pinned API; higher DLSS/XeSS
counts follow SDK queries and lack device/game proof on the tested RTX 4070.
# AC7 motion research capture

Set `RSF_MOTION_CAPTURE=1` in the proxy INI before launching AC7. With the game focused,
press F9 once during the scene of interest and let it run for at least five seconds.
Repeat for carrier launch with attached weapons visible, aerial refuelling, moving ground
vehicles, and clouds. For clouds, include a steady-camera sample and a camera-pan sample.
Keep DLSS enabled so paired scene inputs are available. Capture readbacks can briefly hitch.

Each request records 60 Present intervals under
`%LOCALAPPDATA%\ReScaleFrame\AC7\motion-*` (or `RSF_DUMP_DIR`). The observer log reports
capture armed/finished. Samples at intervals 0, 30 and 59 contain raw colour, depth and
motion inputs with frame constants when the backend evaluates. Engine selection decisions,
shader bytecode, draw/dispatch bindings and constant-buffer snapshots help distinguish
eligibility rejection from a shader/history problem. Native decisions are preserved.

`native.jsonl` also records guarded engine post-processing, main temporal graph and widget
producer boundaries. Sampled graph records include output names, extents, formats, pass methods,
pooled targets and native family frame numbers. Draw/dispatch records carry candidate graph-to-RHI
command associations. These are read-only diagnostics: queued execution can occur after the CPU
scope and must be validated before those associations are used as a production frame contract.

Run `python tools/analyze-ac7-motion-capture.py <capture-directory>` to check completeness
and summarize coverage. GPU records cover the installed immediate context; deferred command
lists are not decomposed. Post-processing/converter scopes and sampled graph-to-RHI associations
are AC7-captured; full queue/Present identity remains unvalidated.

Nine completed sessions subsequently validated sampled graph-to-draw associations. A pause-menu
capture exposed an unretained cached translucency pointer in route readback; the corrected build
holds that resource and skips layer capture without a current-frame producer. Sampled native
post-process returns also write `native.partial.jsonl` and numbered blobs before queued graphics
execution. Those files preserve partial evidence and do not mark a session complete. Release
builds emit `dinput8.map` beside the DLL for exact private-function crash addresses. Corrected
pause capture validation is pending; the nine completed sessions do not need repeating.

## Native renderer build

The matching `ReScaleFrame.Game.AC7.dll` is deployed beside the proxy. The runtime prepares it after
decryption and activates it after SR initialization. Expected-byte refusal uses the compatibility
renderer. Native mode sizes renderer-owned views and inserts SR in the native graph; texture
promotion, constant-buffer rewriting and timed rediscovery are disabled there. The corrected native
SR/UI and TrueSky wing/cloud path has user acceptance; later lighting and
motion-coverage changes retain their own validation gaps. [Details and current limits](../docs/research/ac7-native-renderer-refactor-20261001.md).
F9 records copied RHI scope/role/frame identity along with the existing paired images, GPU bindings,
widget observations and screenshot. Native family IDs do not prove simulation/FG identity.
