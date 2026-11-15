# Skyrim renderer FG handoff, 15 November 2026

## Question and evidence

The renderer must supply depth and motion independently of SR, before native
post-processing or RSF auxiliary reconstruction changes their contents, then
supply the HUD-less scene and window ownership through actual Present.
This change is a source review and implementation, without a new game or GPU
experiment. The matching executable is SkyrimSE.exe 1.7.104.0, SHA256
`846efccf0c1374d71f892907f46549560f2fcb0a75cb87a3eed438baa0f1402f`.
The existing [integration research](skyrim-se-integration.md) records native
resource/camera producers, the guarded hook bytes, and QASmoke evidence that
the pre-interface scene has no HUD and after-interface bytes equal final Present.
Commit caa5201 records the corrected shared SR transfer device regression;
it does not establish game acceptance of this handoff.

## Implementation and execution

The guarded main post-processing call at RVA 0x153ade7, caller return 0x153adec,
now reports FG_INPUTS before calling reconstruct. It reads the already researched
renderer owner at RVA 0x3331f40: colour target 1, motion target 7, and depth
at RVA 0x3333f58. No D3D creation-order inference is used. The separate native
input reader uses the existing SR reader's camera cache, format, device,
allocation, rectangle and history guards. This keeps FG input reporting
independent of the SR callback and auxiliary reconstruction result.

Camera cache selection matches native camera RVA 0x33d3d28 against entries
at state RVA 0x33363a0 + 0xb8, stride 0x290, and requires the jittered entry.
Motion is R16G16_FLOAT, decoded by the runtime using the host's motion contract;
its canonical motion_to_uv is (-1,-1), current minus previous UV with Y down,
including camera motion. Native depth is R24G8_TYPELESS. Active render rectangles
and output allocation dimensions remain separate. History discontinuities set
RESET and do not silently reuse previous jitter. The first sample seeds history.
The native TAA/camera prerequisites still gate reporting; missing facts retain
the native path rather than supplying fabricated zero motion or camera data.

The synchronous FG_INPUTS begin/end callbacks lease native textures and context
only during those calls. Shared host ABI3 owns normalization, GPU copies and
retirement, including SR Off. The adapter performs no vendor processing or
GPU copies and retains no native motion/depth pointers after reporting. AddRef
would preserve an allocation, not frame contents. FINAL_SCENE must therefore
consume the runtime-owned retained copy rather than rereading those pointers.

The pre-interface hook at RVA 0x116ac80 retains its primary-UI guard. Before
FINAL_SCENE it checks current source, submission and scope as well as the
existing session/native-frame/view/window checks. FINAL_SCENE explicitly leases
the actual swapchain and backbuffer. WINDOW keeps COM leases for the same
swapchain and immediate context through RendererEnd and its actual Present;
its end callback uses that leased context rather than rereading a global.
Retirement clears saved identity even when no window was opened. Diagnostic
capture availability no longer gates native scene/window reporting.
The metadata paired with pre-mutation inputs survives the later SR callback's
report of the same session/source/submission/scope/native-frame/view.

The adapter declares only ABI3 FG_INPUTS capability and enables the cold shared
presentation mechanism, based on the corrected device transfer evidence.
INPUT_TIMELINE is not declared. Existing NO_FG and unknown screen remain:
the CPU worker found that native polling has a separate manager mode and that
safe physical-input association is still unproven. Runtime capability and
per-frame facts gate provider admission; this is not an unconditional adapter
switch that claims working FG. No SKSE dependency was introduced. Existing CS
refusal remains; CS/ENB coexistence and exclusive SR/FG ownership are open.

## Validation and remaining work

Only source review and git diff --check were performed in this worker. No build,
GPU fixture, game launch, deployment or acceptance test was performed because
parallel workers were active and the coordinator owns focused validation.
Build rsf_game_skyrim_se and rsf_orchestrator, run the focused Skyrim plugin,
D3D11 host, frame assembly and native input contracts, then eng/verify.ps1.
Check the coordinator's current CMake target names before invoking them.
A separate authorized device/game run must cover SR Off retention, SR On,
provider switching, same-source Present, resize, quiescence and retirement.

Working FG remains blocked on native input ownership and gameplay/menu
classification, followed by provider-generated presentation validation. The
known vanilla HUD-less boundary does not prove CS/ENB composition, arbitrary
screen transitions, motion coverage for all effects, latency or moving-image
quality. No gameplay class was inferred from a camera or rendered backbuffer.


## Bounded late-enable reduced-input fixture extension

The preceding initial-admission discussion is historical. Subsequent coordinator
work admits FSR rendered sources conditionally through native primary World and
GAMEPLAY facts, with physical input timing and pre-input pacing still unavailable.
The coordinator reports that the original explicit device fixture passed with
30 sources, 26 captures, 15 SR evaluations and 22 generated callbacks after its
versioned latency-status query was initialized correctly. Callback counts are
not confirmed physical-display counts.

That fixture enabled FSR before native-resolution SR. It did not reproduce the
live transition from warmed reduced-resolution DLSS to FSR generation. The
coordinator's newer bounded live experiment (PID46488) corrected commonPresent,
reported successful FSR switch, then exited C0000005 without an observed debugger
AV/second-chance event. Artifacts remain ignored under
`.local/skyrim-research/fg-monitored-20261115-205820` and
`monitored-fg-results.log`. This is a reported live failure, not a device-fixture
result or proof of its exception site.

The explicit rsf_d3d11_host_rendered_fg_device fixture now starts FG Off, applies
DLSS Quality through the public overlay settings API, and warms exactly ten
sources at640x360 in a960x540 native R11G11B10 allocation. Native R24G8 depth and
R16G16 camera-inclusive zero motion retain full allocation dimensions. The SR
colour input and output alias the native allocation. Pixels outside the active
input rectangle start with a marker; a bounded readback after warmup checks that
full-output SR reinsertion overwrites that marker in the same allocation.

FSR3 is then enabled through the same public overlay settings API while DLSS
remains active, followed by exactly ten more sources. Receipts separately log
provider backend/switch result/generation, retained inputs, tagged sources,
application Present calls, image-finalization callbacks and vendor statistics
flags. The latter are a bitmask, not a display-valid boolean. First transition
sources allow provider startup before requiring capture. A successful receipt
requires later reduced sources to be captured/tagged and generated callbacks to
occur; no callback receipt produces explicit incomplete status, not acceptance.

The original twelve SR-Off, twelve native-resolution SR-On, reset, continuity,
non-primary and three FG-Off sources remain. SR-Off callback assertions compare
against the late-enable snapshot so earlier generation cannot satisfy them.
The experiment is bounded to fifty sources total and uses a hidden window with
no foreground request. FG_INPUTS precedes SR mutation, and native final/window
leases bracket actual Present as before. Unknown input QPC, no pre-input pacing,
zero latency/PCL/report assertions and refusal checks remain intact.

This synthetic engine does not reproduce a game's native post-processing draws,
auxiliary depth/motion expansion, native resource restoration, cached engine RTVs,
menu/input concurrency or full-screen composition. It therefore isolates late
enable, reduced regions and in-place SR, but does not close the live failure or
establish mod compatibility. Engine-specific auxiliary policy was not copied into
the generic fixture or runtime.

Only fixture source and this note changed in this extension. Source review and
git diff --check passed; no worker build, GPU run, game launch or commit occurred.
The coordinator can build only rsf_d3d11_host_rendered_fg_device and run it once
against the matched scratch SDK root. The fifty-source extension is uncompiled
and unexecuted until that focused check; the earlier passing receipt applies to
the original thirty-source fixture only.


### Complete bootstrap lease correction

The coordinator built the extended explicit target without rebuilding its
references and ran it once with the matched scratch SDK. It exited before
warmup at settings_get: the dimensions-only unassociated bootstrap packet did
not pass the public host's complete scene admission checks and therefore never
configured the overlay. Evidence is the ignored
`.local/skyrim-research/rendered-fg-late-enable-20261115.log`. This is a harness
startup failure, not a failed reduced-SR or late-FSR evaluation.

The incomplete packet is removed. The same source-owned frame producer now
submits exactly one complete bootstrap source with real camera/texture leases,
CPU facts and actual Present while both SR and FG are Off, before querying or
applying overlay settings. It uses the public host process_upscale admission
path, not overlay.configure or any internal runtime shortcut. The earliest
FRAME/SUBMISSION attempts precede first graphics admission just as during cold
native startup; bootstrap supplies no generation eligibility receipt. Its CPU
and final/window leases close normally and its source ID is never reused.

Summary counters explicitly separate one bootstrap source from fifty test
sources, for at most fifty-one source-owned application Presents plus the
original cold swapchain startup Present. Bootstrap must not retain FG inputs or
evaluate SR. Ten reduced warmups, late enable and all original checks remain.
The coordinator owns the next single focused build/run. This correction was
source-reviewed and git diff --check passed; no worker build, GPU run, game or
commit occurred, and successful bootstrap admission is not yet a run result.


### Passing late-enable fixture and post-final-copy live audit

The coordinator rebuilt the explicit target against the already built corrected
Runtime and ran the matched scratch SDK once. The fixed-bootstrap extension
passed exit0:51 sources, one bootstrap plus50 tests,37 captures,35 SR evaluations,
32 tags and32 generated callbacks. Its late-enable phase recorded10 captures,
10 application Presents,9 tags and9 generated callbacks on provider3/generation2.
Reduced in-place reinsertion, SR-Off/SR-On, reset, non-primary and FG-Off checks
passed, with input QPC, pacing, markers, effective latency, PCL and physical latency
report availability all zero. The receipt is ignored
`.local/skyrim-research/rendered-fg-late-enable-bootstrap-fixed-20261115.log`.
This is coordinator-reported device evidence, not physical scanout or game quality.
The earlier uncompiled/unexecuted statements describe the state before this run.

The actual game remains a separate failure. PID44752 exited C0000005 after
provider switch. Its last first-generation-source receipts were source204,
native374, successful normalize/decode/resolve/capture and final-copy/final-done.
The log is ignored `.local/skyrim-research/fg-frida-20261115-210900/loader.log`.
Seven passive termination hooks were reported armed, with no termination entries.
That absence does not identify the exception or establish how the process exited.
No new live observation was made by this audit worker.

Source inspection narrows the interpretation of this trace. FINAL_SCENE end
calls native_fg_final, which queues CopyResource and a D3D11 shared-fence Signal,
then emits final-done. It does not join GPU completion at that point, so result0
is a queued-copy/signal receipt, not completed GPU validation. Its local source
QueryInterface reference releases on return; the adapter's current backbuffer
COM lease remains alive throughout snapshot. The adapter then retains matching
swapchain/context COM references, copies scalar window metadata, clears the
borrowed scene_surface field, opens WINDOW, invokes the optional bounded capture
callback, releases local snapshot references and finally calls the native UI
trampoline. The WINDOW implementation records a copied thread-local scope and
does not execute UI drawing or vendor GPU work.

There is currently no WINDOW-entry or native-UI-entry/return stage in this trace.
Consequently, absence of prepare/provider/Present stages does not prove the
failure happened before WINDOW or inside native UI; those intermediate portions
are uninstrumented. The next durable exception/stage observer must distinguish
host-final return and its local COM destructors, WINDOW entry/return, capture
return and native trampoline entry/return, while retaining any exception address,
thread and stack. No new permanent option is justified by this evidence.

The saved exact-build decompilation in native-interface-boundary-20261115.json
identifies RVA0x116ac80 as __int64 fastcall with one pointer-sized argument and
an integer return of the restored TLS profiling value. This matches the adapter's
uintptr_t(void*) trampoline call and the guarded prologue. The older Community
Shaders reference declares its corresponding thunk void(int64_t); that source
lead does not override the exact-build result or establish a missing register
argument. The C++ hook contains no custom assembly or hand-written stack changes.
Static ABI agreement does not validate executable trampoline bytes at failure.

The normalized-input path's RAII state scope encloses region preparation,
decode/resolve and capture, then restores D3D11 bindings before native SR/UI.
The state helper retains Get-returned COM references while saved and restores
all six stages' SRVs/constants/samplers, shaders, OM/CS UAVs, RTV/depth, IA,
viewport/scissor, stream-output and predication before balancing those references.
The final HUD-less copy itself does not change those graphics bindings. The
bridge's D3D11 facade surfaces survive provider changes; only explicit surface
recreation replaces them. This source review found no demonstrated native cached
RTV dangling lease or UI ABI defect. It cannot prove GPU state correctness for
Skyrim's actual inherited bindings or hidden UI/driver consumers.

This audit changed only this note, with git diff --check passing. No worker
build, fixture/GPU/game launch, binary analysis mutation or commit occurred.
Compatibility-module/plugin work and runtime tracing remain owned by their
workers; no speculative adapter or shared repair was made. Native auxiliary
restoration and real UI draws remain outside the passing fixture's coverage.


### Native outer creation root and final facade ownership

The newer durable PID41028 fault receipt records a null read at dxgi+0x8e3a
through dxgi+0x863a, RTSS+0x72ed1, Steam overlay+0x9c24f,
Skyrim+0x100a016 and Entry+0x22fe. The chain-valued R14/R15 match the bound
engine chain. This supersedes speculative attribution to native UI. It does not
by itself establish whether external creation wrappers replaced an inner bridge
facade; native creation/output ownership must be observed explicitly. Evidence
is ignored `.local/skyrim-research/fg-frida-20261115-211524/termination.jsonl`.

Read-only installed PE inspection identified d3d11.dll import IAT RVA0x17c92e0.
The saved matching database0044e197 was reopened as skyrim_creation without
new auto-analysis. Xrefs show game thunk0x15b9412, bytes FF25 C8FE2000,
with a sole direct code caller at0x1012320, bytes E8 ED705A00, return0x1012325.
Renderer initialization0x1012060 supplies all12 documented
D3D11CreateDeviceAndSwapChain arguments. The engine root at0x3330188 owns
chain output+96, device output+56, immediate-context output+64 and HWND+72.
Later0x101242a publishes root+72 at0x3330198, independently confirming the
existing adapter's window+0x18 chain access. Native arguments specify selected
adapter, UNKNOWN driver, native flags/descriptor and caller-owned output slots.
The root function was named RSF_Research_Renderer_CreateNativeD3D11, guarded
call/thunk comments added, and the existing database saved. Exact retained
static evidence is ignored `native-outer-creation-root-20261115.json`.

The new adapter native_creation helper hooks only that guarded game-owned
import thunk. Its MinHook trampoline still jumps through the current native IAT,
so the original D3D11/RTSS/Steam wrapper sequence runs with every original
argument before RSF touches returned outputs. The detour validates the actual
return address, native root output addresses and HWND, then calls the generic
host ABI4 rsf_d3d11_host_wrap_native_swapchain service. The request is size/version
checked and carries borrowed returned device/context plus the caller-owned
swapchain reference in/out. The adapter preserves native HRESULT, GetLastError,
device/context/feature-level outputs and descriptor. Native creation failures,
unmatched callers/owners and facade refusals keep the native result; no direct
Present implementation or Present-vtable bypass is introduced.

Host setup now requests generic presentation_bridge mode2, prepare-only with
no inner factory detours. The runtime owns facade creation and verifies identity;
only success replaces the caller's chain reference and releases that original
reference. The adapter neither loads SDKs nor selects provider policy. Cold-owner
checks refuse a prepared device that already exists. Expected-byte checks announce
refusal through plugin status/logging, and the outer-return log records native
HRESULT, separate service result and before/after chain pointer. Runtime identity
logs must then show the final engine pointer, canonical IUnknown, facade owner and
Present entry before graphics activation. This is a guarded ownership mechanism,
not evidence that wrappers rewrote the earlier chain or that the crash is fixed.

Plugin prepare/start/quiesce/stop manage the root hook and active call count;
the existing compatibility profiles and checkpoint are retained. Only the new
helper, narrow plugin/CMake glue and this note changed in this worker. The
completed compatibility author's dispatch could not be reactivated from this
worker; coordinator confirmed its changes were done and creation-root scope
remained assigned. CPU ownership and shared runtime implementation stay with
their respective workers. No builds, GPU/game runs, deployment or commits were
performed; source review and scoped git diff --check are the current validation.
Coordinator must build the integrated frozen sources once and validate cold
creation output identity, refusal/fallback, window/resize ownership and the
bounded actual game transition before any acceptance claim.


### Bounded actual-game outer-root receipt, PID50256

The coordinator compiled the ABI4 outer-root implementation and ran one bounded
actual Skyrim experiment. The prior failure point was passed on this build.
The ignored report root is
`.local/skyrim-research/fg-frida-20261115-213310/`, with `loader.log`,
`run.json`, `termination.jsonl`, exact trial files and symbols under `trial/`,
original files under `backup/`, and `restoration.json`. The separate
`.local/skyrim-research/outer-creation-abi4-results.log` contains the public
overlay status snapshots, PresentMon failure and actual process exit code.

Cold creation records native chain0x253f5ef4440, canonical IUnknown at that same
address and Present in dxgi+0x84c0, followed by final facade0x253f60fcec0,
canonical IUnknown at the facade address and Present0x7ff999155eb0 in Runtime,
with owner1. The adapter records original native HRESULT0 and separate wrap
result0. The engine overlay subsequently binds that exact facade address.
Runtime provider switch identity logs retain it across generation2/provider3.
This is direct evidence of the new path's final outer creation output and engine
binding. It does not establish which external overlay, if any, replaced the
prior path's pointer: the earlier path lacks an equivalent outer-return capture.

Repeated post-switch receipts show bridge application entry, prepare/configure,
provider prepare, physical/vendor Present, retirement and application completion
returning0. The earlier dxgi null-read failure was not observed in this bounded
run. The process completed normally with ExitCode0 before the watchdog deadline;
its termination trace records zero-status exit calls. System/C++ first-chance
exception entries also exist in the trace, so this receipt must not be described
as an exception-free run.

The sampled public overlay API table status reports DLSS Quality1280x720 to
1920x1080, SR result0, source306,106 tagged frames,477 application Presents,
provider-reported212 output presents and runtime rates58 to116. FG request1,
effective generated count1, provider3 and result0 are separate from display
confirmation. The same overlay snapshot reports fg_active0 and the reason that
the provider is inactive, while completed-frame loader receipts report
mode1/generated1/active1. A shared-worker status snapshot correction remains
separate work; this note retains both original observations rather than silently
rewriting the trial metadata.

PresentMon could not start its ETW session: access denied, reported exit6.
Consequently there is no independent physical display-cadence receipt, latency
measurement or broad visual/gameplay acceptance from this experiment. Native
input reservation work is still ongoing; the rendered-source run does not claim
pre-input pacing, physical input ownership or low-latency availability. Menus,
loading transitions, long sessions, moving-image quality, CS/ENB coexistence and
broader device/vendor coverage remain distinct acceptance gaps.

The trial hashes recorded in run.json are Entry
`2dfdd87666654b11c5d080f4c3258e985ce9dd59334ef31c8e9b09b7d108b4e1`
and Runtime
`0a09d2cb50d6d46989f6890afef2f2f8c0f86d4066d6a0a7f7af16a5c6cfd2c0`.
The coordinator restored the original working installation and closed the game.
restoration.json independently records verified restoration of Runtime
`8a485dafcdf55ede5c0bd4e157f00ddb21d7ab379c6be4e750909428da7f02bb`,
Entry `2c2977b1f8a5d2581d568469353c29c28b27fbf04d0308acdf3a0e2c8c5486c0`,
the original FidelityFX DLL and INI. Those hashes identify this receipt's scope;
the successful experimental binaries were not left installed as general acceptance.

This checkpoint updates only this owned research note after reading the retained
coordinator artifacts. Creation implementation remains frozen. No new worker
code edit, build, fixture/GPU/game launch, deployment or commit was performed;
scoped git diff --check passed for this note.
