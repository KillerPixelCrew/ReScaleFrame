# RDR2 integration: retained findings and current handoff

Updated 26 October 2026. Read this before requesting another game run or rediscovering the
camera/HUD path. This file records source evidence, measured captures, failed approaches, current
deployment, and unfinished implementation. It is not a claim that RSF SR or FG works in RDR2.

### Current checkpoint: runtime quality presets and shared overlay

**Current candidate:** ASI66e5b9a9...fb4d3d, Runtimed7b43b45...c0e6ad6,
Rust14369273...03e43f; game ABI19, frontend ABI2, overlay ABI13. Release VS2026
passed97 discovered/11 optional skips, fmt/Clippy. Rust41+19 tests/release passed.
Receipt: `.local/rdr2-tools/quality-overlay-20261026/deployment.json`.
The permanent pending-owner implementation is game-tested in PID140840 and user-confirmed.
`permanent-result.json` records all five actual sizes and repeated Native->Quality transitions.
The final Runtime adds the FG-Off selector mapping, passed native frontend regression and the
full Release gate, and is deployed; no separate post-patch visual run yet. The saved provider
is retained for re-enable, but disabled FG displays Off. Diagnostics remain off.

**Measured root cause:** RAGE owns separate active and pending graphics trees at manager+8
and manager+b8. Earlier code wrote the active object, which native Apply then overwrote from
the unchanged pending object. `live-settings.json` measured the distinct graphics owners,
both with id8c463422 and derived vtable3326488. The active collection had version37; the
pending collection had version0. Its two collections start at owner+b0 and owner+240.
Only RSF-owned scalar fields change: collection+e8 DLSS selector, +ec DLSS renderer mode,
+f4 FSR2 carrier, +15c native marker pump. No string copies are needed.

**All five modes now measured in one live session through a guarded diagnostic:** DLAA0
2560x1600; Quality1 1707x1067; Balanced2 1485x928; Performance3 1280x800; Ultra Performance4
853x533, all with2560x1600 output and accepted DLSS backend contexts. Evidence:
`quality-overlay-20261026/{diagnostic-result.json,pending-probe*.log,all-modes-diagnostic-game.log}`.
This proof used guarded writes to the pending object before the existing main-thread request.
The installed implementation performs those writes itself on the ScriptHook game fiber.
The subsequent permanent run used normal installed code, with no diagnostic memory writes.

**Correct native reset path:** +2c7e8f4 enters manager state9, calls manager virtual+60
(derived vtable3326620 -> +23b570), then renderer virtual+188 (object3c85d78,
vtable3626150 -> +266dd1c). The engine owns synchronization, allocation and dimension refresh.
The +58a908 observer now logs resulting render/output sizes. +2c7d190 is only settings
notification; it did not itself trigger the graphics reset. +2c7e684 reloads GUI/XML settings,
not render targets. Earlier speculative decompiler labels are not evidence of their meaning.

**Capability-dependent indices:** active choices were [5,6,2,1,0,3], while the unrefreshed
pending owner initially had [5,6,4,2,1,0,3]. Resolve the selector from the active list instead
of assuming an index. Native/DLAA retains an active Quality carrier and overrides optimal
dimensions to1:1. FSR2 uses active modes1..3; Native and Ultra Performance override dimensions.
Non-NVIDIA hardware and HDR remain untested. Runtime quality requires the ScriptHook helper.

**Rejected attempts retained:** first active-only candidate produced corruption without
resizing; GUI/XML notification and notification-queue candidates also failed. Native state9
removed corruption but still copied stale pending values. See first/second/third-game logs,
failed-scene.png and deployment receipts. A temporary restore was performed without asking;
the human explicitly rejected that. Do not restore a build again without their instruction.

**Overlay flicker:** source no longer omits a frame on busy settings observation or full
three-slot UI GPU ring. Cached preferences survive contention; a full ring waits for the oldest
already-submitted private fence. A deterministic WARP backlog test passes with no D3D12 errors.
Visual flicker acceptance remains separate from that test. The prior ShowCursor infinite loop
is fixed and was user-confirmed. Native RSF settings-row insertion is cancelled; all controls
live in the shared Rust overlay. Keep forced DX12, Social Club correction, RTSS and caches intact.

### Historical checkpoint before the successful22:13 deployment

**CURRENT TASK: shared RSF overlay and enforced engine input settings.** The human explicitly
cancelled native UI integration and clarified: RSF owns DLSS/FSR/Reflex internally; controls live
in the shared overlay. Force a carrier on even after native Off/Apply, including non-NVIDIA
systems (the human explicitly accepted native FSR2 there). Do not resume stock RSF-row work.
No new agents were authorized or spawned. The old menu-research worker remains settled.

**Critical current checkpoint, after the shared overlay hang:** the installed game files are
still the OLD crashing interop build: ASIc6f5e5b7...3ba19db4, Runtime2cd10ba6...242da05,
Gamefa254ce3...48abdb, Rust overlay ABI13 acc4e639...021e7. The repaired native12/cursor build
has NOT been deployed yet. Game PID124784 was accidentally relaunched when a deployment guard
failed but orchestration continued to a separate launch call. Do not repeat that construction:
make launch conditional on successful deployment. The game's window was not initially exposed
through Process.MainWindowHandle. Win32 WM_CLOSE was posted to its PID-validated sgaWindow;
closing its hung overlay through the verified rsf_rdr2_overlay_toggle export resumed rendering,
then CloseMainWindow succeeded. PID124784 subsequently exited (Orca app_not_found confirmed).
Check process absence again before deployment. Do not replace loaded DLLs.

The ACTUAL hang cause is now measured, not inferred: `old-overlay-hang-stacks.txt/.dmp` shows
a RAGE worker at RDR2+263cf32 in the ShowCursor(FALSE) loop. Exact game bytes at263cf2a:
`33 c9 ff 15 26 56 9f 03 85 c0 79 f4`; JNS repeats until a negative return. Our old shared
hook queried the real cursor count using ShowCursor(TRUE)/ShowCursor(FALSE) and returned a
nonnegative value indefinitely while the overlay kept the real cursor visible. The log recorded
222326762 swallowed hides. Closing the panel resumed Presents immediately. New source in
`runtime/graphics/src/overlay_input.cpp` keeps a thread-local logical counter while visible,
without calling user32, and forwards/caches actual counts while hidden. `tests/overlay_host.cpp`
now tests bounded RAGE-style hide/show loops. Never disable RTSS or Steam as a workaround.

Separate measured interop defect: `shared-overlay-20261026/gpu-overlay.log` fails D3D12 debug
validation with repeated ReflectSharedProperties errors from wrapped raw D3D12 resources.
RDR2 now uses `runtime/orchestrator/src/overlay_native12.cpp`: the SAME Rust panel ABI13, native
DX12 lists/PSO, explicit PRESENT->RT->PRESENT, three fenced allocator/upload/descriptor slots,
fenced texture uploads/replacements/frees, and no retained backbuffer references. It never blocks
the engine Present thread on a reusable slot; Unity keeps its old interop path. Native12 GPU
fixture opens/closes full panel over40 frames/39 FG tags with debug validation passed in
`gpu-overlay-native12-final.log`. The fixture now also executes game-style hide calls while open.

Another actual defect explained FG Off: forcing dlssIndex alone left renderer dlssQuality=5
(disabled). `engine_settings_policy.h` now pins collection+e8=3 AND +ec=1 on NVIDIA, +f4=0;
on other/unknown vendors pins +e8=0,+ec=5,+f4=1. +15c=1 retains the native marker/sleep pump;
RDR2 host suppresses native MODE requests immediately and RSF owns actual driver preferences.
Schema: collection graphics+b4; dlssIndex+34, dlssQuality+38, fsr2Index+40; video+124,
adapterIndex+0, ReflexSettings+38. Version+178 is UINT16, padding after it is not part of it;
defaults version0 and loaded37 are accepted. This was verified against generated parser tables
and ctor+2c5eb34; policy tests preserve all unrelated bytes and exercise NVIDIA/AMD/Intel/Off.
ASI installs exact-byte-guarded collection-copy+20b7f8, load+2c7c9a4, refresh+2c7e6c0 hooks
early after forced-DX12 validation. The refresh was initially called Apply in source research;
its tail rebuilds available quality choices. Typed collection copy also covers native application.

**Next:** collect exec session68794: full Release VS2026 gate to
`.local/rdr2-tools/rdr2-overlay-cursor-final-verify.log`. Earlier gate96 discovered/11 skips
passed, including cursor regression, before the final opt-in probe refinements. Rust workspace
41+19 tests passed and release panel already built (ABI13 unchanged). Once the gate passes,
run the opt-in RDR2 GPU fixture once with RSF_RDR2_TEST_OVERLAY=1 and the real Rust DLL, then
back up/hash-deploy ASI+Runtime ONLY (Rust acc4... and Gamefa25... already match).
Enable one-shot [RDR2] OverlayProbe=1. Host clears it to0 at startup; process opens panel after60
Presents, executes a bounded ShowCursor hide test, closes after120 ACTUAL additional Presents.
Probe only advances if panel visible. `.local/rdr2-tools/watch-overlay-probe.ps1` captures CDB
thread stacks/minidump on stalled probe progress and exits after success/120s. Start it BEFORE
the game, but only AFTER verified deployment; do not launch if deployment fails. Keep every
wait under60s and report meaningful progress. Ensure other heavy builds are not active; a Godot
Vulkan/capture build was active earlier and is now finished. Normal reusable MSBuild nodes are
not active compilation. No test/game/Java jobs from this turn should remain except gate68794.

Orca computer CLI now WORKS, unlike the old CUA pipe. Loaded skill via
`orca skills get computer-use`. However get-app-state --restore-window for RDR2 returned an
occluded ChatGPT desktop screenshot, so no guessed click was sent. Use PID/window selectors,
fresh screenshots and do not act on another app's pixels. The user authorized closing RDR2
normally. The native API close/toggle recovery scripts and all evidence are under .local.

Latest gameplay result: the human reports improved look/feel and no orange flashes, but brief
white text changes. `after-retirement-fix.log` and `retirement-result.json` show83.344 seconds,
4320 source frames,8640 SDK-presented frames,37 periodic samples all active/result0 after warm-up,
and no recurring reset/skip transitions. The retirement correction is accepted for that XeSS
foreground scene. White text alternates because the ScriptHook code replaces labels on any
busy status try-lock. Source now keeps the cached reading for brief contention and says stale
only after1s without a successful observation. It does not debounce actual FG state. Full gate
`rdr2-frontend-stale-status-verify.log` passed82 discovered/11 optional skips, fmt/Clippy.
The ASI at `.local/rdr2-tools/xess-flicker-20261026/staged-status-label/`, hash
a1a7c98e...e07544c, was backed-up/hash-deployed at20:19 after the human exited the game.
Receipt: `staged-status-label/deployment.json`. Runtime68b7229e...5bd37a0 was verified unchanged.
Exec17341 completed successfully. Text-fix visual acceptance remains pending the next game run.
Do not replace the running ASI. The live game remains on the accepted Runtime68b7229e...5bd37a0;
the label-only fix needs only the ASI, so do not replace that runtime with unrelated rebuilt work.

Current correction superseding the timing hypothesis: `frameRenderTime=0` did not fix flicker.
The next live log shows skip result3 precisely with reset1/source_gap1; accepted native FG
inputs remain reason0 in the bounded live state read. In `rsf_fg12_latency`, RAGE PresentEnd
cleanup called `rsf_d3d11_present_abort` for completed/skipped CPU inputs. For XeSS that reaches
`abort_frame`, which unconditionally clears interpolation history, including when the retired
CPU input never supplied GPU resources. Normal completion is not cancellation. Removed those
provider abort calls; retained completed flags and authoritative Streamline retire-through.
The guarded diagnostic hotpatch was NOT applied because RDR2 had exited before invocation.
Permanent Runtime68b7229e...5bd37a0 is backed-up/hash-deployed; receipt is
`xess-flicker-20261026/retirement-deployment.json`, beside before-retirement-fix.log and corrected
live-admission.json. Gate `rdr2-xess-retirement-verify.log` passed82 discovered/11 optional skips,
fmt/Clippy. The preceding gate had a new shared Vulkan capture test added during the build and
could not find its executable; a complete build/gate now passed. Steam launch requested.
Gameplay acceptance is still pending. Do not describe the flicker as game-verified fixed yet.

Latest XeSS flicker follow-up: the human still sees rapid white/orange changes. Transition
telemetry proves XeSS interpolation result3 (`WARNING_TOO_FEW_FRAMES`), enabled1, presented1,
followed by success/presented2, rather than an observer-only colour issue. Evidence snapshot:
`.local/rdr2-tools/xess-flicker-20261026/before-timing-fix.log`. The XeSS guide says optional
`frameRenderTime` participates in non-Intel pacing checks and should be zero when reliable timing
is unavailable. RSF passed engine simulation delta instead. A 300-sample live read ranged
17.3344–64.9517ms; that variation alone does not establish causality. The candidate sets the
optional field to zero and records prepared reset/source-gap alongside skip transitions.
Release VS2026 gate passed81 discovered (11 optional skips), fmt/Clippy; backed-up hashes and
deployment are in that evidence directory, gate `rdr2-xess-pacing-verify.log`. RDR2 was closed at
deployment and Steam launch requested. Gameplay validation is pending: do not call the flicker
fixed yet. A separate shared capture fixture chained UINT-to-UINT16 assignment failed /WX;
splitting its literal1 assignments fixed the build without changing fixture behaviour.

26 October, 19:34 update: ABI19 frontend deployed and the human confirmed the overlay and quick
menu are visible in gameplay. The human then reported FG text alternating white/orange. Their
current preferences are XeSS SR and XeSS FG (both backend5), generated1; preserve these choices.
Run AE logs show successful live switch and mostly active XeSS presentation, with an inactive
sample at source5854 despite valid matched inputs. This does not yet prove why generation skipped.
The new backed-up deployment records actual XeSS query/interpolation results, enabled state,
presented count and source ID on activity/result transitions, rather than hiding the dropout.
It also corrects the misleading backend-changing label for ordinary observer lock contention.
Release VS2026 verification passed (69 executed, 11 optional skips, fmt/Clippy); evidence is
`.local/rdr2-tools/rdr2-frontend-fg-status-verify.log` and AE's `fg-status-deployment.json`.
Temporary preview/probe/capture settings are off; status overlay remains on. Live validation of
this diagnostic build and true inline stock RSF settings remain unfinished. The older deployment
and process statements below describe the preceding checkpoint, not the current installation.

**Current SOURCE is Game ABI19**, installed disk still ABI18 until the next backed-up deployment.
`script_ui.cpp/.h` implements a ScriptHook fiber, native BG text (old HUD text natives are no-ops
after1436), base/presented FPS, requested/effective FG/counts and explicit refusal reasons.
It draws an F7 / both-sticks quick menu with pending/Apply/Cancel, backend/preset/FG/count/
latency/limit/status controls and a shortcut to the stock graphics page. Controls share the new
runtime C ABI in `rdr2_frontend.h`. Source compiles; it is NOT yet visually game-validated.
ScriptHook is loaded from the existing game installation if not already loaded, outside DllMain.

`rdr2_sr_host.cpp` exports size/version checked POD status/settings get/apply. Base FPS counts
completed application Presents; output FPS uses valid SDK presentation counters, never a guessed
multiplier. Output shows n/a without counters. Settings commit the owned section keys together
while preserving unrelated keys. Observations use try-locks to avoid blocking the ScriptHook/
window thread during model/context creation. Existing FG status callers retain blocking behavior;
`rsf_fg12_status_try` is the new nonblocking observer. Game ABI19 appends typed render_feedback,
and `native_renderer.cpp` publishes current descriptor/camera/jitter/admission refusals even
when no usable pass exists, so the widget can explain why FG is off.

The focused frontend test verifies settings persistence, unrelated keys, invalid-request refusal,
camera-jitter reason visibility, unknown output FPS and ABI guards. Full Release gate
`rdr2-frontend-verify.log` passed69 executed/11 skips, fmt/Clippy. Latest tiny preview/foreground
correction gate `rdr2-frontend-preview-verify.log` is running as exec session82228; collect it.
All other old build/analysis sessions are complete. RDR2 remains CLOSED.

**Stock menu investigation:** a broad REA Ghidra pass was deliberately stopped after it analyzed
unrelated code for many minutes. REA target closed cleanly. Its log is
`.local/rdr2-menu-research/overbroad-ghidra-analysis.log`. The successful bounded alternative is
`Rdr2Settings.java`, scoped-functions.txt and scoped-decompilation.txt in that directory,
project `references/ghidra-rdr2-settings/Settings.gpr`. Run headless -process RDR2.exe.dump
-noanalysis with the script for additional bounded functions. Neither GUI nor headless Ghidra
accepts a project path containing a dot-prefixed component. Do not repeat whole-program analysis.
Fifteen settings functions were decompiled and named. Some .pdata entries are chained fragments;
do not promote a decompiler prototype or apparent body extent to a verified callable ABI.
The stock value read helper2c627b8 calls its SOURCE object's vtable+78. The source type, path
and commit/backout ownership still need runtime proof. Do not assume its vtable is3326428.
Public CodeX extraction tooling linked by the reference repo is now404; no tool was installed.

`native_settings.cpp/.h` is an opt-in read-only bounded probe for bind2c626e0 and read2c627b8.
It checks exact instruction bytes; records16 relevant rows and32 values with source/destination
vtable/path/variant bytes; never mutates stock rows. NativeSettingsProbe/NativeSettingsProbePath
are off by default. ScriptHook arms it before the stock graphics page is opened. Temporary
FrontendPreview=1 shows the quick menu; the probe requests settings_menu/pause_graphics five
seconds after the first real world input, with game foreground. Both diagnostics must be off in
the final INI. Next: collect latest gate, back up/deploy ABI19 ASI/Game/Runtime together, use one
live pass for overlay/menu and native value-source evidence, then finish the stock rows and
their proper Apply/Cancel handlers. Never claim native row replacement from the shortcut.

**CURRENT FINAL DEPLOYMENT,18:13:** the human closed AD normally. The permanent token-retirement
build is now backed up and hash-verified into the game, receipt
`native-adapter-game-ad-20261026/permanent-deployment.json`; backup
`.local/deploy-backups/rdr2-fg-permanent-20261026-181334/`. Runtime is
**`9a9d0fccb82b53dece1ab7c3595e37d2d1e8077f5821fdf2f3deb953b7d055ac`**; ASI and Game DLL
remain6a5994e0/0754f198. INI is diagnostics-off, SR1/DLSS1, FG1/DLSS1/generated1, Latency1,
FrameLimitUs0, no temporary SRCaptureDirectory. No cache changes, commits or pushes.
RDR2 is currently CLOSED. No build, monitor or diagnostic helper process remains owned.

The human explicitly said **"You can close the game on your own."** Future required normal
RDR2 closure is authorized; do not ask them to close it again. Steam launches remain authorized.
Computer control is STILL disconnected: cua.rewriteDocumentation succeeded, but listWindows
returns native-pipe unavailable (os error2). No further reset/retry loop was attempted. The
permanent disk build is not yet cold game-tested; GPU regression and the equivalent AD live
diagnostic are separate evidence. The old exact-runtime token reader/hotfix must NOT be used
against the new Runtime: its RVA/byte guards belong specifically to83f91dba.

**Completed AD activity:** DLSS2x ->4x ->Off ->2x passes; SDK active1 and driver multipliers2/4
in foreground, Off reports mode0/active0. SR DLSS->FSR3->DLSS while2x FG runs also passes.
Logs `dlss-{2x,4x,off,resume}-rdr2.log`, `fsr3-sr-dlss-fg-rdr2.log`, `returned-dlss-rdr2.log`
and `final-rdr2.log`. Eight accepted SR outputs are finite at1485x928->2560x1600;
projection*inverse maximum residual3.93482635e-8, `sr-pixel-validation.json`. Live MFG count
was restored to1. Full-mode/HDR/visible-overlay/generated-image quality and native RSF menu
acceptance remain open. Native FSR entry and engine resolution-quality ownership are unfinished.

**AD LIVE CORRECTION:** PID114512 cold-launched ABI18. Native guards, RTSS and Social Club
load. Gameplay initially failed shared SR with-8 and FG source0. Read-only exact-runtime
snapshot `native-adapter-game-ad-20261026/live-tokens.json` proved the six-slot SDK pool was
stuck at last_begin1128 while the game reached22858: skipped loading input1123/1125 remained
held. The CPU ring can overwrite those entries before any matched scene exists, so cleanup
only from valid FG preparation cannot recover this circular failure.

Source now retires uncompleted native CPU entries through actual ordered PresentEnd and calls
`rsf_streamline_host_retire_through` on the authoritative SDK pool, even after CPU-ring wrap.
The expanded GPU fixture covers140 inputs without an image, then Present and40 valid SR/FG
frames;39 tags and debug validation pass (`rdr2-skipped-input-recovery-gpu.log`). Full Release
VS2026 passes67 executed/11 skips, fmt/Clippy, `rdr2-skipped-input-final-verify.log`.

To continue this session without replacing loaded files, a separate diagnostic DLL was built
and attached from `.local/rdr2-token-hotfix-20261026/`. It guards the exact deployed Runtime
83f91dba export entry f7c0 and active_host load3af19->a7850, preserves the original callback,
and retires old tokens through its real PresentEnd. `token-fix.log` proves1123/1125 freed at
PresentEnd46160. Normal source/SDK operation immediately recovered; do not initialize a second
Runtime or replace files in PID114512. This diagnostic is process-owned, not production delivery.

**DLSS2x GAME ACTIVITY ACCEPTED:** after the human left gameplay in the foreground, shared
FG logs `active=1`, driver `fg_multiplier=2`, real simulation/render/present timings and
app_called_sleep1. Evidence `native-adapter-game-ad-20261026/dlss-2x-rdr2.log`. Normal SDR
scene/final2560x1600 PPMs differ in116394 pixels, all inside minimap/prompts; zero differing
pixels outside their two ROIs. `hud-pixel-comparison.json` and original PPM/PNG files under
pixels/ establish the actual new hook's normal SDR boundary, not just named-resource inference.
Full all-mode separation, native UI-only premultiplied extraction and HDR remain unproven.
The final deployment checkpoint above supersedes AD's83f91dba plus diagnostic. The live MFG
count3 request passed and was restored to1. Do not confuse SDK activity with image quality.

**LATEST SOURCE, approximately17:20:** Game ABI18 appends the exact native latency-event
callback. `scene_boundary.cpp` hooks native sleep2687fb4, mode1e800 and marker26d4674 with
expected-byte guards. Runtime RDR2Host installs shared cold-start FG presentation before the
game creates graphics, preserves input/simulation/render IDs, supplies scene/window association
and polls FG/FGBackend/FGGeneratedFrames/Latency/FrameLimitUs. `FG=1` means Fixed; `FG=2`
means Auto in the shared enum and is currently refused by native FG12's Fixed-only control.
Do not use2 as Fixed. The newly added latency log counter is atomic.

The first cold GPU fixture caught a real proxy/native queue mismatch at ExecuteCommandLists
(AV in D3D12Core/debug layer, Runtime+3696e). The earlier idea of assigning the engine's raw
queue to StreamlineHost.queue was WRONG. Streamline2.14.1 cannot upgrade an existing queue
through slUpgradeInterface; the upgraded device must create its proxy queue. The bridge already
retains the real engine queue and joins source and presentation work with fences. That path is
restored, with validation of the adopted queue/device. Never repeat the raw-queue substitution.
Source evidence is `.local/streamline/sdk/source/core/sl.api/sl.cpp` and interposer queue code.

Final40-frame GPU fixtures pass the actual RDR2Host callbacks:39 tagged,
scene/input/submission/resource identity matches, configured mode1, result0, debug validation.
Logs are `rdr2-cold-fg-pixel-gpu.log`, `rdr2-cold-fg-fsr3-gpu.log` and
`rdr2-cold-fg-xess-gpu.log`; the latter two switch from initial DLSS FG through the real INI
poll and presentation generation2. FSR3 reports generation callbacks (source7 generated6),
while hidden DLSS reports active0. This is not game/generated-image quality acceptance.
Opt-in original final/HUD-less PPMs use SRCaptureDirectory and exported
`rsf_fg12_capture_outputs`, once per provider generation after30 samples. The cold pair in
`cold-fg-original-pair/` is identical, SHA256
`71d0931056f1dd54e8a7d1df7279213097d7a8b243290d379409e8ca4cf00cf2`.
It has no UI by construction and proves copy/retirement, not RDR2 HUD separation. Failed logs
`rdr2-cold-fg-gpu.log`, `rdr2-cold-fg-debug.log` and the joined
control `rdr2-cold-fg-joined-gpu.log` are retained. Full ABI18 Release VS2026 gate passed:
67 executed/11 opt-in skips, fmt/Clippy, latest `rdr2-fg-pixels-final-verify.log`. The human
closed AC normally. At17:38 the candidate was backed up and hash-verified into the game,
receipt `native-adapter-game-ad-20261026/deployment.json`, backup
`.local/deploy-backups/rdr2-fg-abi18-20261026-173811/`. It is now cold-launched through Steam.
Ready candidate `.local/rdr2-fg-abi18-candidate-20261026/`; receipt and diagnostics-off INI
`.local/rdr2-tools/native-adapter-game-ad-20261026/`. ASI6a5994e0...5b3c7f35,
Game0754f198...513b89df, Runtime83f91dba...2abef3e3. Candidate INI enables FG1/backend1 and
bounded original pixels, all ordinary diagnostics0. The prior probe's fsr2Index1->0 is restored,
dlssIndex3 retained and HDR=false preserved. No cache edits. AC final log is preserved in
`native-adapter-game-ac-20261026/final-rdr2.log`. After AD exits restore its diagnostics-off.ini
to remove temporary pixel recording. Startup/engine hook/FG game acceptance is pending.

**Scene source details, implemented before the ABI18 connection:** `scene_boundary.cpp/.h` is
compiled into Game.RDR2. Native SR calls it only after accepted output. It observes actual
marker IDs at `26d4674`; only masks containing native0/1/2/6 publish input identity. It brackets
output helper **`5f1e08`**, restricted to the three calls inside the postfx upscale node at
`5caafc/5cad96/5cadec` (returns5cab01/5cad9b/5cadf1), then captures after inner blit **`5c1254`**
returning to5c11b9. Parent scoping avoids unrelated generic blits; no stack heuristic is used.
Entry bytes and call destinations are validated. ABI18 replaces native latency operations
only after shared ownership accepts them.

The live renderer global is `571b5d8`; AC read-only lookup gives native vtable+248 ->
**`26b4250`**, bytes `48 8b 42 48 c3`, native texture at grcTexture+48. Context colour views
are `+ca48`, count `+ca94`; each view+8 points to its texture, view+14 encodes mip bits4..7.
Depth view is +ca88. Writers/readers proven at26d4964/26d4100/26ddf80. The blit hook requires
one mip0 colour view, same native frame and command list as accepted SR, exact swapchain
resource membership, and output dimensions. It takes only callback-local native references.
Multiple upscale views refuse association. At actual Present marker4 it compares the scene
identity with the actual current backbuffer on the presenting thread.

RuntimeHost now has render-pass callbacks and scene/submission records. `rsf_sr12_capture_scene`
records actual scene pixels with the accepted SR slot's COPIED frame/camera and its existing
real Execute/fence retirement. Captures use `fg-scene-output` plus ordinal. It also forwards
HUD-less/window data to the shared FG API only when the native source association is valid.
ABI18 above now installs the shared owner and translates real native CPU boundaries.
These engine hooks and live FG output still require game acceptance.

The GPU fixture passed the new RT-state scene copy, debug validation, normal SR output,
unsubmitted-slot refusal/retirement/reuse and DLSS->FSR3->DLSS with persistent registration.
Log `.local/rdr2-tools/scene-producer-fixture.log`; raw scene and reinserted output in
`scene-producer-fixture-20261026/` have identical SHA256
`4eb4547546cfab1521b2719419b3bd2259b19de582ba4974bb91fe6489abe1a2`.
Full Release gate `.local/rdr2-tools/scene-producer-verify.log` has passed 67 executed tests,
11 opt-in skips, fmt/Clippy. This new scene source has **not** been deployed or run in RDR2.
The currently running AC process still owns the earlier32b2/61dc lifetime-fix build.

Next live checkpoint: verify native input->scene->Present identity and supported colour/mode
boundaries on one consolidated run, preserving RTSS and one SDK registration. The shared
context expects a token acquired at the real CPU boundary. Do not fabricate CPU events from
the upscale callback or initialize a second runtime/SDK. Streamline's separate proxy queue is
required; the bridge retains the real engine queue and uses explicit fence joins.

**CURRENT, 26 October approximately 14:54:** Run AC **PID126008 is running and stable in
Settings**, image base `0x7ff62a0b0000`. The persistent NGX registration fix passed GPU live
DLSS->FSR3->DLSS and the full Release VS2026 gate (67 executed, 11 opt-in skips, fmt/Clippy):
`.local/rdr2-tools/native-adapter-ngx-lifetime-final-verify.log`. It was backed up and deployed
before launch, `.local/deploy-backups/rdr2-ngx-lifetime-20261026-144604/`. Installed ASI hash
remains `28f950b6...406209e7`; Game DLL is **`32b2ca8ec7d536078806383cb8daec6693c4ef38d21897e86e7bd00d9c22eff1`**,
Runtime **`61dcb4e20824305be5789bc0b9d15c4e23fae7b9677694e16b5fad91922dae8e`**.
No late attachment or second Runtime was used in AC. The startup byte guard passes with the
correct FSR thunk/IAT **`2ea0af5/6032a98`**. All workers are settled and released.

Run AC automatically collected **12** outputs over DLSS->FSR3->DLSS, then the user opened
Pause/Settings. The former crash path now repeatedly logs native DLSS fallback result **1**
(`0x00000001`), feature `0x1e62797bdc0`, after frozen-projection refusal. The game remains alive
and the human supplied a Settings screenshot. This is real pause/fallback acceptance after
context switching. It is not FG or complete visual/performance acceptance. Evidence is
`.local/rdr2-tools/native-adapter-game-ac-20261026/`, deployment.json, pixels/, provider-cycle.log,
provider-cycle-ready.txt; game `ReScaleFrame/rdr2.log` holds current logs. The provider-cycle
exec session88069 has completed; no monitors/builds are still owned/running.

**Current INI:** SR=1, SRBackend=1, all normal diagnostics zero; temporary SRCaptureDirectory
points at AC pixels. After normal game exit restore AC `diagnostics-off.ini` (retain desired SR
settings), and back up any later replacements. Never replace running files or manipulate caches.
No pending native source changes since this deployed/verified lifetime fix. Documentation has
older wrong RVA/status paragraphs below; the current checkpoint and source supersede them.

**Native menu:** ONE agent was requested, completed, accepted and released. Its report is
`games/rdr2/NATIVE-OPTIONS-RESEARCH.md`, now appended with coordinator live metadata proof.
Read-only memory copying resolved **system/graphics = 8c463422/bc098ed9**, 29 graphics rows.
Live item keys match dlssIndex `4de87d5b`, dlssQuality `db17b778` (hidden), dlssSharpen
`3420e356`, fsr2Index `58e23159`, fsr2Sharpen `0cd9de94`. No menu mutation exists yet.
The screenshot shows **all AA rows greyed**. Current XML has both dlssIndex3 and fsr2Index1,
unlike the pre-admin3/0 backup; the prior probe's selector edits may explain mutual locking.
Do not call it an intentional RSF lock. Need decode the two condition-like records per row
at metadata+48 (24-byte records, field+8, child pointer+10), value accessors and apply/backout.
Read-only snapshot/scripts are `.local/rdr2-menu-research/live-ac-20261026/` and
`.local/rdr2-tools/live-settings-metadata.py`, names mapped by `name-live-settings.py`.
The user prefers ScriptHook. Native metadata admission/visibility hooks are proven, ScriptHook
alone is unproven. No additional agent authorized or active. The native FSR selection route
remains **unexercised in game** because its UI row is disabled. Do not ask them to select it again.

**Remaining primary work:** native FSR route validation, owned resolution producer/quality,
FG/PCL integration and complete HUD separation remain unfinished. The camera has real same-call
and static terrain reprojection proof; HDR unavailable, photo/cinematic/cutscene/Social Club UI
coverage incomplete. Do not stop and call the integration complete merely because SR now runs.
Read older checkpoint details below only when needed. The user asked to keep all findings here
to survive compaction and has repeatedly objected to endless probe/restart rounds.

**Pause crash and current fix, approximately 14:35:** AB PID126676 crashed on opening Pause,
before the requested native FSR setting change. It is now closed. The automatic-capture INI
was restored from `native-adapter-game-ab-20261026/diagnostics-off.ini`; diagnostics are off.
Crashpad minidump is preserved as
`.local/rdr2-tools/native-adapter-game-ab-20261026/pause-crash-623cb3cc.dmp` (19 MB).
Use **`.local/rdr2-tools/venv/Scripts/python.exe`** for `mdump.py`; that venv has minidump and
Capstone. Console Python314 does not have minidump. `mdump.py` now reads the exception-stream
CONTEXT instead of the later crash-handler thread context. Stack/facts/disassembly are
`pause-crash-stack.txt`, `pause-crash-facts.json`, `pause-crash-driver.txt` in the same directory.

The exception is in DriverStore `_nvngx.dll+687c3`, instruction
`cmp qword ptr [rcx+rdi+24b8],0`. Native cached feature pointer `0x202cd8c6c40` contains ASCII
`{"api":{"build":1,"major":0,"minor":0},"build_config":"PRODUCTIO...` instead of an NGX handle.
The bogus type DWORD +4 is `0x7b3a2269`, multiplied by 0x98 to RCX `0x492a846e58`, producing
the fault read `0x24bca203320`. Stack: `_nvngx -> nvngx+ a4f4 -> RDR2+2e9c900/2e9ce30 ->
Game.RDR2+36a1 (native fallback) -> RDR2+26f4199 -> 26f060f -> 5c7d89`. It follows the
frozen-projection refusal at tick337924265. This is a dangling native feature handle after
global NGX/Streamline shutdown/reinitialization across RSF backend changes, not proof of a
stock-menu drawing defect. The earlier process-only native-fallback interval did not prove
native handle validity.

Source now separates persistent `rsf_dlss_native12_registration` ownership from per-SR feature
contexts. The RDR2 runtime keeps one registration for the process/session, clones its API/device
owner into serial contexts, frees only RSF feature resources on provider/preset/size changes,
and continues manual Present bookkeeping while another backend or native fallback is active.
No `slShutdown` or interposer unload occurs during those changes. This fixes ownership rather
than masking pause or suppressing the native fallback. Existing standalone/shared-host callers
retain their prior lifetime semantics. `rsf_sr12_setup` ABI2 appends borrowed `dlss_registration`.
Changed files: dlss_native12.h/.cpp, native_sr_d3d12.h/.cpp, rdr2_sr_host.cpp and the GPU fixture.

The focused GPU fixture **passes** DLSS -> FSR3 -> DLSS through live INI changes, verifies the
same interposer registration remains loaded, pixels and D3D12 debug validation:
`.local/rdr2-tools/sr-host-persistent-ngx-gpu.log`. The full Release gate is currently running,
exec session74096; log is
`.local/rdr2-tools/native-adapter-ngx-lifetime-verify.log`. Finish it, then back up/deploy current
Runtime and corrected Game DLL with the game closed, and validate pause/native selection in
one run. No product files with this lifetime fix have yet been deployed at this checkpoint.

The ONE menu agent completed successfully and was accepted/released. Report:
`games/rdr2/NATIVE-OPTIONS-RESEARCH.md`; exact native metadata visibility gate `2c7f0ca`, binding
helper `2c626e0`, ScriptHook list natives and an implementation slice. A pure ScriptHook stock
graphics replacement is unproven; native row IDs and apply/backout handlers still need matching.
Dispatch `ctx_4fc9e9fc3521` is released; delivery `delivery_7c6f40dfa086` was acknowledged.
No workers remain owned. Do not poll/restart it or create another agent without authorization.

**Latest coordinator state, 14:17:** the human requested ONE background Orca agent to investigate
stock graphics-menu replacement, preferably through ScriptHook. It is running under
Run `run_ad8cd967d63e`, Task `task_542320f302f3`, Dispatch `ctx_4fc9e9fc3521`, worker handle
`structworker_d26c912b-79eb-4f1c-b7a2-8b87b61112b0`. Current coordinator caller remains
`orca_session_id:666a44fe-2f73-4955-b33e-202413c72e2d`; the new Run's coordinator handle is
`structworker_473e0472-9e9a-4f78-a509-858bbc822315`. Use the exact CLI
`C:\Users\N1GHT\AppData\Local\Programs\orca\resources\bin\orca.exe`. The version-matched
orchestration guide was read. Check/handle/ack its deliveries at natural checkpoints and accept
the result, then release the settled worker before ending. Do not reuse any old settled task IDs.
Brief `.local/rdr2-tools/native-options-task.txt`; exclusive report ownership is
`games/rdr2/NATIVE-OPTIONS-RESEARCH.md`. Worker must not touch builds, game memory/installation,
desktop or other source. No further agents authorized. Primary work remains direct, as requested.

Live PID126676 now has **20** automatically captured SR outputs: DLSS, FSR3, FSR4 INT8, XeSS,
then DLSS again, all in session336880188 without restart. `SR=0` retained native evaluation for
roughly 144 seconds (tick337386625 to337530984), after which SR=1/backend1 resumed accepted
output. Existing logs do not explicitly record the native vendor return code, so process survival
alone is not complete native-pixel acceptance. The human identified FSR4/XeSS first-use pauses
as model loading; retain the measured cold-load timing without inventing a redesign requirement.

**Pending human action:** async question asks for ONE native Graphics menu switch from DLSS to
FSR2, Apply, return to gameplay. RSF stays SR=1/backend1; this verifies normalization of the
native FSR selection into RSF, not native FSR2 execution. Do not request another restart or F10.
On reply, force a backend/context transition if needed to obtain four new automatic input/output
captures from that native route, and inspect results. The game is still running; the corrected
plugin is attached from .local and must be deployed into the installation after normal exit.

**Live AB checkpoint, approximately 14:12:** RDR2 PID **126676** remains running in Story.
Startup refused the first SR adapter with `prepare=-5`: the FSR import thunk RVA had been
miscomputed from its dump VA. Correct values are **`+2ea0af5` and IAT `+6032a98`**, not
`+34a0af5/+6632a98` in the older paragraphs. The byte guard prevented the incorrect patch.
Source is corrected; the Release gate passed again (67 executed, 11 opt-in skips, fmt/Clippy),
log `.local/rdr2-tools/native-adapter-rva-fix-verify.log`.

To avoid another game restart, `rsf_rdr2_attach` was built as an explicit research helper.
It borrows the already loaded Runtime, fingerprints the running game and prepares the corrected
plugin from `.local/rdr2-live-attach-20261026/`. It never loads another Runtime or replaces
installed running files. The corrected Game DLL SHA256 is
`d41ca63c0fa79f40dbd802314d08c6058b0db8b173a0c63368055c816861d66b`.
Attachment code is `src/attach_entry.cpp`, injector `.local/rdr2-tools/inject-attach.ps1`.
The old failed plugin was unloaded by the runtime's refused-prepare cleanup before attachment.

**Actual game SR now runs.** Session `336880188` has accepted outputs for backend 1 DLSS,
3 FSR3, 4 FSR4 INT8 and 5 XeSS, changed through the INI without restarting PID126676.
Input/output are **1706x1066 -> 2560x1600**. The game frame-time source is now directly sampled:
first captured frame24582 is 19.5161018ms. RTSSHooks64, d3d11on12 and SocialClubD3D12Renderer
remain loaded; existing queue/Present chains and Social Club correction are retained.
Sixteen fenced SR outputs plus normalized colour/depth/motion were collected automatically,
without asking for F10. All reviewed DLSS/FSR3 outputs are finite and write the bottom/right
edges; update `sr-pixel-validation.json` for the later providers before claiming their pixels.
FSR4's first full-size dispatch took approximately **56.8 seconds** between context creation
tick337060437 and subsequent frame processing337117234. It then returns successful results.
The human identified this and XeSS's first-use pause as model loading. Record the cold-load
timing and distinguish later warm switches; no asynchronous warm-up redesign was requested.

Live evidence: `.local/rdr2-tools/native-adapter-game-ab-20261026/attach.log`, `pixels/`,
`sr-pixel-validation.json`, `launch-time.txt`, `diagnostics-off.ini`. The current game INI has
temporary `SRCaptureDirectory` for bounded evidence. **Restore that saved diagnostics-off INI
after the game exits**, retaining the desired SR settings. Last action requested **SR=0** to
check native fallback after four RSF providers. Next: verify native fallback, restore SR=1 and
backend1, then obtain one native game selection change to FSR so its normalization is actually
exercised. No F10 needed. Native selection is still DLSS at this checkpoint.

Installed Runtime was updated before launch to SHA256
`7286caca8635ce27d5afeb7ff72fc2e8016327a642ee4544637c61202a780948` to add explicit bounded
capture configuration/retirement. The installed Game DLL is still the **incorrect-RVA**
`379025a1...5da5c82c`: **after normal game exit, back it up and deploy the corrected Game DLL**.
Do not replace it while running. Final prelaunch receipt is
`.local/rdr2-tools/native-adapter-deployment-final.json`; backup
`.local/deploy-backups/rdr2-adapter-complete-20261026-135622/`.
No FG, PCL or native menu replacement has been implemented. Do not stop after the SR test as
though the requested integration were complete. The live runtime and plugin remain pinned to
process lifetime; a second active plugin attachment is refused by the helper script.

**13:44 checkpoint:** real native SR replacement is implemented and deployed, with game ABI 17.
`src/native_renderer.cpp` hooks DLSS helper `+26f3a3c`, normalizes native FSR dispatch through
guarded import thunk `+34a0af5` / IAT `+6632a98`, observes the actual DIRECT queue through
`+26c97d4` called by `+26df79b`, and preserves the existing Steam/RTSS Present chain on the
game swapchain at global `+571c7e8`. Runtime ownership is in
`runtime/orchestrator/src/rdr2_sr_host.cpp`. Both native choices use the same camera/input
conversion and shared SR host. Native vendor evaluation is suppressed only after accepted
RSF output; fallback resets both native and RSF histories. Frozen pause projection refuses SR.
This is implemented source, **not yet a successful game run**. FG, PCL translation and native
menu replacement remain unimplemented. Plugin readiness stays zero.

The GPU host fixture passed DLSS, FSR 3.1.5, FSR 4.1.1 INT8 compatibility and XeSS 2.0.2 on the
RTX 4070 Laptop GPU, with external R32_FLOAT exposure and engine input extents 426x240 to
640x360. It checks three unsubmitted slots, fourth-call refusal without waiting for submission,
retirement/reuse after actual Execute, original pixels and actual DXGI Present bookkeeping.
Logs: `.local/rdr2-tools/sr-host-gpu-exposure-{1,3,4,5}.log`. Two shared DLSS defects were found:
native D3D12 initialization dropped application_id, and standalone manual hooking lacked common
Present garbage collection. Both are corrected. External game exposure is now tagged instead
of silently replacing it with a separate auto-exposure estimate. These changes passed the full
Release VS2026 gate, 67 executed tests plus 11 opt-in skips, fmt and Clippy:
`.local/rdr2-tools/native-adapter-verify.log`.

Deployment receipt: `.local/rdr2-tools/native-adapter-deployment.json`; backup:
`.local/deploy-backups/rdr2-native-adapter-20261026-134424/`. ASI SHA256 is
`28f950b6b8fceda6f8453536d71d9fa8717c50185b51a65d4e424f69406209e7`;
Game DLL `379025a1...5da5c82c`, Runtime `6257d1a4...ad398723`. Diagnostics remain off.
INI controls, polled every second: `SR=0/1`, `SRBackend=1/3/4/5` for DLSS/FSR3/FSR4/XeSS,
`DLSSPreset` shared preset ID; omitted `SRQuality` follows the game's actual input ratio.
Backend/context transitions drain submitted work and refuse while recordings remain unsubmitted.
The adapter does not yet change the game's render-resolution producer, so SRQuality is a vendor
mode override, not an implemented resolution control. Do not advertise native menu controls.

The visually selected static foreground-rock ROI now closes scoped nonzero camera reprojection:
AA6/7, render-pixel rectangle `[85,896,938,128]`, 7,520 samples in each of eight temporal sets.
Median residual 0.0427 to 0.1700 pixels versus 1.646 to 11.996 for the opposite sign; predicted
median motion 0.840 to 6.102 pixels. Evidence:
`.local/rdr2-tools/native-run-aa-20261026/static-rock-reprojection.json` and `contact-1.jpg`.
This proves the static terrain region, not object/foliage/particle motion coverage.

The older paragraphs below retain previous checkpoints. The above implementation/deployment
state supersedes their statements that the adapter is absent or the complete AA run is pending.

Upstream was fetched and merged at the human's request. Current committed base is
`9699b4d5f1eae006b1c2793fd1778dac2022ff34`, containing upstream
`84d63e9a4b591ff1d7c2c5c2c80c08837a8c83f5`; the 152 pre-merge modified/untracked files were
backed up under `.local/upstream-reconcile-20261026-120456/`. The merge commit contains upstream
history and committed-history reconciliation only; RDR2 and the other pending local work remain
uncommitted. The integrated Release gate passed 67 tests with 11 opt-in skips, fmt and Clippy
(`.local/rdr2-tools/upstream-integrated-verify.log`). Mor3a remains current at `bc853d0b`.

Run AA is complete and the game is closed. It produced 27 manifests: five loading-screen captures
without DLSS input, followed by 22 bundles with four same-call camera/colour/depth/MV/output groups
each. All captured draw shaders were identified. Actual original pixels, full draw metadata and
88 temporal input sets are under `.local/rdr2-tools/native-run-aa-20261026/`; use `bundle-analysis.json`,
`camera-adapter-validation.json`, `draw-summary.txt` and `contact-1.jpg` through `contact-3.jpg`.
Motion is previous-minus-current in render pixels (NGX scales 1,1). Example moving samples have
median residuals 0.0154 and 0.0156 pixels versus 4.38 and 8.80 for the wrong direction; static
geometry ROI validation remains to be recorded separately. Normal final blit `BB74CEA4` and
the NGX inputs share the exact command-list generation in AA 6/7. UI often records on a different
list, and some captures start after scene output, so do not declare a universal first-draw boundary.
Pause AA25/26 has scene PS `A7D93689`, `A85DBFA9`, `2EACAA61`, then final blit `BB74CEA4`, before
UI. Generic UI issue function is `+0x596828`, final blit caller `+0x5c1254`, and submitter `+0x26df760`.
The diagnostics-off original INI was restored after AA. The display has no HDR support.

New source `src/render_camera.cpp/.h` converts the measured worker view into `rsf_camera_frame`,
removing current jitter from both current/previous scalar projections, deriving inverse/reprojection
in double precision, preserving negative-Z forward, and exposing frozen-jitter disagreement.
Synthetic geometry/reset/invalid-input checks pass; all 88 AA views decode with maximum projection
inverse residual 4.68364e-8. This converter is compiled into the game plugin but not yet wired to
an active RSF renderer. The shared DLSS SR/FG forward axis now follows perspective clip-W sign;
orthographic behavior is preserved. These source changes passed the integrated gate above.

Next adapter hook lead: `+0x26f3a3c` is the game's DLSS evaluation helper accepting command list,
feature handle, NGX parameters and a direct-resource evaluation descriptor. Its wrapper retains
the game's descriptor-heap/cache restoration at `+0x26ce948`, called after success at `+0x26f41cc`.
The NGX frame-time key is zero in AA, and `DLSS.Exposure.Scale` is absent; do not pass zero frame
time blindly to every backend. Static game frame-time source is float RVA `0x59b393c`, multiplied
by 1000 in the FSR branch at `+0x5c7fca`; the read sequence is
`f3 0f 10 05 6a b9 3e 05 f3 0f 59 05 72 74 d4 02`. This producer has not yet been sampled directly.
AA observed states before the native evaluation: colour/motion 192 (PSR|NPSR), depth 32
(DEPTH_READ), exposure/output 8 (UAV). The shared SR runtime instead requires NPSR inputs and
COPY_DEST output, so an adapter must transition and restore the actual states.

The human explicitly instructed continued implementation after the documentation-only stop.
Current source adds NGX lifecycle/evaluation observers, four input/output GPU snapshot groups
per F10 request, camera bytes from those exact calls, tracked resource states, command-list
generations and pre-submission snapshots, full bounded draw metadata, HDR/colour-space metadata,
and a bundle manifest. `tests/rdr2_capture.cpp` runs the actual capture code against D3D12 WARP
and simulated NGX. It now passes 75 draws, a final snapshot past the 64-slot limit, pipeline-stream
shader identification, four colour/depth/MV/output groups, exact raw pixels, and debug-layer checks.
The fixture caught and corrected final-row readback padding and nested-wrapper duplicate draws.
Its own teardown also required a queue drain after DXGI Present, separate from the earlier copy
fence. Debug evidence: `.local/rdr2-tools/capture-fixture-debugger.log`. The NGX before/after
callback order also passes the existing vendor-census test. The complete Release VS2026 gate
then passed 59 tests with eight opt-in skips, fmt and Clippy, including BGRA8 and RGB10 capture
fixtures and command-list reuse. Log: `.local/rdr2-tools/capture-bundle-verify.log`.
`tools/analyze-rdr2-capture-bundle.py` validates the raw input files, camera algebra and numeric
motion comparisons without claiming static-geometry or live-game acceptance. The complete probe
was hash-verified on deployment as `10eb25faeecc5d6ecb8a104c3f758f0f5dc73a0b961f245cbad94a5267e56394`,
and one consolidated run AA was launched with diagnostics enabled temporarily. Its output is
`.local/rdr2-tools/native-run-aa-20261026/`. The later deployment section describes the previous
checkpoint. No new AA capture or game acceptance had been observed at this update. Restore the
original diagnostics-off INI after this run. SR/FG adapter execution and routing are still pending.

## Human requirements and immediate execution correction

- Forced DX12 remains the target. RTSS compatibility is mandatory. Preserve the guarded Social
  Club selector correction. Never manipulate the game's pipeline caches.
- The human confirmed after run AA that this display has no HDR. Retain HDR as an explicit
  untested hardware condition; do not ask the human to capture HDR on this display.
- **All RSF backends use the game's DLSS-shaped inputs.** Both native DLSS and FSR selections
  must route to that common adapter. Native FSR2 execution/validation is not the objective.
- Replace native vendor execution while retaining colour/depth/motion/jitter production and the
  upscale insertion point. Turning off the whole temporal graph would destroy required inputs.
- Investigate replacing the native DLSS/FSR settings rows with RSF controls. This is a requested
  feasibility track, not an implemented feature. If it works, the RSF overlay can be optional;
  this does not remove the RTSS requirement.
- The human supplied repeated captures and explicitly objected to being sent through repeated
  runs for incremental probe corrections. **Finish and verify the complete capture bundle before
  asking for another run.** One F10 request must collect the needed camera, depth, motion,
  colour, shader/PSO, UI-sequence and frame-association evidence together. Preserve existing
  captures; request only missing evidence in one consolidated pass.
- The human explicitly supplied the Mor3a mod/source below. Failing to inspect it before the
  repeated capture work was an execution mistake. Its checkout already existed locally on
  25 October at 18:27. Do not repeat that omission.
- Preserve the current modified tree. No checkout/stash/reset/clean or commits were authorized
  by this investigation. Do not replace game files while RDR2 is running; back up replacements.
- The original Orca dispatch was settled as failed/partial before the human directly resumed.
  Do not reuse task `task_baa5e66b76b1` / dispatch `ctx_99dd0e373207` for lifecycle messages.

## Required reference: Mor3a Vulkan frame generation

- [Nexus mod 10997](https://www.nexusmods.com/reddeadredemption2/mods/10997), version 0.82.
- [Source repository](https://github.com/mor3a/rdr2-vulkan-frame-generation).
- Local checkout: `references/rdr2-vulkan-frame-generation/`, revision
  `bc853d0b4a9e7ba23ee1fe00c2c107092ccd596d` (22 October 2026, 0.82 source).
- Start with [implementation notes](https://github.com/mor3a/rdr2-vulkan-frame-generation/blob/bc853d0b4a9e7ba23ee1fe00c2c107092ccd596d/docs/MAINTAINER-NOTES.md).
  Original code is MIT according to the repository; dependencies retain their terms. No reference
  code or binaries have been copied into RSF by this investigation. Study contracts and verify
  the DX12 equivalent; the project's reference-code rules still apply.

| Source in that checkout | What is useful; what it does not establish |
| --- | --- |
| `src/inputs.cpp` | Hooks successful game NGX SR creation/evaluation, tracks flags per feature handle/generation, rejects FG's own evaluates using `DLSSG.Backbuffer`, reads depth/MV/jitter/reset, prepares copies before evaluation and accepts them only after successful native evaluation. Do not inherit flags from an unrelated or failed feature. |
| `src/input_contract.h` | Finite jitter/scales, nonzero scale, bounded positive extents. These are contract checks, not proof of pixels or coverage. |
| `src/mvecscale.h` | NGX scale converts motion to pixels; Streamline's normalized scale is raw NGX scale divided by the tagged motion extent. Confirm the receiving RSF backend's convention instead of applying this conversion twice. |
| `src/frame_association.h` | Tracks tokens through command recording, secondary execution, submission queues and binary semaphore dependencies; does not use newest CPU timestamp as frame identity. The implementation propagates the maximum token and retires consumed tokens. Its assumptions need review when translating to D3D12 fences/lists. |
| `src/native_hud.cpp`, `native_hud_history.h` | Captures an actual acquired swapchain image before a full-size, single-sample colour pass with LOAD semantics. Records copy command, image acquisition epoch and submitted token, and tags only a matching present. Misses clear the HUD-less tag. This is explicitly a renderpass/dynamic-rendering heuristic, not a discovered RAGE HUD producer or proof of every special mode. |
| `src/hud_capture.cpp` | Similar HUD snapshot path for the Vulkan/D3D12 bridge; tracks renderpasses, framebuffers, views, submissions and capture epoch. The game still renders Vulkan. This is not a native DX12 RDR2 hook map. |
| `src/input_capture.cpp` | Existing bounded diagnostic design: 24 requested depth/MV pairs using four slots, saved constants, command ownership, fence-verified readback, and restoration of original layouts. Depth/stencil layouts are coupled while only depth texels are copied. This should have guided one complete RSF diagnostic pass. |
| `src/resource_state.*` | Observes resource/image state rather than assuming a universal input layout. Inspect the complete implementation before translating ownership or barriers. |
| `src/neutral_camera.h`, `camera_frame.cpp`, `rdr_camera.cpp` | Default camera is deliberately nominal: near 0.05, far 10000, vertical FOV 60 degrees, identity reprojection. It is not the game's render camera. Active ScriptHook registration is disabled; legacy Sampled is normalized to Neutral. Optional derived camera is not the default. This source does not replace our measured camera work. |
| `src/xess_provider.cpp`, `xess_bridge_*`, `dlssg_bridge_runtime.cpp` | Both XeFG and DLSSGBridge use shared Vulkan-to-D3D12 transport. Active input rectangles/origins, current-frame HUD matching, GPU copies and retirement are useful contracts. XeFG uses BACKBUFFER_HUDLESS mode and inverted-depth initialization. |
| `src/vkhooks.cpp`, `slboot.cpp` | Native DLSS-G uses the interposer-owned Vulkan device/queues. Avoid second registration/queue rewriting. Present association and generation suspension are explicit; missing valid DLSS input can suspend generation. |
| `src/ngx_capture.cpp` | Diagnostic NGX init hooks preserve initialization ABI and distinguish the game's initialization from nested vendor calls. Diagnostic resource extraction is separate from the active FG integration. |
| `src/menu.cpp`, `menu_vulkan.cpp` | An Insert-key overlay, not evidence that native RDR2 graphics-menu rows can be replaced. |

Specific input contracts read in `inputs.cpp`:

- Keys: `Depth`, `MotionVectors`, `Jitter.Offset.X/Y`, `MV.Scale.X/Y`, `Reset`,
  `DLSS.Render.Subrect.Dimensions.Width/Height`, `DLSS.Input.Depth.Subrect.Base.X/Y`,
  `DLSS.Input.MV.Subrect.Base.X/Y`. Retain allocation extent separately from active extent/origin.
- Low-resolution MV uses the render rectangle; other MV uses its own extent. Validate rectangles
  against allocation bounds. The depth+stencil view is valid metadata; copy/tag the depth aspect.
- Create flags determine inverted depth, low-resolution MV and jitter inclusion. Motion is
  explicitly marked as including camera motion. This still needs our texel/producer validation.
- Raw pre-tonemapped SR output is not final HUD-less FG colour. The reference explicitly warns
  against enabling that optional tag as though it were the finished scene.
- HDR bridge handling includes packed RGB10 transport, HDR10/PQ BT.2020 colour space and metadata
  forwarding. `xess_provider.cpp` maps mastering primaries/white point in 1/50000 units and minimum
  luminance in 1/10000 units, checks output support, then applies DXGI HDR10 metadata. None of this
  proves the native DX12 game's HUD blend/colour-space boundary.
- The 0.82 notes leave cutscene suspension under investigation. They distinguish stale/missing
  DLSS input from a transition defect. Do not claim universal cutscene/HDR/HUD acceptance.
- FSR3 is disabled and AMD is unsupported in that mod. Its AMD termination guard and restart-only
  presentation switches are not RSF requirements or acceptable substitutes for RSF's scope.
- Its native Vulkan queue fix accounts for every host queue request before reserving SDK queues.
  This explains that mod's startup correction; it is not our DX12 Social Club defect.

## Fingerprint, installation and build

- Repository: `D:\Coding\ReScaleFrame`.
- Game: `D:\SteamLibrary\steamapps\common\Red Dead Redemption 2`.
- `RDR2.exe` 1.0.1491.50, SHA256
  `b56c9548f670654a9b73bf25def3cd73af12e269f6e47dba28a34079adaf465e`.
- PE timestamp `0x65eec92e`, SizeOfImage `0x7397600`.
- Decrypted dump: `dumps/rdr2-20261025/RDR2.exe.dump`, RVA equals file offset, dump base
  `0x7ff7b9de0000`, SHA256 `496fa1bd3cdfb9ba77841f17a3242a554e929bb704da1f6b309e94b39281b7cb`.
- Required gate: `eng/verify.ps1 -Configuration Release -VS2026`. Latest successful log:
  `.local/rdr2-tools/native-stream-verify.log`, 57 passed / eight opt-in skips, fmt and Clippy.
- VS updated overnight. The verified current instance is VS2026
  `C:/Program Files/Microsoft Visual Studio/18/Community,version=18.11.12324.101`.
  The stale cached 18.10 instance caused a configure failure; selecting the installed instance
  corrected it. This was not a game or source defect.
- Latest built/deployed ASI SHA256:
  `d480f183709d5f6646c317b29470673785f4a1853d6e44eb765ff3b4b44583fa`.
  It includes pipeline-stream observation, F10 and a retained final draw beyond the first 63.
  **These latest changes are built and deployed, not yet game-tested.**
- Game was observed closed after the user's run Z. Original INI restored, SHA256
  `a25c1475eb71d4caf218cf9c54812ef6634f33113a6a3ef2fa8595480a6a39b2`.
  Census/capture diagnostics are off; forced DX12 and Social Club fix remain enabled.
- A diagnostic INI is staged under `.local/rdr2-tools/native-run-aa-20261026/`; no AA run occurred.
  Do not ask for that run yet: the complete capture bundle is unfinished.
- Graphics XML at `C:\Users\N1GHT\Documents\Rockstar Games\Red Dead Redemption 2\Settings\system.xml`
  was last observed with DX12, HDR false, `dlssIndex=0`, `fsr2Index=1`, `dlssQuality=1`.
  Despite those indices, the actual active path was DLSS. Do not infer the provider from these
  indices. XML restoration remains to be reconciled with the pre-run backup.
- Original backups: `.local/deploy-backups/rdr2-camera-20261025-2348/` and
  `.local/deploy-backups/rdr2-admin-resume-20261026/`. Later replacement backups have prefixes
  `rdr2-native-corrected-` and `rdr2-stream-probe-`. No cache files were manipulated.

## Camera source: measured and static evidence

Observe the active view at the actual job-worker upscale call, not a global script camera.
TLS index DWORD is at RVA `0x5b403e0`; `GS:[0x58]` selects the TLS array; indexed TLS block
`+0x14d8` is the active view, `+0x14f0` the draw context, and `+0x1ce8` the job.
CPU frame counter RVA `0x542904c` is an observation, not a final GPU/Present lease.

| Site | RVA | Expected first 16 bytes from the matched dump |
| --- | --- | --- |
| Postfx upscale node | `0x5c6e44` | `48 8b c4 44 88 40 18 88 50 10 48 89 48 08 55 53` |
| DLSS thunk | `0x7bb60c` | `8b 81 a8 00 00 00 4c 8d 0c 80 4a 8d 0c c9 48 83` |
| FSR2 thunk, historical observation | `0x7ba10c` | `48 83 3a 00 4c 8b ca 74 27 48 83 7a 18 00 74 20` |
| Perspective builder | `0x26f63fc` | `48 8b c4 48 89 58 08 57 48 83 ec 70 0f 29 70 e8` |
| Derived view state | `0x26f5e40` | `48 89 5c 24 08 48 89 7c 24 10 55 48 8d 6c 24 b0` |
| Current/previous camera setter | `0x26f6dfc` | `48 8b c4 48 83 ec 78 41 0f 10 10 41 0f 10 58 10` |
| Current camera setter | `0x26f6ff4` | `48 8b c4 53 48 83 ec 70 0f 10 12 48 8b d9 0f 10` |
| World-transform setter | `0x26f7eb4` | `48 8b c4 48 89 58 08 48 89 78 10 55 48 8d 68 a1` |
| Reverse-depth selector | `0x26f7474` | `48 83 ec 28 8b 05 62 8f 44 03 65 4c 8b 0c 25 58` |
| Pixel-jitter setter | `0x26f7b1c` | `48 83 ec 28 80 b9 70 06 00 00 00 74 5f f3 0f 59` |
| Worker view setter | `0x1eb420` | `48 89 5c 24 08 48 89 74 24 10 57 48 83 ec 20 65` |
| Job worker | `0x2d2687c` | `48 89 5c 24 08 48 89 6c 24 20 56 57 41 54 41 56` |

Full static instructions: `.local/rdr2-tools/camera-static-20261025.json`.
Only the two thunk entries are hooked by the probe. Producer execution was not directly traced.
DLSS return is `0x5c7d89`; historical FSR return `0x5c8057`; job dispatch return `0x2d26a07`.

| View offset | Established interpretation |
| --- | --- |
| `0x00` | World transform; identity at the observed upscale calls; not the camera transform |
| `0x40`, `0xc0` | Current world-to-view and camera-to-world |
| `0x80` | Current jittered projection |
| `0x140`, `0x1c0` | Previous view and camera-to-world |
| `0x180` | Previous-camera scalar projection with **current** jitter in the observed path |
| `0x280` | View rotation without translation |
| `0x630`, `0x634`, `0x640`, `0x66c` | Near, far, vertical FOV degrees, aspect |
| `0x65c`, `0x660`, `0x664`, `0x668` | Base X/Y projection offsets, additional X/Y jitter offsets |
| `0x671` | Reverse-depth selector byte, statically traced |
| `0x674..0x688` | Viewport x/y/width/height/minDepth/maxDepth |

Matrices use row vectors. Camera-to-world rows are right/up/back and position; game world is
Z-up, visible camera forward is negative camera Z, depth is reversed. Numeric world units have
been compared with ScriptHook, but a physical metres-per-unit calibration has not been measured.
`P[2][0]=baseX-2*jitterX/width`, `P[2][1]=baseY+2*jitterY/height`; viewport size is rounded with
`floor(value+0.5)` by the jitter setter. Remove current jitter from `+0x180` before applying an
independently retained preceding-frame jitter. Do not call `+0x520` inverse view-projection:
that hypothesis failed with residual over 500,000. Analytic inverses were constructed separately.

Run W (`.local/rdr2-tools/camera-run-w/`): 128 DLSS calls across eight workers/views; max
view/inverse error 0.000176135, projection/analytic-inverse error 1.22445e-8, jitter error below
1.97616e-8 pixels, previous-view error zero. Near 0.15000000596, far 10000, FOV 37.801094 degrees
(0.659753552 radians), viewport 1485x928. Capture 2 sample 1 has `(0.25,-0.3888888955)` jitter.
Historical FSR scalar values were 0.14999111 / 10000 / 0.659771383 radians; a direct synchronized
FSR comparison is not required by the clarified human scope.

Run X (`.local/rdr2-tools/frame-run-x-20261026/`): 320 DLSS samples; 192 same-counter ScriptHook
matches establish position error <=5e-7 world units, FOV <=5e-8 degrees, forward <=1.92e-7.
Changing association by one frame produces materially larger errors. Script callbacks run on
the ScriptHook fiber, not the render worker. In pause, projection freezes while DLSS jitter
continues, producing up to 0.84375 pixel disagreement. Do not accept a continuing NGX call as
proof of a fresh view. Reference hashes: coordinate `0x5352E025EC2B416F`, rotation
`0x602685BD85DD26CA` (order 2), FOV `0x04AF77971E508F6A`; vector return elements occupy 8-byte slots.

Run Z adds 640 camera samples. Max view/inverse error 0.000625158, projection/analytic-inverse
error 1.12991e-7, FOV error 6.15553e-8 radians, maximum jitter mismatch 0.6875 pixels. Of 630
adjacent sample pairs, one has previous-view disagreement 460.292679: capture 9/sample 60,
CPU frame 43401. At that sample, current view equals previous view exactly and parameter byte
`+0x80` becomes 1. Static DLSS wrapper `0x26f3f10` propagates this byte into the evaluation reset
field. This is reset/history evidence, not a reason to discard the camera source. All other
adjacent previous-view comparisons are exact. Raw bytes and `camera-analysis.json` preserve it.

## HUD pixels and special modes: evidence that must not be lost

RenderDoc run X has five actual `.rdc` files: world 7695, Eagle Eye 8173, Dead Eye 9323, map 9691,
pause 10784. Embedded thumbnails contain the real scene. **Replay produces a black 3D scene
while UI reproduces.** Successful API opening is not successful pixel replay. Same RTX4070 Laptop
GPU was used; the capture log warns about unsupported NvAPI extensions. Exact vendor failure
is not isolated. Do not repeat those replay attempts as proof of original scene colour.

Captured DX12 topology in frame 8173: tonemap/LUT E74162 PS `4FF4CC58` to resource 50791,
2560x1600 R11G11B10_FLOAT; final scene output E74185 PS `DBCF46F8` to BGRA8 swapchain 391;
offscreen 512x512 minimap draws E74231..74304; first main UI E74313 PS `CA8E18BF`, followed by
`2E866023`, `EE341D7A`, `1A0D957F`, `CA8E18BF` through E74413. First UI RGB blend is
SrcAlpha/InvSrcAlpha, alpha One/InvSrcAlpha. World frame 7695 instead has `1D1EEAC6` then
simple output blit `BB74CEA4`. Shader binaries, assembly and PSOs are under `world-tail/` and
`eagle-tail/`. UI shaders have conditional premultiplication/LUT/PQ logic; not every variant
has the same convention.

Controlled replay replacing scene output with transparent black produced a premultiplied
UI-only layer: `eagle-ui-layer/`, 148170 pixels with nonzero alpha, max(RGB-alpha)=0.
This validates that replay experiment, not original scene separation in every mode or HDR.
`world-diagnostics/` has depth/MV texels. A nearly stationary cliff comparison had median
residual about 0.00121 pixel, but predicted motion was near zero, so direction/scale and exact
camera association remain unproven. Do not mark motion reprojection verified.

Native run Z (`.local/rdr2-tools/native-run-z-20261026/`, PID 116608) is the first successful
original DX12 pixel readback. ASI hash `6ce5e24a52f6943f78716533f0fb981e0ec0ab4c3b6dc02c4306aa72fafa37af`.
There are **586 completed raw per-draw readbacks across 12 captures**, each 2560x1600 BGRA8 SDR.
`native-analysis.json`, `roi-analysis.json`, `contact.jpg`, raw `.bin`/`.json`, camera snapshots,
ScriptHook logs, `native-probe.log`, `hotkey.log`, and the census are retained. Census final:
55313 Presents, 51703 DLSS evaluations, no dropped records. No FSR2 evaluation was required.

| Capture | Observed content | Result and limit |
| --- | --- | --- |
| 1 | Normal world, initial remote request | 34 draws. First copy contains intact scene without HUD; final adds HUD and RTSS. 144173 pixels differ in localized HUD/OSD regions. |
| 2-4 | Normal world in rain | 34 draws each. Corresponding HUD-less and final images preserved. |
| 5-6 | Weapon wheel | Reached old 64-draw cap. Draw 1 already changes about 3.84 million scene pixels before wheel drawing; first copy is too early for the completed mode-specific scene. |
| 7 | Eagle Eye / blue-tinted mode | 34 draws; localized subsequent HUD changes. |
| 8 | Pause menu | Reached old cap. Draws 1, 2 and 3 make full-screen changes; draw 3 changes 4088147 pixels. Do not reuse the normal-world first-copy boundary. |
| 9 | Map | 60 draws. First copy is already map content, not a HUD-less 3D scene. Draws 56/57 each replace all 4096000 pixels; later output restores the map. |
| 10 | Dead Eye / orange-tinted mode | 36 draws. Draw 1 changes 1355361 pixels across the screen, including a small change outside localized HUD regions. |
| 11 | Journal/book | Reached old cap; this is not photo mode. |
| 12 | Log/objective menu | Reached old cap; draw 1 changes 3879937 pixels before menu text. |

Independent scene ROI `[1800,300,2500,1050]`, away from wheel/pause/log UI, contains 525000 pixels.
It stays identical through normal/Eagle Eye later draws. Weapon-wheel draw 1 changes 492350
pixels there; pause draw 3 changes every ROI pixel relative to draw 2; log draw 1 changes 495501;
Dead Eye draw 1 changes 40024 with mean absolute byte difference 0.04918. These original pixels
disprove a universal assumption that everything after first scene output affects only UI.
The specific named POSTHUD/LUT nodes have not yet been mapped to those native draws.

RTSS is present in the final native images at the top edge. Its observed late shaders include
PS CRCs `CB54F2A6`, `D70DAF42`, VS `88A78FCD`; blend RGB 5/6 (D3D12 SrcAlpha/InvSrcAlpha),
alpha 2/6 (One/InvSrcAlpha). Census stacks establish D3D11-on-12 through RTSS and the game's
Present. Native game draw returns are `+0x26a7246` and indexed `+0x26a71e8`.
Social Club's existing correction is dispatcher `+0x27f09e8`, renderer DLL selector `+0x4b89`:
use the supplied recording-buffer index when rendering into the game's own command list.
An actively visible Social Club overlay was not separately pixel-validated in Z.

## Probe implementation and failures

- `src/camera_probe.cpp/.h`: expected-byte guarded pass-through thunk capture, raw copied view,
  params, TLS/job metadata, optional ScriptHook comparison. v1 records are 2416 bytes; v2 are
  2568. View begins at byte 112, parameter block at 2160, v2 extras at 2416.
- `src/native_capture.cpp/.h`: optional D3D12 observer, actual swapchain-buffer RTV matching,
  copies after draws, exact RT state restoration, queue fence signalled after original submission,
  asynchronous file writer and buffer-reference retirement. Default off.
- `loader/census/include/rescaleframe/census_d3d12_observer.h` and `src/wrap_d3d.cpp`: optional
  device/after-submit/before-Present callbacks. They do not change the public game ABI.
- First native launch Y failed census install with `-6`: probe initialized MinHook before census.
  Corrected so census owns initialization. Z proves the correction and successful native copies.
- Z game shader hashes were zero because the probe observed descriptor PSO creation only.
  Static game instructions at `+0x26a2bbc` call pipeline-library `LoadPipeline` (slot 13), and
  `+0x26a2bdb` calls Device2 `CreatePipelineState` (slot 47). Latest source observes both stream
  paths, including reused caches. Never delete caches to force another creation path.
- Z capped snapshots at 64 and lost the tail of UI-heavy frames. Latest source retains the first
  63 draws plus a reused final-draw slot with the last submission fence. Intermediate draws after
  63 are not retained; a complete metadata sequence is still needed. This correction is built,
  not live-proven. F10 is now directly in the probe; Z used `tools/watch-rdr2-capture.ps1`.
- Current native probe **does not yet copy DLSS depth/MV/SR input/output in the same capture**,
  record all UI metadata after the cap, or establish full queue-to-Present frame ownership.
  Those are pending source work, not features that can be enabled by INI.
- `tools/analyze-rdr2-native-capture.py` exports first/last previews by default, all previews with
  `--all-previews`, and computes raw pixel differences. It decodes BGRA8/RGBA8/RGB10A2; HDR PNGs
  preserve encoded values and are not tone-mapped luminance measurements.
- `tools/analyze-rdr2-camera-probe.py` produces camera algebra and frame comparisons.
- Computer-use native pipe disconnected; retry and session reset failed. The shell is elevated,
  but that does not repair the pipe. No shell-driven GUI/input workaround was used. User input
  supplied navigation. The F10 helper observed keys and wrote requests; it did not inject input.
- Python on PATH resolves to Inkscape's GUI Python and can return before processing finishes.
  Use an explicit interpreter and wait for its process. Python314 has NumPy but no Pillow;
  Inkscape Python has Pillow/NumPy. Do not confuse detached processing with completed analysis.

## Other pinned leads and tools

- `references/rdr2-cfx-camera/`, Cfx `46de79bc1d4dd7c64f16c5cfbadf94612b5ab3d9`:
  `grcViewport` 0x40/0x80/0xc0 agrees with measured fields. Its name at 0x140 does not override
  measured previous-frame semantics. Signature `+0x1507b2a` points to global `0x3ec2610`.
- `references/rdr2-cyberfsr-camera/`, `ed00f4a0022e273e8e4e938b70781d481d3b868d`:
  scalar signature `+0x3212b7` -> `0x3ec2450`, fields +0x60. No worker-frame ownership proof.
- `references/rdr2-ultrawide-camera/`, `0a000d93f9dbb56633a34ffc64cbf1ed7750a2b4`:
  exact-build camera application `0x170028`, letterbox store `0x320545` -> `0x39751b4`,
  UI boxing `0x1cbd30`. Separate script subtitle/layout paths are reported.
- `references/rdr2-renodx-dx12/`, `f20c3c3003c29fc17697a7ab2ddf1cdc9389b0b0`:
  nine tonemap and ten output/PQ shader candidates; author labels them untested. Several
  match run-X shaders. Output `cb22[9].y` HDR gate, `cb22[10].x` LUT gate, t32/t35/b22/b23.
- `references/rdr2-research-nativedb/`, alloc8or `e8558ab9`: final rendered camera natives;
  no near/far getter. `references/rdr2-research-shdotnet/` supplied vector/export ABI cross-check.
- Native-menu leads: femga `rdr3_discoveries/databindings/uiapps/README.md` describes
  `settings_menu` graphics/pause_graphics entrypoints. Halen84's native-menu base draws a menu;
  it does not demonstrate replacing graphics settings. `.rdata` near `0x367b82e` names
  `rage__fwuiSettingsItemInfo_*`, `rage__fwuiGraphicsSettings`, `rage__fwuiSettingsGroup`,
  `dlssIndex`, `dlssQuality`, `dlssSharpen`, `fsr2Index`, `fsr2Sharpen`.
- `.local/rdr2-tools/peek.py`, `rdis.py`, `callers.py`, `mdump.py`, `names.tsv`,
  `camera-static.py`, `match-public-camera.py`, `compare-script.py`, `motion-check.py`,
  `rd-inventory.py`, `rd-tail.py`, `rd-pixels.py`, `rd-diagnostics.py`, `rd-ui-layer.py`.
  Capstone path: `.local/ac7-motion-tools`. RenderDoc: `C:\Program Files\RenderDoc`, version 1.46.
  Serialize RenderDoc replay helpers using `rd-request.json`; stop heavy jobs before game launch.

## Remaining work, in order

1. Use the Mor3a contracts above to finish the single bounded DX12 capture bundle. Capture real
   DLSS inputs and output, actual resource states/rectangles, exact same-call camera/parameters,
   full draw metadata including pipeline streams, final scene/UI pixels, colour-space/HDR metadata,
   and command-list/submission/present identity. Retain bounded ownership and a clear completeness
   manifest. Verify it before requesting another human run.
2. Reuse X/Z to identify mode-specific scene-effect and UI producers. The first swapchain draw
   is not universally the completed scene. Validate nonzero camera-motion reprojection against
   raw motion/depth, retaining reset and frozen-pause behavior rather than hiding those cases.
3. One consolidated missing-evidence pass: complete shader-identified normal/wheel/pause tails,
   photo mode, cinematic camera, letterboxed cutscene, HDR, and visible Social Club overlay as
   needed. Existing journal/log captures are not substitutes for photo/cinematic modes. No
   additional FSR2-provider run is required. Keep RTSS active.
4. Implement the common DLSS input adapter and both-selector routing, then native menu controls
   if feasible. None of SR/FG execution, selector routing, replacement settings, or native UI-only
   extraction has been implemented by this research. Keep readiness false and report that plainly.

Authoritative companion records: [renderer research](../../docs/research/rdr2-renderer.md),
[engine evidence](engine.json), [implementation tracker](../../docs/implementation.md).
Earlier entries describing no captures or unexecuted ScriptHook are historical, superseded by
X/Z above. Keep these records synchronized; never promote build/source evidence to live acceptance.
