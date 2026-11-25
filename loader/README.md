# Loader and research proxy

Product launching and frontend IPC remain pending (the version-only bootstrap DLL and launcher
scaffolds were removed; Git history keeps them).
Current game loading uses the AC7 DirectInput carrier and shared Unity version shim. Runtime-owned
plugin preparation/start/quiesce/stop and renderer/CPU callbacks are implemented. The published
v0.1.0 AC7 ZIP is SR-only; FG/Reflex below describe current source/development deployments.
See [current implementation and validation](../docs/current-status.md).

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

Settings live in `ReScaleFrame.ini` beside the proxy, one `NAME=value` per line, `#` or `;` starting
a comment, read once at attach. [`ReScaleFrame.ini.sample`](ReScaleFrame.ini.sample) is a starting
point. The names are the ones below, unchanged, because they were environment variables first and an
environment variable of the same name still wins over the file. That keeps an existing launch line
working and makes a one-off override a launch option rather than an edit.

Numbers accept hexadecimal with an `0x` prefix, and a setting present and zero is that value rather
than an absence, so `RSF_DECODE_MOTION=0` disables decoding and quality `0` selects Native. Both
were review findings against the old parser.

The vendor runtime is found at `ReScaleFrame\streamline` beside the proxy, and `renderdoc.dll`
beside the proxy, so neither path normally needs setting at all.

## AC7 environment settings

These keys belong to the AC7 carrier and cover its input work: the observer, motion capture,
jitter and translucency patches, and diagnostics. SR, frame generation, latency, the frame limit,
DLSS colour transport and SR colour correction are the shared overlay's settings, saved in
`%LOCALAPPDATA%\ReScaleFrame\AC7.ini` `[Rendering]` by the core D3D11 host like every other game.
Do not apply this flat AC7 configuration format to Unity's `[UnitySR]` INI.

| Variable | Default / purpose |
| --- | --- |
| `RSF_DUMP_DIR` | Logs/captures; defaults to `%LOCALAPPDATA%\ReScaleFrame\AC7` |
| `RSF_DUMP_MODULE` | `0`; opt in to executable dumps and module sampling |
| `RSF_OBSERVE` | `1`; install the D3D11 observer |
| `RSF_OBSERVE_FORMAT` | `35`, `R16G16_UNORM` |
| `RSF_OBSERVE_MIN_WIDTH` | `512` |
| `RSF_OBSERVE_CAPACITY` | `8` retained textures |
| `RSF_VIEW_CB_MIN`, `RSF_VIEW_CB_MAX` | `1024`, `8192` bytes |
| `RSF_RENDERDOC_DLL` | Explicit Windows RenderDoc DLL path |
| `RSF_CAPTURE_PREFIX` | RenderDoc output prefix |
| `RSF_ENABLE_JITTER` | Default `1`; `0` off; `1` patches the AA gate but opens it only while a reconstruction runs; `2` opens it from decryption to exit, which is what every result before this used |
| `RSF_TRANSLUCENT_VELOCITY` | `0`; set `1` to let translucent draws reach the velocity pass |

| `RSF_DECODE_MOTION` | `1`; controls decoded diagnostic dumps |
| `RSF_OVERLAY_DLL` | Explicit path to `rescaleframe_overlay.dll`; otherwise looked for beside the proxy |
| `RSF_RENDERDOC` | `0`; set `1` to load RenderDoc at attach for a capture session |
| `RSF_FULL_TRANSLUCENCY` | `1`; patches the separate translucency halving out and carries the scale as a rewritable immediate |
| `RSF_TRANSLUCENCY_TARGET` | `100`; percent of output, independent of the scene preset. `0` explicitly matches the scene |
| `RSF_TRANSLUCENCY_SCALE` | `0`; a direct multiplier in percent overriding the target |

The jitter, translucent velocity and separate translucency sites are discovered by signature in the decrypted executable (`games/ac7/src/discovery.rs`); there is no address override.

The keys the [representation plan](../docs/representation-plan.md) introduces (`RSF_UI_*`, `RSF_POLICY_*`, `RSF_PRESENTATION`, `RSF_FG*`, `RSF_SR_VENDOR`, vendor runtime directories) are documented here as each milestone lands, not before.

Quality selection derives input dimensions from the selected SDK and sizes the renderer
through engine-owned views. A refused native activation leaves the game running without SR.

## Startup and diagnostics

The worker waits for decrypted code before applying expected-byte-checked patches. The carrier
reports to the core D3D11 host (`runtime/orchestrator/src/d3d11_host.rs`): the first Present binds
the presentation, the next one after the patches starts the native renderer once, and the host
plans and starts SR from the presenting chain's extent. Renderer maintenance runs on the render
thread. Module dumps are opt-in; vendor informational spam is not forwarded to the log.
Warnings, errors and state changes remain available in `rsf-dump.log`.

The carriers implement current plugin lifecycle loading, while the general product bootstrap and
installer remain incomplete. Recorded game acceptance and synthetic/device checks have separate
scope; neither a build nor a successful deployment proves new game image quality.

## SR backend switching

The current development overlay offers DLSS, FSR2, FSR3, FSR4 and XeSS after startup. Selection is
applied on the render thread; a refused backend keeps the active one. Existing startup is DLSS-first.
The corrected published v0.1.0 package includes DLSS, FSR2/3/4 and XeSS SR. The optimized bridge
and FSR4 INT8 SR path received user acceptance on 2 October. New source work does not update
that published ZIP; later FG/Reflex changes are separate.

The vendor runtimes come from `ReScaleFrame\streamline`, `ReScaleFrame\fidelityfx` and
`ReScaleFrame\xess` beside the proxy, as for every D3D11 game. No new vendor binaries are
installed by building or testing; preserve their distribution terms when preparing a package.

[Architecture, SDK revisions, device checks and remaining validation](../docs/research/orchestrator-sr-switching.md).

## AC7 frame generation

Frame generation, its provider and multiplier, the latency technology and the frame limit are
chosen in the overlay (Insert) while the game runs and saved per user, as in every game. The
presentation bridge is installed at startup with FG Off, so FG can be switched on at any time.
AC7 supplies completed HUD-less scene colour and preserves depth/motion through interpolation,
independently of the chosen SR vendor. Unsupported provider creation retains the current provider.
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
# AC7 website input dump

This is the shared [orchestrator frame dump](../docs/research/core-frame-dump.md),
also wired through the D3D12/Vulkan integration paths and Minecraft's GL bridge.

With AC7 focused and SR enabled, press **F10** once in the scene to export. No diagnostic
INI setting is needed. Each press writes one source frame under
`%LOCALAPPDATA%\ReScaleFrame\AC7\website\scene-*` (or `RSF_DUMP_DIR\website`).
The readbacks can briefly hitch. Wait for `website dump complete` in `rsf-dump.log`
before taking another scene. A `partial` result names missing/failed inputs in the manifest.

Every available image has a browser-ready, fully opaque `.png`, a bit-exact `.bin`,
and `_raw.json` recording DXGI storage format and packed row pitch. `manifest.json`
names the images, explains preview transforms, and records source/session/view/frame
identity, RSF version, executable SHA-256, and per-image success or unavailability.

| File stem | Contents |
| --- | --- |
| `sr_source_jittered` | Jittered linear HDR scene at the active render rectangle |
| `sr_color` | Actual SR colour input, including DLSS colour transport when active |
| `sr_game_motion`, `sr_motion_decoded`, `sr_motion` | Native encoded, sparse decoded, and actual submitted vectors |
| `sr_scene_depth`, `sr_depth`, `sr_cloud_depth` | Original scene, submitted SR, and cloud/translucent device depth |
| `sr_engine_exposure`, `sr_exposure` | Engine exposure and exposure supplied to SR |
| `sr_opaque_color`, `sr_transparency_layer` | Available opaque colour and separate translucency guides |
| `sr_reactive_mask`, `sr_transparency_mask`, `sr_bias_mask` | Available reactive, coverage, and bias hints |
| `sr_backend_output`, `sr_output` | Backend result and reconstructed linear HDR before engine tonemapping |
| `hudless_color` | Tonemapped scene immediately before the native HUD compositor |
| `hud_color`, `hud_alpha`, `hud_composed_color` | Native HUD raster RGB, alpha coverage, and composition result |
| `fg_depth`, `fg_motion`, `fg_scene_color` | Normalized FG guides and actual completed scene handed to FG before final window UI |
| `final_color` | Final real frame with game UI, before RSF debug/overlay drawing and interpolation |

`sr_constants.json` contains the actual SR jitter, vector scales, camera matrices,
near/far planes, quality/preset, reset, colour transport, and render/output rectangles.
When a provider receives this frame, `fg_constants.json` records its preparation result,
timing, jitter, matrices, vector scales, flags, and generation identity. Enable FG to
include those provider constants. The FG source images are also exported with FG Off.

The native HUD raster is an engine input, with its stored alpha exported separately;
that alpha can remain zero for translucent/additive producers. The current AC7 FG path has no separate `fg_ui` resource; the
manifest marks it unavailable. Optional guides absent in the chosen scene/backend are
also marked unavailable. `hudless_color` is the genuine scene before the UI compositor;
`fg_scene_color` is the later actual FG input and can already contain game UI.
Both are preserved. No image is substituted from another frame.

HDR previews use Reinhard plus sRGB. Reversed-Z depth uses logarithmic grayscale.
Vectors centre zero at gray, with X in red and Y in green; sparse unwritten vectors
are blue. Dense pixel vectors use a fixed 32-pixel display range and have no sparse
sentinel. Masks and HUD alpha use grayscale. Preview transforms never alter raw data.

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
