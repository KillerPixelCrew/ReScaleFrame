# Anomaly frame-generation stage in Rust

The cut starts from the merged Anomaly adapter at `9d53556`. The staged
`8322179a36c7` adapter supplies the PE export baseline and the earlier
game evidence. [Engine fingerprints and source revision](../../games/anomaly/engine.json)

## Engine facts and resource ownership

| Boundary | Evidence carried through the port |
| --- | --- |
| Cold startup | Rust reads and admits the existing INI options and validates D3D11 FG API 3 and Present API 1. It looks up both queries on the carrier's borrowed Runtime. It installs the bridge once before device creation and publishes the overlay link only on success. Auto and the requested provider settings go to the Runtime for resolution. |
| Input and simulation | Verified `CInput::OnFrame` still reports the actual sample and simulation begin. Buffered receipts and delivered window messages use the open source ID, with source zero for window messages outside that frame. A new input update closes a preceding skipped render. The original render Begin reports the simulation end and reserves an identity when no input callback ran. The Runtime owns substitutes, deduplication, pacing and marker mapping. |
| Camera and SR | Rust latches the unjittered row-vector view/projection at render Begin and applies the existing 4 m / 45° cut rule. The FRAME, SUBMISSION and SR source scope carry the same session and source ID. Stage 5 supplies the normalized inputs and camera from its own SR pass. Stages 2 through 5 retain ownership of world depth and can leave FG inputs unavailable. |
| SR-off inputs | The native leaf reads the selected profile's `HW.pBaseZB` (Standard `0xceb8f0`, AVX `0xcd98f0`). Rust holds a local depth texture reference and admits a written main-view RTV2 at matching dimensions. The existing input pass copies standard D24S8 depth to R32_FLOAT and converts valid RTV2 vectors to previous-minus-current render pixels in R16G16_FLOAT. Its shader uses the same previous-clip transform and first-person depth rule. The pass keeps its output leases live through Runtime capture. |
| HUD-less colour | At entry to `CHUDManager::RenderUI`, the scene includes post-processing, world-space item UI and tracers at output resolution. The adapter holds the owned immediate-context and backbuffer references through the paired FINAL_SCENE calls and diagnostic dump. The shared overlay still draws in the bridge's before-Present callback. |
| End, Present and reset | End closes SUBMISSION and clears the SR association before opening WINDOW. The bridge forwards its actual Present and latency callbacks to the Runtime. End closes the accepted WINDOW and FRAME scopes before publishing the status and accepted camera history. The SR-off input owner drops before the original `CRender::reset_begin` runs. Provider retirement and resize remain at the existing Runtime and native boundaries. |

The existing Standard and AVX byte guards, the shader code and the engine
producer owners are shared with the baseline. The scope, menu, pause and
post-process paths keep their prior classification and refusal. This cut
used the recorded shader inventory and source inspection. Fresh active
bindings and motion coverage, including PDA and post-process UI paths,
require a game capture. [Anomaly FG evidence](anomaly-fg.md),
[AVX profile](anomaly-avx-profile.md), [C++ game runs](rust-anomaly-cpp-baseline.md)

## Corrected history admission

The old HUD-less handoff updated the policy with depth/motion readiness
before `GetBuffer` and ignored both FINAL_SCENE return values. A successful
Present could then count a source as fed and update its previous camera
even if the code never obtained a HUD-less surface. Rust now requires
both inputs and acceptance of the paired scene passes before the source
can advance adapter history. A rejected handoff takes the existing
`no_inputs` path. The fake-table regression refuses a missing backbuffer
despite ready SR inputs and a successful Present. The next accepted
source still carries RESET. The same test checks the exact pass order,
source and submission IDs, and buffer and context release order.

## Input staging target ownership

The SR-off input pass kept its six staging objects in a native `Targets`
bundle that Rust held as one opaque lease. `fg_inputs.rs` now holds each
object as its own `Lease`. This is a source move with no new experiment.
It started from `aa66153c`.

| Item | Before (`fg_inputs.cpp`) | After |
| --- | --- | --- |
| Creation order | `make_targets`: R24G8_TYPELESS copy (SRV bind), R32_FLOAT depth and R16G16_FLOAT motion (SRV and UAV bind), then the R24_UNORM_X8_TYPELESS depth SRV and both default UAVs | `SurfaceSet::create` makes the same six calls in the same order. Rust passes the DXGI format and bind mask; the native leaves keep the typed D3D11 descriptors |
| Partial failure | `unique_ptr<Targets>` destroyed the created `ComPtr` members in reverse declaration order | Each `?` drops the leases created so far, newest first |
| Complete set | `release_targets` deleted the bundle: `motion_uav`, `depth_uav`, `depth_view`, `motion`, `depth`, `depth_copy` | `SurfaceSet` declares its fields in that order |
| Resize failure | The old bundle stayed leased but unpublished | Unchanged. The new set is assigned only once complete, and the older set drops after that |
| Publication | Bundle returned raw depth and motion pointers | `outputs()` borrows from the depth and motion leases and still requires a dispatch accepted this frame |
| Dispatch | Cast the opaque bundle | Receives a borrowed `rsf_anomaly_input_targets` with all six handles. The shader text, binding slots, predication clear and state restore are unchanged |

The private platform table replaces `targets` and `release_targets` with
`texture`, `depth_view` and `unordered_view`. Each returns one reference
released through the existing `release_interface`. The table grows from
88 to 96 bytes and the borrowed targets struct is 48 bytes. Both headers
and the Rust layout checks assert these sizes. The pipeline, motion SRV,
device admission, shader-failure latch and every refusal result are
unchanged. The FG stage still carries the table by pointer. The public
GameSDK and the Runtime boundary do not change.

The merge reproduced the binding byte for byte with the focused
bindgen 0.72.1 recipe. The fake platform in `fg_inputs/tests.rs` now
records creation and release by object and generation. It covers a full
set, a failed `motion_uav` create that releases only the five partial
objects, the older set staying live through that failure, and the release
order on replacement and owner drop. The checks were pinned `rustfmt`,
locked offline `cargo metadata` and `git diff --check`. Native builds,
Clippy, the Rust tests, device fixtures and game runs were not run.

## Input pipeline ownership

Compared with `8f5d0b49`, the Rust FG input owner now leases the compute
shader and constant buffer individually. It compiles the shader before
allocating the 96-byte dynamic buffer. A refused buffer allocation
releases the new shader and keeps the existing per-device shader
failure latch. On retirement the constant buffer drops before the shader.
The native call borrows a 16-byte record for the synchronous dispatch.

The HLSL bytes, shader entry, buffer descriptor, successful recording
order and public ABI match the previous source. A successful Map with
a null address now unmaps, restores the saved D3D11 state and refuses
the dispatch. The narrow binding regenerated identically with bindgen
0.72.1. Pinned formatting, locked Windows Cargo metadata and whitespace
passed. The partial-allocation regression and device and game checks
remain unrun.

## Validation

MSVC 19.52 x64 Debug and Release built the adapter, the FG and Stage 5 SR
device fixtures, and their focused dependencies in `build/anomaly-fg`.
Each configuration passed 15 CPU contracts, including the Standard and
AVX SR and input hooks, input events through the Runtime table, lifecycle,
cold entry, FG policy and the SR and overlay API contracts. The device
fixtures were built for subsequent device validation.

Both adapter DLLs matched the staged `8322179a36c7` DLL by export name and
ordinal. The exports are `1 rsf_anomaly_observation`, `2 rsf_anomaly_start`
and `3 rsf_get_game_plugin_api`. PE imports and lookup strings confirmed
that the adapter borrows `ReScaleFrame.Runtime.dll` and resolves the FG
and presentation queries dynamically.

Bindgen 0.72.1 and libclang 18.1.1 regenerated the private binding byte
for byte. Formatting and workspace Clippy passed with Rust 1.100.0.
All 223 locked workspace Rust tests passed, including the new settings,
camera and HUD-less refusal regressions.

Device execution, real-world borderless placement, vendor recovery after
resize, moving-game output and measured latency remain unverified for this
revision. Scope hold, capture and development diagnostics use cold INI
settings. The panel continues to expose the bridge and shared frontend
settings.

## Standard game checkpoint

On `9b304d1`, a separate copy of the Standard 1.5.3 game loaded the
existing autosave on an RTX 4070 Laptop GPU. The executable hash matched
the Standard profile. The test used the merged adapter and Runtime,
the baseline carrier and panel, and the matching motion pack. It ran
windowed at 1280×720 with `SRStage=0`, `SR=0`, motion enabled and DLSS-G
requested. The installed game and the earlier C++ baseline were untouched.

The adapter reported 21,930 level frames with HUD-less tags and accepted
FG inputs, and zero `no_inputs` refusals in its periodic windows. The motion
stage reported successful variant cache hits, paired shader draws and
stock-shader restores. DLSS-G reported an effective requested mode and
`active=0` throughout the saved status records. This verifies the native-size
handoff in game. Generated presentation and live image comparison are
still open. VSync was enabled and the game remained in the background.

The saved checkpoint contains 182 steady reporting windows with 120
game frames and 120 SDK presents each. The earlier Standard run contains
23 windows with 120 game frames and 240 SDK presents. The DLSS backend
queries `numFramesActuallyPresented` after Present and uses an OK result
with a count greater than one as its active signal. The two runs differ
in SR and window state. Their logs establish the presentation difference
and leave its cause open. [NVIDIA Streamline guide](https://github.com/NVIDIA-RTX/Streamline/blob/main/docs/ProgrammingGuideDLSS_G.md#130-how-to-obtain-the-actual-frame-times-and-number-of-frames-presented)

Windows refused the foreground transition before input was delivered.
The game ignored a window close request, so the test harness stopped its
own disposable process. Its exit code was `-1`. Overlay open/close, input
resumption and normal teardown remain open on the migrated adapter.
Artifact hashes, configuration and logs are in
`.local/anomaly/staging/live-rust-fg-overlay/evidence/`.

## N08 source follow-up

This follow-up starts from the integrated N06 seam at `d104f2dd`.
The engine producers and shader audit are the ones recorded above and in
[Anomaly FG evidence](anomaly-fg.md). The port keeps the HLSL bindings and
math, the Standard and AVX depth locations, and the matching main-view
motion admission. The SR-off shader still uses depth reprojection where
object motion is unavailable, with the existing first-person rule.
World-space UI and post effects remain part of the HUD-less colour at
the observed UI boundary. Their motion coverage needs active captures.
The adapter keeps the normalized input textures live through the
synchronous Runtime handoff. The Runtime still owns the shared copies,
GPU fence, slot retirement, provider selection and presentation.

| Rust review finding | Source resolution |
| --- | --- |
| Distinguish a constant-buffer allocation failure from a permanent shader refusal. | A failed constant buffer reports Resources, releases the partial shader lease and retries on the same device. A shader refusal still latches until device replacement. The input fixture now expects a successful retry. |
| Count tag eligibility separately from completed tagging. | Adapter counters report `eligible_at_ui_boundary`. `runtime_tagged` comes from completed Runtime tags. The missing-backbuffer fixture observes an eligible frame with zero Runtime tags and no history advance. |
| Anomaly FG keeps validated Option tables and unwraps them at every call. | Startup checks the readable size prefixes, Runtime table versions and required leaves once. The stage and input owner call typed function pointers. A fake table covers short stage and input tables, a missing input callback and a missing Runtime capture callback. |
| Accept FG input resources without requiring a DLSS SR frame. | D3D11 FG table version 4 accepts a size- and version-checked texture pair. The SDK pass supplies dimensions and camera. The Runtime translates into the existing transfer owner's capture call, also used by the established SR path. C and C++ ABI clients cover version and size refusal. |
| Keep inferred frame closure out of simulation timing events. | A new input update sends only skipped frame closure for the preceding source. The Standard and AVX Runtime CPU fixture paths check that the source ended with no simulation-end event or timestamp. |
| Share X-Ray window identification across the overlay and FG bridge. | Both use the overlay's game-local process/class predicate. The FG callback owns subclassing and placement correction. |
| Tie the cached subclass registration to the window lifetime. | The callback queries the live subclass association and the subclass removes itself on WM_NCDESTROY. |
| Anomaly FG options round-trip INI values through the native config and several string encodings. | INI and direct fixture admission use the same Rust settings. Vendor paths follow the N06 UTF-16 normalizer and one final conversion to the Runtime encoding. |

The portable policy fixture now runs through the Rust owner. Its old
C++ facade and CTest executable were removed. The native policy header
used for binding generation retains just its enums and records.
The typed DXGI immediate-context, backbuffer, output-window and depth
texture probes moved to Rust. Native stage code supplies the guarded
engine depth read, the current motion producer and the existing C++
carrier entry points. The D3D11 input shader, resource creation and state
scope leaves in `fg_inputs.cpp` remain native. The real device fixture
retains the C++ D3D11 scene/Present and SR TemporalDriver scaffolding and
now checks the revised CPU event contract.

The separate N02 checkpoint `9f8df413` also updates the FG capture
timestamp. When integrating, retain this Rust settings admission and use
N02's shared `capture_clock::local_timestamp` for the default capture
directory if resolving the options hunk. The overlay predicate visibility
change applies to the same predicate in N02.

Checks were source comparison against the Windows SDK bindings, pinned
Rust 1.94.1 formatting, repeat bindgen-cli 0.72.1 regeneration with
libclang 18.1.1, locked offline Windows Cargo metadata and Git whitespace
checks. Builds, lint, Rust and native fixtures, and device/game runs
remain user-owned. Window recreation, provider output and motion
coverage after the port require execution evidence.


## N08 typed D3D11 and device fixture port

The source baseline is `031aa9ac`. The earlier FG port at `410a672c`
provided the existing Rust stage, policy and typed DXGI probes. This
pass moves the remaining D3D11 input recording and fixture scenarios
into that adapter. The engine and shader producer evidence remains in
[Anomaly FG evidence](anomaly-fg.md).

| Source path | Disposition |
| --- | --- |
| `games/anomaly/src/fg_inputs.cpp` | Removed. `fg_inputs/d3d.rs` creates the shader, constants, textures and views and records the pass using typed D3D11 interfaces. `shaders/fg/rsf_fg_inputs.hlsl` contains the extracted shader body. |
| `games/anomaly/src/fg_inputs_platform.h`, `fg_inputs_abi.rs` | Removed. The allocation and recording bindings are private to Rust. The stage receives only the generic state save/restore callbacks alongside the existing engine facts. |
| `games/anomaly/src/fg_policy_platform.h`, `fg_policy_abi.rs` | Removed. `fg_policy.rs` owns the decision and counters and uses the GameSDK screen constants. `fg_policy.h` and `tests/anomaly_fg_policy.cpp` were already absent at the baseline. The Rust policy fixture continues to call the same owner. |
| `games/anomaly/src/fg_stage.h`, `fg_stage_platform.cpp`, `fg_stage_platform.h` | Retained for the carrier and native observer. Cold start supplies the exact profile's guarded depth RVA, SR reduced-depth ownership and current N04 motion target. The fixture-only native Options, install, depth-source override and status interfaces were retired. `fg_stage/options.rs` admits typed Rust settings from both INI and fixture callers with the existing N06 path normalization. |
| `tests/anomaly_fg_device.cpp`, `tests/anomaly_fg.cmake` | The executable is a thin launcher and still links the Runtime DLL. It passes native profile facts and explicitly imported CPU ledger functions. The `fg-fixture` Rust feature owns both CPU contracts and all four registered device scenarios, including adapter selection, window and swapchain creation, SR stage 5, texture allocation, draw/Present order, resize, switching, assertions and skip decisions. |

The FG input pass keeps the immediate context and canonical IUnknown device
identity checks. It copies the D24S8 resource to an R24G8 typeless
texture with an R24_UNORM_X8 view. The full-size R32F depth and RG16F
motion retain their SRV/UAV bind flags. Constants remain 96 bytes with
the same reprojection, motion validity and first-person threshold. The
pass saves state before mapping, unmaps any successful map, clears
predication, records the copy and dispatch, unbinds CS SRVs and UAVs,
and restores state through the existing generic scope on every exit
after a save. N07 SR restoration remains in its existing owner. The
stage only publishes a successful input pair for the synchronous
Runtime handoff. Failed allocations remain retryable, and a shader
refusal remains latched for that device.

The source review compared shader text and D3D11 calls to the C++ body
and the checked-out Windows bindings. Bindgen 0.72.1 with libclang 18.1.1
regenerated the stage and fixture ABI records. Rust 1.94.1 formatting,
locked offline Windows Cargo metadata with `rsf-anomaly/fg-fixture`,
and Git whitespace checks were the verification for this pass.
Build, fixture execution and game/device acceptance remain unverified.
