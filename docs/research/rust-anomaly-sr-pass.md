# Anomaly SR pass construction in Rust

The port starts from `eb3d579`. The Stage 5 seam now gives the Rust
temporal driver the camera and jitter facts already collected by the Rust
stage driver, paired with the native leaf's borrowed texture pointers. The
temporal driver selects the reset reason, builds `rsf_game_render_pass`, and
submits it to the same synchronous Runtime evaluation function. It also
validates a backend's planned render size with the existing Rust plan
function directly. `sr_pass.h` keeps the native fixture API as a facade over
the production Rust builder. The stored-motion decode reference remains
available to fixtures that compare the Runtime's GPU decode with CPU values.
[SR seam evidence](anomaly-sr-seam.md#12-stages-4-and-5-implementation-sr-inputs-and-temporal-sr-at-the-seam),
[SR stage audit](rust-anomaly-sr-stage.md),
[SR temporal audit](rust-anomaly-sr-temporal.md)

The guarded reads still take `Device.mView` and the unjittered
`Device.mProject` at the main-view H1 before raster jitter, for both verified
Anomaly 1.5.3 profiles. SR and FG now use one X-Ray camera implementation for
planes, pose and clip mapping. It preserves positive view Z, standard finite
depth, and row-major storage with row vectors. It derives near, far and
vertical FOV from `_33`, `_43` and `_22`, and computes
`inverse(V * P) * (previous_V * previous_P)` and its inverse in double
precision. SR maps a reset or unavailable previous camera to the current
camera. Current and previous applied jitter still travel in render pixels
with positive X right and positive Y down. The SR cut compares consecutive
main-view poses in metres and degrees with the configured thresholds. The
reset order remains first, device, settings, sequence, then cut. The Runtime
still handles its own size, rectangle, session and frame-continuity resets.
[FG camera audit](rust-anomaly-fg-stage.md)

The existing D3D11 owner keeps the resources live through evaluation, including
the depth twin and prepared motion. The native seam supplies an optional
exposure pointer and the motion-coverage fact separately. The SDK layout,
resource identities, refusal order, output reinsertion and bilinear recovery
use the same contracts. The prior shader and buffer inventory still applies
because the port uses the same producers, shaders and native resource leases.

## Verification

Worktree-local MSVC 19.52 x64 builds used the coordinator's pinned MinHook
`c3fcafdc10146beb5919319d0683e44e3c30d537` and vendor inputs. The first
configure failed the downloaded yyjson hash check. The existing pinned
copy then passed the repository's manifest checks. The installed Rust
MSVC toolchain was 1.100.0. All 271 locked workspace tests passed, including
the FG camera contract and an H1 test for the exact cut distance, a larger
move, and an invalid camera. `cargo fmt --check` and locked workspace
Clippy with all targets and warnings denied passed. Bindgen 0.72.1 reproduced
the private bindings byte for byte.

Debug and Release each passed 27 focused CPU contracts, including pass,
reconstruction, jitter, Runtime SR API, swap and the Standard and AVX hook
and input fixtures. The input-event contracts used the CPU-only dispatch
with a fake installer. The SR round-trip, jitter, temporal and FG device
modes were build-only. Live SR behaviour in both profiles still needs
verification in the game.

A worktree-local CPU comparison compiled the `eb3d579` header alongside
the Rust facade in Debug and Release. It compared 1,000 cases with changing
poses, FOV, far plane, jitter and reset, plus singular history and refusal
inputs. Pass decisions, populated SDK fields, matrix elements and cut
decisions matched. The checked-in native pass contract also verifies that
a refused pass clears its lease pointers and that a reset discards a
singular previous camera while retaining the previous applied jitter value.

`dumpbin` compared both adapter DLLs with the `8322179a36c7` staged DLL
(SHA-256 `b978d99e2506d4d4864d267a109cd354a31e834c8e420d27dba4d8e9031fdca5`).
Both builds match the staged DLL's `rsf_anomaly_observation` at ordinal 1,
`rsf_anomaly_start` at 2 and `rsf_get_game_plugin_api` at 3. They retain
the UTF-16 `ReScaleFrame.Runtime.dll` name, the loader lookup imports
and all 16 by-name Runtime lookups, including the SR table, colour transport
and input dump functions. The Runtime is borrowed dynamically. The staged
Release imports for CRT conversion and time are absent, consistent with the
earlier SR and FG stage ports (`3c4b0d0` and `f7a0f83`).

## Offscreen device acceptance

The merged MSVC Release build at `58fe95acdca6` passed the following
offscreen fixtures. The staged loader, Runtime, adapter, panel and
motion pack have SHA-256 receipts. Fixture and vendor hashes and full
logs are under `.local/anomaly/staging/58fe95acdca6/device-evidence/`.

| Fixture | Device evidence |
| --- | --- |
| Round trip and spatial seam | Intel UHD hardware. The exhaustive FP16 round trip stayed within one code value. The seam preserved position and depth bit-for-bit at the tested extents. Valid bindings produced zero debug-layer errors. The deliberate mismatched RTV/DSV controls produced the expected errors. |
| Jitter | Intel UHD hardware. The static scene measured 0.0033 render pixels of residual motion over 93,312 pixels and 18 phases. The pan and uncorrected jitter controls passed. |
| Decode, thin motion, prepare and reinsert | RTX 4070 Laptop GPU. All three cases passed with zero debug-layer errors. Valid zero and missing motion remained distinct, the thin mover and edge checks passed, and the reinsertion stayed within one UNORM8 code. |
| Vendor evaluation and switching | RTX 4070 Laptop GPU. DLSS, DLSS with automatic exposure, FSR3, FSR4, XeSS and the return to DLSS each evaluated 24 of 24 frames. Pipeline and pass refusals were zero. The checked outputs contained no non-finite pixels. |

The offscreen DLSS fixture reported that Streamline did not observe
`presentCommon`. This run covers SR evaluation and switching with real
textures and vendor DLLs. In-game presentation, input, overlay interaction
and plugin teardown still require game evidence. The fixture reads and
removes its temporary comparison images as part of the run.

## Scripted Anomaly 1.5.3 runs

Standard and AVX used the same save, MSVC Release artifacts from
`58fe95acdca6`, DLSS Quality with automatic exposure and DLSS-G 2×
requested. Both used a disposable copy of the game. The first attempted
Standard run retained `keypress_on_start on`. The actor never entered its
update callback and the runner stopped that process at its time limit.
The subsequent runs set `keypress_on_start off` in the test copy only.

A test script registered an actor update callback and queued a display
resize and a normal engine quit after the save loaded. This exercised the
game's command and teardown paths. The script sent no keyboard or
mouse events. [Anomaly callback reference](https://github.com/TheParaziT/anomaly-modding-book/blob/main/docs/tutorials/scripting/callbacks.md),
[X-Ray console binding](https://raw.githubusercontent.com/themrdemonized/xray-monolith/028ccf5f5189b393396480076b11e40852ba3ecd/src/xrGame/console_registrator_script.cpp),
[X-Ray quit command](https://raw.githubusercontent.com/themrdemonized/xray-monolith/028ccf5f5189b393396480076b11e40852ba3ecd/src/xrEngine/xr_ioc_cmd.cpp#L63-L74).

Both profiles reported 569 DLSS evaluations and zero refusals in the
last periodic SR status. The resize completed once on each profile.
Native rendering changed from 1280×720 to 1600×900, and the DLSS
input changed from 853×480 to 1067×600. Both logged level destruction
and Direct3D shutdown, and exited with code 0. Streamline logged that
the underlying swap chain still had one reference, as it had in the
C++ baseline. Explicit plugin unload and GPU resource retirement were
not measured.

Each profile saved SR color, depth, motion and output with the final
color. The bundle identified frame 151 as generation 1 and frames 392
and 632 as generation 2, all on the main view. The capture arm was
recorded before evaluation, so its `frame.json` had no camera and left
the backend at 0. The synchronous pipeline sidecars, named by the
matching frame IDs, recorded the actual backend, jitter, render and
output extents, motion scales and clip-to-previous matrix. The bundle
header and the vendor inputs remain separate records.

Both profiles submitted HUD-less color, depth and motion to the FG path
and presented through it. The vendor reported one display present per
source present and `active=0`, so these runs did not demonstrate
generated frames. The `standard` and `avx` evidence directories under
`.local/anomaly/staging/live-58fe95a-stage5/evidence/` hold the two log
sets, input settings, exit status and capture bundles.

The Standard comparison used the original C++ adapter from
`8322179a36c7` with the same merged Runtime and the same game executable,
settings, script and autosave. The engine registered the script's actor
callback but never reached it. SR evaluated and FG accepted level frames
at the original resolution, with `active=0` throughout. The runner stopped
the isolated process at its time limit. This comparison reproduces inactive
FG with the C++ adapter on the merged Runtime. It does not isolate the
cause or reproduce the successful resize and shutdown. The inputs and logs
are in `evidence/standard-cpp-adapter/` beside the Rust runs.

The foreground Standard run with the Rust adapter used the same build,
save and resize. DLSS-G activated before the resize and resumed after it.
In steady windows the Runtime recorded 240 vendor presents per 120
source frames. The HUD showed about 60 rendered and 120 presented fps.
Insert opened and closed the panel. Its first opening drew all 2,452
visible frames, consumed pointer events, and applied and saved three
sharpening mode changes. A brief Escape press after closing the panel did
not open the game menu, so this run did not prove that gameplay consumed
keyboard input after the overlay returned it.

Switching SR off kept the native image visible and suspended FG. The
next complete window reported 120 level frames, zero feeds, and 120
missing-input skips. The engine then quit with code 0. Evidence is in
`evidence/standard-foreground/` beside the scripted runs.

The SR-off gate used the configured stage number, which assigned depth
to the SR pass even when Stage 5 left the real world targets installed.
The stage now records the native frame at main-view prepare if the swap
stayed closed with SR disabled. FG may use its existing native depth
and matching motion path only for that frame. The stage clears this
eligibility at frame end and when stopped. A regression covers scope,
stale frame IDs, frame end, enabled SR and an actual swap.

The fix at `4fb67e498606` passed a foreground AVX run. After resizing to
1600×900 and turning SR off in the overlay, one transition frame lacked
FG inputs. Subsequent windows each fed 120 of 120 native frames with
240 vendor presents. Re-enabling SR resumed DLSS. The same run switched
to FSR3, FSR4 and back to DLSS with zero reported SR evaluation refusals.
The first FSR4 selection took 31,874 ms at the frame boundary, matching
the cold pipeline compilation path announced in the log. XeSS switching
was verified offscreen. The AVX game exited with code 0.
The logs are in `evidence/avx-foreground-sroff/`.

The forced-recovery Standard run used `SRJitter=0` on the same build.
The pass rejected the unjittered camera, and the spatial seam drew
bilinear output on each rejected frame. The scene and HUD rendered
at both output sizes. The four final-color captures
include one 1280×720 and three 1600×900 frames, with distinct generations
before and after the resize. The game camera reacted to mouse movement.
Dragging in the open overlay moved the pointer while the world view
stayed still. The game then exited with code 0. The logs and images are
under `evidence/standard-forced-fallback/`.

That run also exposed 2,025 failed DLSS resize attempts in the last
sample. The world continued at the new extent through spatial
recovery, while the DLSS pipeline retained its original output size.
The shared D3D11 pipeline tried to release the DLSS viewport even
though no frame had reached its evaluation entry. The Rust owner
now skips that release before the first attempted evaluation. It still
requires a successful release after an attempt, since even a refused
evaluation can leave SDK state. A focused regression injects a release
error and checks that the first resize succeeds and a later resize
keeps its prior output until release succeeds.

The Standard retest at `26e358b1222f` reproduced the jitter refusal
and completed the same resize before any DLSS evaluation. The
pipeline reported one attempt and one success, with a 1067×600
render plan for 1600×900. Bilinear recovery drew throughout, four
final-color captures were saved, and the game exited with code 0.
The overlay jitter control was not changed in that run. Evidence is
in `evidence/standard-cold-resize/`. The Runtime and embedded AC7,
Loader and Minecraft DLLs retained their recorded export inventories.

## Capture identity at evaluation

The earlier live captures reserved their frame at main-view prepare.
Their headers carried dimensions and resource generation, but lacked
the camera and left the backend at zero. The provider sidecar carried
the actual jitter and backend under the dump prefix. The Stage 5
evaluation now supplies the camera and effective backend to the
existing capture result call after a successful evaluation, for the
same guarded engine frame, resource generation and dimensions. The
leased pass now reports the guarded engine frame counter where it
previously used the index of the jitter publication.

The Runtime accepts this metadata only for the selected frame and
before its matched Present. It fills fields missing from the header
and retains any already recorded camera or backend. The linked and DLL
CPU capture contracts reject a failure, stale generation and wrong
dimensions, then verify the accepted camera and backend in the header.
The Stage 5 fixture checks the engine frame passed to evaluation.

The thin carrier retains its Runtime and adapter references for the
process lifetime. Anomaly pins its adapter once native trampolines are
installed. The lifecycle contract covers callback retirement through
quiesce and stop, and the game runs cover normal engine exit. No
live module-unload path is exposed by the thin carrier.

The foreground Standard retest at `ff89e18f0362` captured frames
151, 392, 632 and 872. Frame 151 used generation 1 at 853×480 to
1280×720. The later frames used generation 2 at 1067×600 to 1600×900.
All four headers report the camera and DLSS, match the corresponding
pipeline sidecars for dimensions, backend and jitter, and have SR
result records with matching session and frame IDs. The last status
sample reported 4,169 successful evaluations, zero refusals and one
successful resize. Steady windows reported 240 vendor presents per
120 source frames. The game exited with code 0.

Insert opened and closed the panel. The panel consumed two key events
during the Use-key test and drew all 842 visible frames. After it
closed, mouse movement turned the camera in the world. Short Use,
Inventory and Escape presses did not produce their expected gameplay
actions or a matching game key receipt. The game script observed
input through the existing callback and submitted no engine events.
The settings, logs, script and metadata comparison are in
`evidence/standard-capture-keyboard/`.
[Anomaly key callback reference](https://github.com/TheParaziT/anomaly-modding-book/blob/main/docs/tutorials/scripting/callbacks.md).

## Shared capture binding

The motion and SR stages now resolve the existing frame-dump exports
from one Rust binding. Both MSVC configurations preserve the Anomaly
DLL's three exports and its dynamic Runtime lookup. The SR hooks,
motion and Runtime API CPU contracts passed.

The isolated Standard run at `57c66151ff65` enabled both capture
options. The log recorded that SR owned the service. It saved DLSS
frames 151 and 392 with camera metadata and resource generations 1
and 2. The isolated AVX run enabled motion capture alone and saved
the motion, motion preview and G-buffer position for frames 152 and
392. Both profiles resized once to 1600×900 and exited with code 0.
These runs are in `evidence/standard-shared-dump-sr/` and
`evidence/avx-shared-dump-motion/` under the same isolated copy.

The AVX bundles initially reported generation zero after the reset.
The motion collector already receives the observer's reset counter
and checks it before copying. The frame header now carries that
same value. The focused motion capture test verifies that the
reservation receives it.

The AVX retest at `da58f43a56df` saved frame 151 at 853×480 with
generation 0 and frame 392 at 1067×600 with generation 1. Both bundles
contained motion and G-buffer position, and the game exited normally.
The test restored the isolated INI after exit. Its captures and
comparison are in `evidence/avx-shared-dump-generation/`.

## Live provider switches

An isolated Standard run of `8ccee0311c2f` switched from DLSS to
XeSS Quality at 942×530 to 1600×900. XeSS evaluated 120 of 120
frames in steady windows. The frame-generation overlay switched
from DLSS-G through FSR3 and XeSS, then disabled FG and returned to
DLSS-G while XeSS SR kept running. FSR3 and XeSS each reported active
2× generation in live windows. An Off sample showed one display
present per rendered frame, and DLSS-G returned to 2×. The last
periodic sample reported a cumulative 6,209 SR evaluations with zero
refusals. The process exited with code 0. Logs and settings are in
`evidence/standard-provider-switches/`.

The transition windows exposed a diagnostic underflow. The report
subtracted the preceding provider's total from the new provider's
smaller total and printed values near `u64::MAX`. The report now
compares two valid, increasing totals from the same bridge generation.
A switch, a missing total or a decrease reports `unavailable`.
The regression verifies same-generation counts, a decrease and
switches in either direction. The Runtime counters and public ABI
retain their existing meaning.

The Standard retest at `9367f1306570` repeated DLSS-G to FSR3 to
XeSS with XeSS SR running. Each provider change reported
`unavailable` for its first window. Steady windows returned to
roughly 240 presents per 120 source frames. The process exited with
code 0 and restored the isolated settings and script. The logs and
paired window checks are in `evidence/standard-provider-counter-retest/`.

## Package-relative SR paths on the configuration candidate

The isolated Standard run used the configuration worker's Release
Anomaly DLL with SHA-256 beginning `346191e9` and the merged Runtime
with SHA-256 beginning `424952a9`. The three SR vendor-directory
overrides were removed from the clone's test INI. The overlay advertised
the packaged backend choices, DLSS SR started, and the engine resized
from 1280×720 to 1600×900. The overlay then saved `SRBackend=5` and
XeSS ran at 942×530 with 1600×900 output. The last full SR report
showed 3,929 evaluations with zero refusals. DLSS-G remained active.

The post-switch window capture timed out. The provider result is from
the Runtime log, counters and saved INI. The engine exited with code 0
and the runner restored the isolated DLLs, pack, settings and lifecycle
script. This result covers the staged Release candidate and the DLSS
and XeSS package-relative paths. Logs and complete hashes are under
`.local/anomaly/staging/live-58fe95a-stage5/evidence/` in
`rust-sr-configuration-candidate-standard-346191e9`.
