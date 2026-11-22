# Red Dead Redemption 2

Read [RESEARCH-HANDOFF.md](RESEARCH-HANDOFF.md) before continuing integration or requesting
captures. It retains the Mor3a reference findings, measured camera/HUD evidence and failed
approaches from 26 October.

The Windows x64 plugin recognizes the fingerprinted Steam executable and exposes game-plugin
ABI 18. Its guarded native DLSS and FSR paths feed a common SR adapter; preparation refuses
without the runtime host or matching native instructions. The ASI forces DX12 and supports an optional vendor census and HRESULT
trace. A guarded Social Club correction preserves the game's recording-frame backbuffer index.
Forced DX12 with reused caches and RTSS has run successfully, including a Story-mode DLSS/FSR2
census. RSF SR has live pixel evidence for DLSS, FSR3, FSR4 INT8 and XeSS; the persistent NGX
ownership correction survives Pause/Settings after switching. AD accepts normal SDR DLSS FG2x,
4x, Off/resume and SR switching with FG active after a guarded loading-token diagnostic. The
permanent retirement fix is built/verified and hash-matched into the installation with diagnostics off.
The engine resolution producer, complete HUD modes and native graphics-menu replacement remain open.

| Item | Value |
| --- | --- |
| Plugin ID | `rdr2` |
| CMake targets | `rsf_game_rdr2`, `rsf_asi_rdr2` |
| Outputs | `ReScaleFrame.Game.RDR2.dll`, `ReScaleFrame.RDR2.asi` |
| Executable | `RDR2.exe` 1.0.1491.50, Steam build 13773296 |
| SHA256 | `b56c9548f670654a9b73bf25def3cd73af12e269f6e47dba28a34079adaf465e` |
| Engine | RAGE, from executable strings |
| Rendering readiness | `0` |

Detection requires the executable basename, x64 PE machine and exact hash, with ASCII case
insensitivity for the name and hex digest. A different build is unknown. Recognition establishes
identity only.

## Source ownership

`rsf_game_rdr2` links the Rust crate `rsf-rdr2` (`Cargo.toml`, `src/*.rs`) as a static library.
The crate owns the plugin policy: recognition and lifecycle validation (`lib.rs`), fingerprints,
RVAs, expected entry bytes and native layouts (`layout.rs`), the worker-view camera decode
(`camera.rs`), DLSS/FSR2 SR routing, refusals, queue/Present observers and hook lifecycle
(`renderer.rs`), and the completed-scene producer with its native marker, sleep, mode and input
facts (`scene.rs`). `src/native_platform.rs` keeps only typed leaves the crate calls through a
table: MinHook, D3D12/DXGI COM calls, ReadProcessMemory, TLS, QPC, window-message and XInput
observation, and return-address capture. `src/lib.rs` supplies the `rsf_get_game_plugin_api` export.
The `rsf_rdr2_camera` fixture and capture-file CLI share the Rust camera decoder and the
versioned synthetic fixture parser.
[The port note](../../docs/research/rust-rdr2-plugin.md) records what moved, the deliberate
differences and the remaining work.

The ASI (`rsf_asi_rdr2`) links the same crate for its startup slice: `asi.rs` owns process
admission, the `[RDR2]` configuration gates read under the loader lock, the ForceD3D12 parameter
write, the D3D12 trace guard and the order in which the startup owners are armed, and
`engine_settings.rs` owns the settings-collection carrier, refresh-rate storage and the
ScriptHook quality request. Rust owns the Win32 startup leaves, ASI and research-attach
workers, settings probe, DXGI adapter discovery and diagnostic adapters. `src/asi_entry.c`
binds the linked capture and diagnostic services through `asi_platform.h`. The Social Club
diagnostic calls the borrowed swapchain through the SEH leaf in `social_club_guard.c`.
`runtime_setup.h` declares the
C bridge also used by the Unity D3D12 fixture. The engine-settings vector suite lives in
`src/engine_settings/tests.rs`.
[The ASI note](../../docs/research/rust-rdr2-asi.md) records that slice.

## Deployment

RDR2 modding already ships an ASI loader (ScriptHookRDR2's `dinput8.dll` or Ultimate ASI
Loader), so this game does not use a forwarding system-DLL carrier. `ReScaleFrame.RDR2.asi` is an
ordinary module the loader calls `LoadLibrary` on. In `RDR2.exe`, its `DllMain` applies the guarded
API override and optional census before renderer creation; a worker loads the runtime and runs
the orchestrator's plugin-session lifecycle outside the loader lock. The ASI does not manipulate
the game's cache files.

The directory holding the `.asi` is the deployment root:

```text
<asi directory>/ReScaleFrame.RDR2.asi
<asi directory>/ReScaleFrame.ini                  optional; [RDR2] Enable=0 disables startup
<asi directory>/ReScaleFrame/ReScaleFrame.Runtime.dll
<asi directory>/ReScaleFrame/ReScaleFrame.Game.RDR2.dll
<asi directory>/ReScaleFrame/streamline/*.dll
<asi directory>/ReScaleFrame/fidelityfx/*.dll
<asi directory>/ReScaleFrame/xess/*.dll
<asi directory>/ReScaleFrame/rdr2.log             written by the entry
```

The `.asi` lands in `build/windows-x64/rdr2/bin/<Config>/`; the runtime and plugin DLLs land in
`build/windows-x64/bin/<Config>/`. The measured loader entry is after decryption, during CRT
startup and before renderer creation. `[RDR2] Census=1` enables the vendor probe;
`TraceD3D12=1` logs failing game HRESULTs to `ReScaleFrame/rdr2-d3d12-errors.log` without changing
the results. Both diagnostics default off and, with the F10 frame capture (`NativeCapture`), the
camera and native-settings probes, module dumps and the RenderDoc frame probe, exist only in
developer builds configured with `-DRSF_RDR2_RESEARCH=ON` (the crate's `research` feature).
Release builds ignore those keys.

## Target

DX12 is forced when RSF is installed, so the D3D12 runtime serves every provider: DLSS SR/FG/MFG,
FSR3/FSR4 SR and FG, and XeSS SR/FG with XeLL. The intended adapter uses the game's DLSS-shaped
inputs for every RSF backend; both native DLSS and FSR selections must route there. This routing
is implemented in source and awaits live acceptance. Its native Reflex markers are to be translated to the selected vendor's
latency API. Vulkan is not a target. [The research note](../../docs/research/rdr2-renderer.md)
records the plan, the evidence so far and the open questions.

`[RDR2] SR=0` retains native evaluation; `SR=1` enables replacement. `SRBackend=1/3/4/5/7`
selects DLSS/FSR3/FSR4/XeSS/Auto (the default, resolved by the orchestrator from the owning adapter), and `DLSSPreset` uses the shared preset IDs. These settings are
polled once per second. Omit `SRQuality` to follow the game's active render ratio. Its override
currently changes vendor mode only; changing the engine's render-resolution producer and
replacing the native options rows are pending. Context changes wait for submitted GPU work
and refuse while their recordings are still unsubmitted. The process owns installed hooks
and runtime contexts until exit. Recognition and GPU-fixture checks do not imply game acceptance.

New source also polls `FG=0/1` (Off/Fixed), `FGBackend=1/3/4/5/7`
(DLSS/FSR3/FSR4/XeSS/Auto), `FGGeneratedFrames`, `Latency=0/1/2` and `FrameLimitUs`.
These controls require the cold-start shared presentation path. Its hidden-window fixture
checks tagging, native marker translation and submission identity; it does not prove generated
frames by itself. AD adds game activity and exact normal SDR HUD-boundary pixel evidence;
broader image quality, visible overlay, other modes and physical latency remain unverified.

Build and run the contract checks from the repository root:

```powershell
./eng/verify.ps1 -Configuration Release -VS2026
```

The contract test loads the plugin in a test process, verifies detection, ABI refusal, inactive
status and cleanup, and does not modify or launch the game.

[Engine evidence](engine.json) records measured facts and leaves unmeasured fields null.
[Current status](../../docs/current-status.md) compares this scaffold with the implemented paths.
