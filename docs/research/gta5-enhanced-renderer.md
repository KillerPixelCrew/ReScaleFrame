# GTA V Enhanced renderer

## Installed build and combined input probe, 27 October 2026

The Steam download is complete. The installed manifest reports StateFlags4, build25971420 and
matching downloaded/staged totals. `GTA5_Enhanced.exe` is55,819,384 bytes, version1.0.1158.16,
x64, SHA-256 `b48e80f5fc899eb89e5dcc9f3b68e9551cc8dcfac78b2a940b207e569e9987ea`, PE timestamp
`0x6aa45f10`, preferred base`0x140000000`, entry RVA`0x24790fc`. The allowlist now recognizes
only this file. Recognition still refuses renderer preparation and keeps readiness0.

Question: can the existing vendor census establish native SR/FG inputs, camera and presentation
owners together, before implementing GTA-specific hooks? This approach reuses the measured
shared diagnostic rather than guessing RDR2 addresses. It forwards native calls and executes
no RSF GPU work. The installed files include Streamline2.9.1 (`sl.interposer`, `sl.common`,
`sl.dlss`, `sl.dlss_g`, `sl.reflex`, `sl.pcl`), NGX SR/FG and FidelityFX DX12 version1.0.1.41314.
Those files establish available middleware, not an active API, feature or resource contract.

Static exact-string leads in `.rdata`:

| Name | RVA |
| --- | --- |
| `sl.interposer.dll` (UTF-16) | `0x28363e0` |
| `slInit` | `0x27d5cc6` |
| `slSetConstants` | `0x27d8fc5` |
| `slSetTagForFrame` | `0x27f29f0` |
| `slEvaluateFeature` | `0x27f10de` |
| `slGetFeatureFunction` | `0x27e4b8f` |
| `slDLSSSetOptions` | `0x27da8ef` |
| `slDLSSGSetOptions` | `0x27da910` |
| `slReflexSetOptions` | `0x27da8dc` |
| `ffxCreateContext` | `0x27d24e8` |
| `ffxDispatch` | `0x27ebe30` |

These are string locations, not producer RVAs or hook sites. Normal PE imports do not include
the graphics/vendor entry points. Both file `.text` sections have about7.99 bits/byte entropy;
this suggests protected code. A REA/Ghidra file import was stopped before runtime testing because
analysis of the protected file had not produced usable call-site evidence. Do not accept random
file disassembly as decrypted code. Obtain a runtime dump and callers from actual vendor calls.

`[GTA5Enhanced] Census=1` now installs the existing census on the ASI worker, after exact-file
hashing and before loading RSF's runtime. Enable also must be1. Defaults remain off. The probe
skips static signatures and Vulkan interception for this DX12-first experiment, leaves the debug
layer off, caps JSONL at64 MiB, and samples after32 full records per site. Calls before the
worker installs are outside its coverage and must be identified in the actual run. Native DLSS,
DLSS-G, FSR, Reflex/PCL, command submissions and Present are observation targets. They are
forwarded, not replaced. Native FG suppression and RSF GPU execution are still unimplemented.

The shared Streamline decoder previously omitted matrices and axes. It now copies all five
matrices and world-space position/up/right/forward at `slSetConstants`, together with the native
frame token, viewport, jitter, near/far/FOV and flags. Offsets after the32-byte BaseStructure are
32/96/160/224/288 for the five matrices, and376/388/400/412 for the four vectors. The decoder
reads the456-byte v1 prefix also shared by v2. These offsets are checked against the local vendor
headers and by a distinct-value fake-DLL fixture; this is layout/decoder evidence, not proof that
GTA supplies valid values. Streamline specifies row-major unjittered matrices and pixel-space
jitter. Game compliance, units, axes, depth convention, projection-inverse products and
reprojection against actual motion remain unproven until capture. HUDLessColor/UIColorAndAlpha
tags are leads; pixels still must establish the real HUD boundary.

Initial Release VS2026 gate:99 discovered,88 passed,11 optional skips, formatting and Clippy passed.
The real ASI stand-in fixture explicitly requests Census on a wrong fingerprint and verifies
that no census directory is created. Default-off and Legacy refusal remain covered. Deployed
ASI SHA-256 is`a66c04e354fa4ce3e4e5fce385eae43651d4a096683f972dc957925ffcf9f8d9`;
plugin SHA-256 is`b4c0a2d30ddc20fbce47f3bd6286b7c939fe59db34457a27024f7fa9b3fd522b`.
The existing ASI loaders and ScriptHookV.dll are preserved. A downloaded newer Ultimate ASI Loader
was unnecessary and was not retained in the installation. No caches, launcher or BattlEye
settings were changed. [Script Hook V](https://www.dev-c.com/gtav/scripthookv/) currently lists
1158.13, so its scripting API is not assumed compatible with1158.16. This probe only needs an
ASI loader. [Rockstar documents Story Mode without BattlEye](https://support.rockstargames.com/articles/1nenwhZlVrJY6CTFeSS2Fx/grand-theft-auto-online-battleye-faq).

Evidence under `.local/gta5-enhanced/installed-20261027/`: `fingerprint.json`,
`installed-static.json`, `verify.log`, `deployment.json`, and `ghidra-file-analysis.log`.
The local inventory records exact middleware hashes and file sections. No game binaries or
decompiled/reference implementation are included in Git.

First live attempts: PID21148 exited with`0xc00000fd` (stack overflow); PID5116 exited with
`0xc0000005` (access violation), per Steam's process log. Both loaded the ASI, installed the census
and recognized the exact build, but reached no Present or vendor-frame input. This is a failed
startup, not game acceptance. No Windows crash dump was found. Logs are preserved under
`startup-failure-2138/`. The first non-elevated debugger attach was refused by Windows (error5).
An elevated helper captured the failed process. The summary's fault
site, ASI RVA`0x24657`, is the stack-probe write `mov byte ptr [r11], 0`, verified against the
deployed ASI bytes with Capstone. This localizes stack exhaustion to an ASI allocation, but does
not identify which call chain consumed the stack. The following captures resolved two defects.

### Startup corrections and accepted probe

PID113676's full dump (`startup-paused-full.dmp`, `startup-fault-thread.txt`) shows repeated calls
between Steam overlay's factory wrapper and the census wrapper (ASI return RVA`0xce9e`). An early
GetProcAddress thunk could be cached as an overlay's original while the census forwarded into
that overlay. Graphics creation exports now preserve the resolver's exact pointer and use native
entry detours. A DLL fixture resolves all four creation APIs inside DllMain, before post_load,
and verifies identity. The existing WARP case still verifies recorded native creation calls.
The first candidate removed the pointer conflicts and reached the game factory call, but still
failed. It was not accepted as fixing startup.

The next controlled trace, PID134248 (`corrected-startup.dmp`, `corrected-startup.log`), catches
DLL_THREAD_ATTACH: RCX is the ASI module, RDX is2, and its entry prologue requests`0x10040` bytes
before testing the reason. The64 KiB automatic executable-path buffer overflowed small native
thread stacks. Census adds static TLS, so thread notifications cannot be assumed disabled;
[Microsoft documents this restriction](https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-disablethreadlibrarycalls).
The path buffer is now static storage, keeping every thread-attach entry small. The real-ASI
fixture now creates a64 KiB-reserve thread after loading. It passes the corrected ASI and fails
the original ASI in isolated stand-in processes (`old-stack-control-result.json`). No old build
was restored into the game for that control.

Final Release VS2026:100 discovered,89 passed,11 optional skips, formatting and Clippy passed.
`thread-attach-fix-verify.log` is the final gate; the earlier failed build was a fixture export-name
collision with Windows SDK declarations, corrected using explicit export aliases. The deployed
ASI SHA-256 is`3a164032298e585b11459f781ab1b06e213cef8df772283b5153f69552f0b2a5`;
`thread-attach-deployment.json` records hashes and backups. Both probe defects are corrected;
the subsequent PID107892 stays running and the human confirms Story gameplay with native DLSS,
FG and Reflex enabled. RTSS's`CreateSwapChainForHwnd` call at return RVA`0x71ffb` succeeds. This
accepts entry/probe coexistence in this session, not RSF SR/FG execution. Runs now use a separate
`<pid>-<tick>` directory so a retry cannot overwrite earlier evidence.

### Native frame inputs, PID107892

Frozen records are in `run-107892-452409125/{census.jsonl,census-summary.json,survey.json,gameplay-analysis.json}`
under the evidence directory above. They show successful native DLSS evaluate calls after the
human enables it; earlier gameplay had no recorded SR dispatches. At the restart reconciliation
the process had93092 presents and no queue, size or budget record drops. These are presentation
calls, not an FPS, latency or RSF-generated-frame claim.

The completed run exits with code0 at22:26:38, per Steam's process log. The final archive is
`final-run-107892-452409125/{census.jsonl,census-summary.json,gameplay-analysis.json}`. Its last
summary reports145015 presents,3,879,574 intercepted calls and no recorded drops; later JSONL
records reach present145237. The summary precedes exit, so its counters are not final totals.
The complete JSONL has7371 lines including its header and201 camera samples. Projection/inverse
error remains6e-8; history/inverse error is at most8.55e-8. Eleven sampled world positions occur,
but raw motion-vector reprojection is still untested. The earlier135-sample archive is preserved.

All addresses below are **return RVAs after intercepted calls**, not verified instruction starts
or approved patch sites. No GTA producer code has been patched or its private view layout decoded.

| Native call | Game return RVA | Observed role |
| --- | --- | --- |
| `slInit` | `0x781d8c` | thread146768; SDK`0x200090001fedc`, features0/3/4/1000, DX12 |
| `slSetD3DDevice` | `0x1e80287` | thread126524 |
| `slSetConstants` | `0x49dda4` | eight workers, viewport0, explicit frame token |
| SR `slSetTagForFrame` | `0x49babe` | same worker, frame and command list as SR evaluate |
| `slEvaluateFeature` | `0x49bb00` | feature0, successful DLSS SR, same eight workers |
| FG scene/UI `slSetTagForFrame` | `0x49fce3` | same frame token, can be a different worker from SR |
| `slDLSSSetOptions` | `0x49c663` | mode3, output2560x1600, pre-exposure/exposure scale1 |
| `slDLSSGSetOptions` | `0x4a0d98` | thread140856; On and Off observed, requested1 generated frame |
| `slReflexSetOptions` | `0x78254c`, `0x7825fa` | render/main initialization and later setting changes |

The eight sampled SR workers are21372,21916,43872,134932,136380,140660,140972,147340. Frame31152
uses thread140972 and command list`0x1510c7a8510` for SR; FG scene/UI tagging for the same frame
uses thread21372 and a different command list. Associate metadata by frame token and viewport,
not a render-thread singleton. Null-resource tag calls at`0x49bf6c`/`0x4a0c3e` are not usable
scene resources. Native FG/Reflex Off requests also occur after On; foreground/pause policy has
not been isolated, so an enabled menu option alone is not evidence of continuous native FG.

The135 sampled Constants v2 packets supply unjittered projection/inverse, lens identity,
current-to-previous clip and its inverse, world position/axes and separate jitter. Maximum
projection/inverse error in either order and history/inverse error are`6.0e-8`. All sampled poses
are static, so moving-camera history and object reprojection remain untested. The projection has
`w=-viewZ` and reversed depth. Its coefficients independently derive near`0.1`, far`10000.0002`
and vertical FOV`0.872664623` radians (50 degrees), agreeing with the reported0.1/10000/0.87266463.
Projection aspect is1.60000007; reported aspect is1707/1067 (1.59981251). Preserve the distinction
between display and rounded render extents.

Camera position in the initial samples is(6.62053347,534.458862,176.56752). Up is close to positive
world Z and all three supplied axes have unit length within about`2.1e-8`. World-unit scale and
the supplied forward vector's relation to Script Hook's rendered look direction remain unproven.
Thirty-four nested SL/NGX evaluations match by worker, command list, entry time and completion
sequence. Their jitter agrees exactly with the same sign, including(0.5,0.5),(-0.0625,0.462962955)
and(0.1875,0.129629612). NGX MV.Scale is(1,1); SL mvecScale is(1/1707,1/1067). This is a native
API contract for pixel motion, not a raw-vector direction/coverage validation.

| Tagged resource | Measured allocation | Tag state/lifecycle |
| --- | --- | --- |
| ScalingInputColor | 1707x1067 RGBA16F | `0xc0`,2 |
| ScalingOutputColor | 2560x1600 RGBA16F | `0x8`,1; tagged extent still1707x1067 |
| Depth | 1707x1067 R32G8X24_TYPELESS | `0xc0`,2 |
| MotionVectors | 1707x1067 R16G16_FLOAT | `0xc0`,2 |
| BiasCurrentColorHint | 1707x1067 R8_UNORM | `0xc0`,1 |
| HUDLessColor | 2560x1600 BGRA8_UNORM | `0xc0`,1 |
| UIColorAndAlpha | separate2560x1600 BGRA8_UNORM | `0xc0`,1 |

State`0xc0` requests pixel/non-pixel shader read and`0x8` requests UAV. Lifecycle2 means valid
until evaluate;1 means valid until Present. These do not grant RSF a lease after the native
consumer or preserve contents against later game writes. Exposure is null in the sampled SR
packets. The SDR swapchain is BGRA8_UNORM with color space0 (full-range G22/P709), initially
three buffers and recreated with four when native FG is enabled, through RTSS. HDR is not tested.

GTA's native scene/UI resources are promising FG boundaries, but no captured pixels establish
whether they exclude every UI path, how alpha is premultiplied, or what post-UI grading changes.
Map, pause, cutscenes, HUD modes, motion-vector reprojection, private producer bytes/layouts,
FSR-to-canonical routing, RSF quality control, native-feature suppression and RSF reinsertion all
remain unimplemented or unverified. GTA already owns a Streamline2.9.1 device/swapchain path;
do not copy RDR2's independent Streamline initialization and accidentally create a second owner.
The RSF plugin continues refusing rendering preparation and reports readiness0.

## Initial scaffold evidence, 27 October 2026

Determine which RDR2 RAGE concepts and shared RSF components can be reused for GTA V Enhanced,
and which game-owned producer sites must be established independently. This increment supplies
an entry and plugin scaffold; SR and FG are not implemented.

Steam app3240220 is downloading to `Grand Theft Auto V Enhanced`. Its manifest reported target
build25971420, StateFlags1026 and zero bytes staged/downloaded at inspection. The staging directory
already contains `GTA5_Enhanced.exe`, but its first64 bytes were all zero and the PE inspector
rejected it. Its apparent file length is not proof of downloaded content. No executable hash,
version, PE fingerprint, imports, RVAs, or bytes are accepted from that placeholder. Reinspect
the completed installation and record its actual identity before adding an allowlist entry.

Rockstar advertises native DLSS, FSR, ray-traced features and DirectStorage for Enhanced in its
[Steam listing](https://store.steampowered.com/app/3240220/Grand_Theft_Auto_V_Enhanced/).
The scaffold also cited a [graphics support page](https://support.rockstargames.com/articles/2vEOZW3tzshakakcwRvakg/grand-theft-auto-v-on-pc-crashing-with-err-gfx-state-error)
for a Vulkan option. That did not establish such a path in this Enhanced build; only DX12 is
measured here and Vulkan remains unverified. Published features are not measured RSF inputs.
The game is RAGE-family, but exact structures, axes, depth conventions and code ownership remain
unverified for this build. Do not reuse RDR2 RVAs, bytes or structure offsets by resemblance.

## Existing references and concrete leads

- The existing local [OptiScaler source](https://github.com/optiscaler/OptiScaler/blob/92337ebfbf07d533eabe3731fd8de1bb19437aea/OptiScaler/misc/Quirks.h)
  disables its HUD-fix path for `gta5_enhanced.exe`. Its separately versioned
  [game-specific wiki](https://github.com/optiscaler/OptiScaler/wiki/Grand-Theft-Auto-V-Enhanced)
  reports DLSS and FSR3.1 inputs, hardcoded quality presets and a BattlEye compatibility issue.
  These are external observations, not proof for the current build. Do not copy its implementation.
- The existing local [RenoDX GTA V Enhanced implementation](https://github.com/clshortfuse/renodx/tree/da608c4d4feaa41b261e252440a73337613c2705/src/games/gtav-enhanced)
  has composite, depth-of-field, motion-blur, calibration, Bink and other shader candidates.
  Its addon excludes `RGSCD3D12_TEMPWINDOW` and describes an HDR10 swapchain conversion path.
  Those are capture targets and a Social Club coexistence lead, not an established HUD-less
  boundary or a reason to reuse the RDR2 Social Club patch.
- [Script Hook V's developer](https://www.dev-c.com/gtav/scripthookv/) describes GTA V ASI loading
  and says Script Hook does not work in GTA Online. A version-compatible Enhanced loader,
  initialization order and coexistence with this game's current protection are not yet tested.

The shared runtime, native D3D12 overlay, backend contexts, presentation bridge, bounded census,
frame capture and frame/camera ABI can be reused once the game plugin supplies validated leases.
RDR2's worker-to-view association, carrier selection, quality transaction, camera decoder,
Social Club fix and scene/HUD boundary are hypotheses to investigate, not portable constants.
The intended control model follows RDR2: RSF owns the upscaler and FG choices in its overlay,
routes game inputs into one canonical adapter, and avoids running the game's own FG alongside
RSF. Native FSR3.1 is a candidate input carrier where DLSS is unavailable. These are design
targets, not implemented paths or a hardware restriction.

## Next measurements

1. Capture pixels from the tagged scene/UI resources and final image, including map, pause,
   cutscenes and post-HUD changes. Validate alpha/color conventions and resource reuse; names
   and tags alone do not establish a usable HUD-less boundary.
2. Resolve private producer instructions/expected bytes, resolution ownership and quality
   transitions. Validate moving-camera and object reprojection, then inspect the native FSR
   path for canonical input routing on non-NVIDIA hardware.
3. Integrate one Streamline/presentation owner with RSF's shared backends and overlay. Implement
   native-feature suppression, leased input production and output reinsertion before claiming
   RSF SR/FG, latency control, runtime quality/provider transitions or broader mode coverage.

Entry/probe startup and native DLSS API inputs have game evidence above. No pixel-boundary
capture or RSF renderer execution is accepted. Unknown fingerprints still refuse, readiness
remains0, and the final100-case gate has89 passes/11 optional skips. Diagnostics are reset to
Census=0 for subsequent launches; the existing bounded capture stays installed until this
process exits. No binary is replaced in the running game.
