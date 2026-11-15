# D3D11 callback host FG capabilities, 15 November 2026

The question was how a callback adapter can expose shared FG and latency controls
without embedding vendor policy or claiming that renderer identity proves input
ownership. Source review of d3d11_host.cpp and overlay_api.cpp found that the host
hardcoded FG unavailable and latency Off, while the existing overlay already saves
settings and forwards native FG options and presentation backend requests.

Host ABI 3 appends capabilities. FG_INPUTS declares synchronous FG_INPUTS plus
FINAL_SCENE, FRAME, SUBMISSION and WINDOW leases; INPUT_TIMELINE declares actual
input consumption associated with source CPU facts. Both are required to expose
FG choices; INPUT_TIMELINE permits latency choices. A presentation bridge is
required for either declaration. The runtime selects providers and clamps actual
capabilities through existing shared overlay/presentation policy. Capability is
neither runtime input admission nor acceptance. Missing capabilities retain Off
controls and emit a refusal explanation. Existing native_fg reason/status remains
the authority for individual frames.

FG_INPUTS begin borrows the adapter's pre-mutation native colour, depth, motion,
region and camera on the admitted immediate-context owner. It normalizes region
inputs, decodes motion into R32G32, then resolves R16G16 previous-minus-current
pixel displacement and R32 depth. Camera-inclusive motion and explicit UV scales
are currently required. Valid zeros remain valid when sentinel detection is off.
The native_fg capture owns copied shared surfaces and retirement; scratch outputs
remain alive through that synchronous copy and are reused in immediate-context
order. No native output is written and no SR vendor startup is required. Provider
Off does not prevent attempting valid input retention; absence of a bridge owner
still returns NotReady. Full source/submission, scene/window, HUD-less and CPU
eligibility checks are preserved. FINAL_SCENE still supplies HUD-less colour.

RSF_SCREEN_GAMEPLAY is appended as value 8 without changing frame structure layout.
The common eligibility helper already allows it while respecting NO_FG. Shared
ownership diagnostics expand from eight to nine entries to avoid aliasing gameplay
with loading. No existing class changes meaning.

The latest Skyrim research records the caa5201 normalized shared DLSS positive
RTX4070 run and identical old-Runtime negative control. That is evidence for SR
transfer, not this new FG path. This change received source review and diff
whitespace checks only: the coordinator will build and run focused host checks
after concurrent edits settle. No build, GPU fixture, deployment, game run or
latency measurement was performed by this worker. Actual input-source admission,
shader coverage, moving-image FG quality and device acceptance remain separate.

## Rendered-source admission correction

Physical input sampling is not an interpolation input requirement. The initial
host gate coupled FG to input-keyed pacing and therefore also refused FSR, whose
provider has no required pre-input pacer. Primary contracts reviewed on 15 November:
[Streamline DLSS-G section 8](https://github.com/NVIDIA-RTX/Streamline/blob/main/docs/ProgrammingGuideDLSS_G.md),
[Reflex sections 4-5](https://github.com/NVIDIA-RTX/Streamline/blob/main/docs/ProgrammingGuideReflex.md),
[Intel XeLL sleep and markers](https://github.com/intel/xess/blob/main/doc/xell_developer_guide_english.md),
and [AMD frame interpolation swapchain](https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK/blob/main/Kits/FidelityFX/docs/techniques/frame-interpolation-swap-chain.md).
These live upstream guides were compared with local provider source; this is a
source/contract review, not SDK/device acceptance. DLSS-G requires Reflex and
XeSS-FG requires XeLL. Their lack of a physical input marker requirement does
not permit a sleep after input was already sampled. They remain refused here.

RENDERED_SOURCE capability (4) is mutually exclusive with INPUT_TIMELINE (2).
Together with FG_INPUTS (1), it admits the separate runtime rendered-source path.
Frame ABI 2 appends source provenance, native FRAME-begin QPC and its frequency;
input_qpc remains zero. Engine frame delta is used when valid, otherwise an actual
continuous native-FRAME delta. No substitute timestamp becomes physical input.
CPU source/session and actual simulation-end facts remain required, along with
exactly one primary native submission/view, complete FRAME ownership, matching
final-scene/window/HUD-less resources, history/reset, provider generation and GPU
retirement. NO_FG refusals remain effective, including recovered bound identity.

The cold bridge constrains all provider choices and switches to FSR3/4. The core
requires pacing None, latency Off and limiter zero; it never sets paced after a
no-op call. It suppresses input/PCL markers and reports physical input/latency
unavailable. The measured-input path retains its original event, pacing and
marker semantics. An independent ledger entry class records only real submission
and presentation boundaries without fabricating simulation/input markers.
Provider SDK requirements still constrain each entry class.

Pure FG_INPUTS GPU preparation skips while FG is Off. The shared capture endpoint
also skips generation retention while Off, after allowing any already armed bounded
input dump to observe SR inputs; HUD-less retention already has an Off guard.
Normal SR processing and scene/window ownership continue. This avoids imposing FG
copy/resolve work just because the adapter can report leases.

Focused contract checks cover unknown-input provenance, rejection of rendered
sources by a pre-input-pacing session, successful no-pacing session ownership and
marker ordering without SDK begin/marker calls. No build or test was executed by
this worker during concurrent integration. A coordinator-run build, gate and
explicit device/game validation remain required; no lower-latency or completed
DLSS/XeSS claim follows from this source change.

The CPU normalizer also has a cold rendered-source class. It accepts actual
FRAME_BEGIN/SIMULATION_END/FRAME_END facts (mask 1 to 9 to 25), or the additional
reported SIMULATION_BEGIN (mask 1 to 5 to 13 to 29), and retains missing input
sampling at zero. The measured-input normalizer still derives and labels its
existing substitutes. Rendered CPU records cannot emit a physical INPUT_SAMPLE
or pacing fact. Focused latency-event checks cover both rendered orderings and
assert zero substitutes and zero invented input timestamps.

## FSR provider initialization and staged SDK comparison

The coordinator's integrated Release build succeeded and its native gate reached
CTest with two failures out of 232: the measured-input Anomaly switch from DLSS
SR to FSR3 FG returned runtime INIT_FAILED (-6), and cold FSR3/SR-Off retained
plain presentation with no tags. DLL presence was not sufficient evidence of a
working provider. Review found no new FSR provider-source edits preceding the
failure, and the bridge's zero allowed_backend_choices preserves legacy provider
selection and switches. The runtime error occurred before FG input admission.

Failure-stage diagnostics now retain the underlying FFX result, selected effect
provider/version ID, requested API version, SDK directory, swapchain dimensions
and chain presence. The bridge separately logs provider-create failure and its
allowed mask. These diagnostics remain generic, with no game or version switch.

The coordinator then compared an identical fixture executable and current Runtime
against the old staged SDK root and a scratch clone whose only changed SDK binary
was amd_fidelityfx_framegeneration_dx12.dll. The staged file is 4.0.1.2740,
SHA256 02297BEEDD285E822D3A64F314CF00FAF378DCEC0EDC47FF0C4DD71B3A8C2F18;
the matched local signedbin file is 4.0.2.4555,
SHA256 E6843252F58B0341A0D80F6A20BC0D840643EE95BC48C22CAC08745344577BB6.
Current compile-time generation API is 4.0.2, swapchain API 4.0.0.

Both binaries successfully create generation context for selected FSR provider
3.1.6, ID 17726168133342859270, FFX result 0. The old binary fails swapchain
creation with FFX result 6 and chain absent; the matched binary returns result 0
with a chain. The installed ffx_api.h decodes 6 as
FFX_API_RETURN_ERROR_PARAMETER, distinct from result 7 for a provider not
supporting a new descriptor type. The result does not identify which parameter
or internal validation differs. The earlier hypothesis of general generation
context/descriptor incompatibility is therefore too broad: generation creation
works in both, and the reproduced difference is DLL-specific swapchain behavior
under identical caller descriptors. No backend-mask regression is evidenced.

Old-root fixture exit is 1, tagged count 0. Matched-root exit is 0, runtime tagged
count 272, vendor presentation count 590 and 273 active-FSR3 samples. This proves
this synthetic measured-input SR-Off fixture with the matched binary; it does
not prove moving-game quality, rendered-source admission, measured latency or
compatibility with every older/newer DLL. Pair current headers with the matched
4.0.2.4555 signed binary for the validated test setup. This is an empirical
validated pairing, not a claimed universal minimum compatible version.

Evidence is retained in ignored .local/skyrim-research/ files:
fsr-provider-old-sdk-diagnostic.log, fsr-provider-matched-sdk-diagnostic.log,
fsr-provider-comparison-results.json and fg-provider-matched-root.txt; the scratch
root is .local/fg-provider-sdk-comparison-20261115-202429. The coordinator ran
these bounded device checks; this worker reviewed their outputs and did not run
builds or GPU fixtures. No installed game files changed. The remaining switch
case and explicit rendered-source device fixture still require coordinator
validation, followed by the remaining format/lint checks.

## Real-game provider switch termination and common Present bookkeeping

The coordinator reports a passing explicit rendered-source device fixture and
passing matched-SDK switch control. These synthetic checks do not establish the
real-game switch. In the second bounded Skyrim run, PID 20352 became unavailable
about 22 seconds after launch, before the 50-second watchdog, immediately after
live FSR3 enablement. Before enablement, status reported DLSS Quality success,
1280x720 to 1920x1080, and 472 application Presents. The final log records
successful FSR generation/swapchain creation and effective provider 3 generation
2. A later status read failed with WinError 5. No Application 1000 record was
found by the coordinator. This establishes loss of process/status availability,
not the exception address, exit code, faulting module or underlying cause.
Evidence: ignored rendered-fg-game-second-20261115.log and
rendered-fg-second-results.log in .local/skyrim-research/.

Source review found a separate, verified lifecycle omission explaining the
FG-Off presentCommon warnings. rsf_dlss_native_present returns success without
hooks when shared_host is used. Off and FSR physical swapchains are native or
vendor-owned, so their Present bypasses the Streamline swapchain proxy. No other
shared-host call invoked common Present hooks. Reference Streamline source at
revision 2122257e0fce486f91b385aa63b9a09b0a34b363 explicitly calls presentCommon
from slHookPresent and afterPresentCommon from slHookAfterPresent. This is
bookkeeping/garbage collection, independent of input sampling or PCL markers.

A generic host service now resolves those common-only hooks and the shared
bridge calls them around a physical application Present when a host exists and
the active provider is not DLSS. DLSS proxy presentation retains its own hooks,
preventing duplicate common calls. Begin refusal stops presentation with a log;
end refusal is logged after the actual Present. No input/latency markers or
sleep are introduced. This change has source/whitespace review only and is not
a demonstrated fix for the termination.

The D3D11 switch's drain signals the Streamline queue, whose native_queue is
obtained by unwrapping that same queue in create_host. Therefore a claim that
it fails to drain Skyrim's native presentation queue solely because the two
interface pointers differ is unsupported. Native D3D12 can have a distinct
engine queue, a separate issue outside this D3D11 crash conclusion. No speculative
queue or provider lifecycle change was made without evidence.

### Prepared bounded exception/exit experiment

An existing headless monitor binary is available at
D:/Coding/VulkanStory-Rewrite/artifacts/gate-r24/stage/VulkanStory/native/win-x64/VulkanStory.CrashMonitor.exe,
3273824 bytes, SHA256
30BE885770805D78563BEBC0C02605F0B3CF42229C2AA678AA4473200054CF02.
Its source is native/bootstrap/crash_monitor.cpp in that external checkout;
the original CLI takes PID, report directory and readiness-event name. It uses
DebugActiveProcess, disables debugger kill-on-exit, records debug exception and
process-exit events and captures a terminal exception minidump. Current source
also has optional first-chance diagnostics and postcapture report packaging;
those are unnecessary here. The source and existing binary were located/read,
not executed or claimed to be a matched build. No cdb or ProcDump was found in
PATH or the inspected standard debugger/tool directories.

Proposed coordinator-owned experiment: verify one newly owned Skyrim PID's exact
executable path/hash and fresh report directory, attach this monitor with its
original three arguments and hidden window before live settings apply, confirm
attachment from its monitor log, then perform one FG-Off baseline and one FSR3
apply under the existing 50-second run bound. Record whether the process emits
a second-chance exception (module/RVA, registers, threads and minidump), exits
normally with a code, or remains alive until the watchdog. If the monitor or
ready signal fails, stop without applying FG. Continue terminal exceptions
normally; do not suppress them or label watchdog termination a game crash.
The coordinator should verify the external monitor's deployed CLI before use,
retain helper exit/attach receipts, and ensure it detaches or exits after the
owned run; no global debugger registration is needed. A dump/exit trace is the
missing evidence required to identify the switch fault. Nothing was launched,
installed or changed in a game directory by this worker.

## Monitored termination without an AV debug event

The coordinator rebuilt the common-Present correction and ran one bounded owned
PID 46488 with verified CrashMonitor attachment/readiness before settings apply.
DLSS Quality baseline succeeded. No presentCommon warning or common-hook refusal
was observed, supporting the bookkeeping repair for this bounded path. FSR context,
swapchain and switch again succeed, followed by actual process exit 0xc0000005.
Monitor records its initial breakpoint, thread-name exception 0x406d1388 and two
first-chance C++ exceptions 0xe06d7363; it records no AV or terminal exception
event and therefore no exception context/stack. Exit status 0xc0000005 alone is
not an AV address or proof of an unhandled memory fault. An in-process termination
API request is a hypothesis; an externally initiated exit remains possible.
Evidence is in .local/skyrim-research/fg-monitored-20261115-205820/ (run.json,
monitor.txt, crash.txt, loader.log) and monitored-fg-results.log. The coordinator
restored working game files/settings and removed owned QA state after the run.

Only bounded diagnostics were added next: native FG stage receipts cover
normalization entry, regions/decode/resolve, retained-input copies, final HUD-less
copy, configuration and provider preparation; at most 96 receipts per provider
generation are emitted while FG is requested. Shared bridge receipts cover the
first four application Presents after each switch, callbacks, command-slot entry,
physical vendor Present and retirement. Each includes generation, thread, source
and native frame where available, plus results. They change no admission,
resources, synchronization, provider selection or exception handling.

### Next coordinator-owned termination trace

Prepared ignored script:
.local/skyrim-research/trace-fg-termination-20261115.js.
Syntax checked with node --check only; no runtime instrumentation was executed.
It requires Windows x64 and SkyrimSE.exe, installs observation-only Frida attach
hooks for KernelBase/kernel32 TerminateProcess, ntdll NtTerminateProcess,
ExitProcess, RtlExitUserProcess and optional fail-fast/abort exports. Export aliases
are deduplicated. It records at most 32 termination entries with status/handle,
caller, thread and at most 24 module/RVA stack frames. Accurate unwind is preferred;
fuzzy fallback is explicitly marked as candidates. It does not replace calls,
change arguments, install an exception handler, suppress exits or read input values.
Observers detach after 30 seconds. Frida interception itself can perturb timing.

REA 6.9.0 is installed. The ephemeral command was verified with its actual help:

```powershell
rea instrument-with-frida attach <new-owned-pid> --device-id local --script-file D:/Coding/ReScaleFrame/.local/skyrim-research/trace-fg-termination-20261115.js --duration-ms 32000 --format json --full-output
```

For an explicit armed receipt before applying FG, use the available MCP persistent
operations instead of waiting for the ephemeral command to return. Installed
registerFridaTools.js confirms these exact inputs:

1. start_frida_session with mode attach, pid equal to the newly verified owned
   PID and device_id local; never spawn or reuse PID 46488.
2. load_frida_script with session_id from that result, source_kind file and path
   D:/Coding/ReScaleFrame/.local/skyrim-research/trace-fg-termination-20261115.js.
3. frida_session_status with that session_id and detail complete; require matching
   armed PID/path, installed termination hooks and no script error before the
   single live FSR apply. Do not proceed from a timeout or absent receipt.
4. Save complete status/Evidence after the bounded apply, including module/RVA
   return address and accurate-versus-fuzzy classification, even if target exits.
5. close_frida_session with that session_id on success or failure; retain cleanup
   evidence and verify no observer/owned helper remains after the run.

Use the existing 50-second owned run bound and the same baseline/apply sequence;
correlate native-stage receipts with the termination entry, not merely the last
loader-log line. A self termination API stack would identify the requesting
module/RVA and caller chain, not necessarily the earlier defect that prompted it.
No termination entry despite process exit would leave external termination,
unsupported entry paths or observation loss unresolved. This experiment is
prepared for the coordinator, not performed by this worker; no build, GPU run,
install, registry change, game launch or commit was made.

## Synchronous exception/termination receipts after the final-scene boundary

Runtime SHA256 C2AD8F02A689DD1591970C08B4C95F15C6F2203732AA65388690DA5CEC4E29E9
reproduced exit 0xc0000005 in coordinator-owned PID 44752. The first post-switch
source 204/native frame 374/generation 2 logs successful normalization,
retained depth/motion copy and HUD-less final copy/signal submission. No prepare,
provider-prepare or physical-Present receipt follows. This narrows the observed
CPU interval; it does not prove execution reached SDK preparation, nor do the
copy/signal receipts prove GPU completion. A passing 51-source extended device
fixture with warmed reduced DLSS, late FSR enablement and in-place input did not
reproduce a generic late-enable failure; its scope omits the real native UI and
adapter state. Coordinator reports 35 SR evaluates, 37 captured inputs, 32 tags
and 32 callbacks, with zero physical-input/pacing/PCL claims.

The passive REA session 6fe2372a-f2d4-40cd-8e00-33989c3f7127 armed seven hooks
before baseline/apply and later detached without termination-entry/error receipts.
A 16 MB evidence bundle is retained under ignored fg-frida-20261115-210900,
including loader.log and rea-evidence-bundle.json; final status evidence is
ev_af6cecbf87234bd13cf804d8f22bf5afcd8270927861a936dd8337ab11fc2e07.
No received termination message is not proof that no termination API ran: abrupt
exit can lose queued messages. The earlier PID 38100 trace expired before apply
and captured a normal exit-zero entry, so it is only a clean baseline control.
The separate controller GetFileHash-autoload failure happened before FG and was
recovered/restored; it is not part of the game-fault evidence.

Production code remains frozen. Read-only review places the next CPU operations
after final-done at local source COM release, host callback unwind, adapter WINDOW
lease construction/report and original native UI work. A deferred driver fault
from previously submitted GPU commands could also surface there. These are
candidates, not a ranking established by a stack; no synchronization, state or
ownership change was made from temporal proximity alone. The adapter worker is
independently reviewing its UI/cached-state interval.

The ignored trace now requires an explicit coordinator prefix:

```javascript
globalThis.RSF_TRACE_CONFIG = {
    expected_pid: /* newly verified owned PID */,
    output_path: /* fresh absolute .jsonl inside D:/Coding/ReScaleFrame/.local/skyrim-research/ */
};
```

It exclusively creates that one fresh file (mode wx), writes and flushes each
JSONL receipt synchronously before asynchronous send, and writes the minimal
termination/exception record before attempting unwind. It adds an observational
Process.setExceptionHandler that records exception type, operation, fault address,
PC, x64 register values, module/RVA and bounded stack, and always returns false,
including recursive or observer-error paths. No exception is suppressed and no
context/argument is changed. It retains caps of 32 termination entries, eight
exception records and a 30-second armed window, then removes the exception
handler and hooks. Only the selected receipt file is written; no directories,
registry values or target memory are changed by caller-authored code. Frida
interception and exception observation still have runtime overhead.

[Frida's primary JavaScript API documentation](https://frida.re/docs/javascript-api/)
confirms File.write is synchronous, flush drains buffered file data, and a false
exception-handler result leaves handling to the application. This improves
process-exit durability, not power-loss durability. The coordinator must confirm
both a flushed armed file record and matching REA armed receipt before applying
FG; unsupported exclusive-open mode or a path/configuration error must stop the
experiment before apply. New script SHA256 is
38C39F8DE69ECE1EC677D8B440B4D96D37734EA47404E6386D5D6114379BDA89.
Only node --check was run; the revised observer has not been injected or device
tested by this worker. Working files remained restored and no game/build/GPU run
or commit was performed.

## Durable AV evidence and outer creation-root admission

Coordinator-owned PID 41028 supplied the first actual fault context. Durable
exception 8 at time 1794773743101 is a null read at dxgi.dll RVA 0x8e3a on thread
42576. Accurate unwind includes dxgi+0x863a, RTSSHooks64+0x72ed1,
gameoverlayrenderer64+0x9c24f, Skyrim+0x100a016, Entry+0x22fe and native renderer
callers. Registers R14/R15 are 0x1f116c2be40, matching the overlay-bound chain in
the loader log; later NtTerminateProcess receives status 0xc0000005. No Runtime
or Bridge Present frame or first-post-switch Present receipt appears, despite
successful final-done source 162/native 332. This disproves treating the exit
status alone as the fault location and supplies a real DXGI fault, but the stack
does not assign fault ownership to RTSS, Steam or DXGI. The observed path bypassing
the expected application facade, or a mismatched/altered interface dispatch, must
still be distinguished with COM and method identity receipts.

Evidence: ignored fg-frida-20261115-211524/termination.jsonl, run.json, loader.log
and frida-fg-durable-results.log. No game or GPU run was performed by this worker.

Source review identified an admission defect independently of that unresolved
runtime mechanism. Host capabilities followed successful bridge installation or
preparation rather than proof that the engine retained the final facade. The
old observer selection did call is_owner and could fall back to a native observer;
it did not universally skip that check. That fallback nevertheless allowed the
bridge-enabled host to proceed with a foreign application chain. The host now
refuses bridge-enabled binding unless the engine supplies the exact active facade.
CPU vendor normalization also waits for that verified binding; installation
alone authorizes no pacing/token traffic. Native engine fallback remains possible,
but it is not reported as shared FG ownership.

Host ABI 4 adds a generic post-outer-creation service, agreed with the adapter
worker. presentation_bridge mode 2 prepares runtime services without installing
factory detours. The game adapter owns the exact guarded creation call site and
executes its original twelve native arguments through existing external creation
wrappers before invoking this service:

```c
typedef struct rsf_d3d11_native_chain {
    uint32_t struct_size, abi_version;
    void* device;
    void* immediate_context;
    void** swapchain;
} rsf_d3d11_native_chain;
int32_t rsf_d3d11_host_wrap_native_swapchain(void* services_user,
    const rsf_d3d11_native_chain* request);
```

The adapter preserves the original creation HRESULT and all native arguments.
Device/context are borrowed; *swapchain holds the native caller-owned reference.
The generic runtime verifies size/version, host and pre-bind lifecycle, immediate
context, device/context/chain IUnknown identity, native descriptor/factory and the
adapter's window predicate. It creates the final outer application facade using
the existing create_bridge mechanism and checks exact active ownership before
publishing it. Success replaces *swapchain and releases only the original caller
reference; refusal leaves native output pointer and caller ownership intact.
No native Present is bypassed and no overlay is disabled. Physical chain creation
retains the existing lazy behavior, so successful wrapping is not proof of future
physical creation, provider switching or game compatibility.

Runtime receipts now name the native and final facade pointers, canonical
IUnknown identities and vtable Present entries, plus old/new physical chain and
provider pointers around a switch. These are generic COM facts rather than game
symbols or detection policy. The renderer worker owns guarded game thunk/caller
and renderer output-slot checks; no game RVA or layout enters runtime files.

Focused host contract checks cover wrong owner, short call, wrong ABI and
preservation of original output before graphics access when outer-root mode is
unavailable. Existing mode 0 and mode 1 remain accepted; mode 3 is rejected.
The coordinator must build/run these checks and validate actual outer-wrapper
coexistence with the new identity receipts. Only source and whitespace review
were performed here; no coexistence or new game acceptance is claimed, and
physical input/low-latency work remains separately incomplete.

The coordinator's integrated ABI4 Release runtime/Skyrim build succeeded, while
the host contract test failed compilation because its new native-chain request
reused the existing render-pass variable name outer. That request is now named
native_creation_request; production code is unchanged. A stale pre-ABI4 test was
subsequently invoked after the failed test build and faulted; that invocation is
invalid validation and is discarded, not evidence of a production fault. The
coordinator will rebuild only the test against frozen built DLLs with project
references disabled. This worker performed only source and whitespace checks.

## First bounded real-game FSR activation through the final facade

The coordinator rebuilt the ABI4 host contract test and Skyrim plugin contract;
both passed. Bounded actual-game PID 50256 then used mode 2 successfully. Native
chain 0x253f5ef4440 had Present in dxgi+0x84c0; the returned final facade was
0x253f60fcec0 with Present in Runtime at 0x7ff999155eb0 and owner=1. Engine and
overlay subsequently bound that same facade. FSR switch selected backend 3,
generation 2, followed by repeated application/prepare/physical vendor Present
and retirement receipts returning success. This directly validates the outer
creation mechanism for this deployed wrapper setup, rather than inferring it
from successful installation or a recognized executable.

At the sampled point, source 306 had 106 tagged frames, 477 application Presents
and 212 vendor output Presents; observed counter rates were 58 application and
116 vendor output per second. DLSS SR remained successful at 1280x720 input to
1920x1080 output. Trial Runtime SHA256:
0A09D2CB50D6D46989F6890AFEF2F2F8C0F86D4066D6A0A7F7AF16A5C6CFD2C0;
Entry SHA256:
2DFDD87666654B11C5D080F4C3258E985CE9DD59334EF31C8E9B09B7D108B4E1.
The bounded run exited normally with code 0 and restoration.json verifies original
working files/settings hashes. No game or observer remains. Evidence is retained
under ignored fg-frida-20261115-213310/ and outer-creation-abi4-results.log.

PresentMon could not start its ETW session: exit 6, access denied. No privilege or
group membership was changed. Vendor counters and FFX callback activity are not
physical scanout/cadence measurements. This run proves bounded actual-game
activation and continued SR/FG operation, not general visual acceptance across
scenes/menus/mod combinations, lower latency or physical frame cadence.

### Completed-Present activity status correction

The same run exposed a status-only bug: external overlay status reported
fg_active=0 and provider inactive while eligibility reason was zero, effective
generated count was one and completed vendor logs repeatedly reported active=1.
Overlay sampling replaced the native completed cache with a fresh provider query
from a pending source; FSR prepare explicitly clears its transient active field
until after-Present. The two phases were therefore mixed.

The existing status path now publishes an internal completed-Present snapshot
only when actual successful Present source and provider generation match the
prepared source/generation. Overlay refreshes capabilities/configuration while
retaining completed activity and cumulative presentation/callback counters. It
does not infer activity from a tag, request or higher count. Current Off,
reset/menu/refusal, unsupported capability, status errors, lost owner and provider
generation changes suppress activity. Mode/count changes deactivate retained
activity, so rapid Off/On cannot resurrect a previous completed active frame.
The snapshot helper is internal-only; no public output struct or C ABI changed,
and no CPU timing/sleep/token policy was modified. Tests added to the existing
fg_session target cover pending inactive state versus completed activity, live
capabilities, exact completion source/generation, failures and inactive transitions.

Only source review and focused diff whitespace checks were performed by this
worker. Implementation is frozen for the coordinator's final batched checks;
focused targets are fg_session and d3d11_host_contract plus the existing overlay
contract checks in that batch. No new runtime validation, game/GPU fixture,
build or commit was performed here. The updated user goal limits follow-up to
necessary integration/menu/verified shader fixes; no new telemetry or optional
display-statistics mechanism is introduced by this correction.


### Prospective input source integration, 15 November 2026

Question: can the existing source reservation facts place the single runtime pacing
operation before an actual future poll, without moving the native poll or treating
FRAME_END as completed presentation? The native timing/consumption evidence and
remaining refusal sites are recorded in
[the prospective-source investigation](skyrim-prospective-source-20261115.md).
This change is shared-core source implementation, not new game acceptance.

Validated v2 source facts now reach the existing native CPU sink synchronously,
after releasing its ledger lock, only outside rendered-source mode. At RESERVE the
existing CpuOwner and provider token are acquired for the prospective identity.
The existing completion event supports a bounded 20 ms wait, but its wake alone
never authorizes pacing: the reservation ledger must report the exact predecessor's
successful actual Present (or no predecessor). Pending timeout, failed, cancelled,
unverified and superseded outcomes refuse this source. FRAME_END never satisfies
this dependency. The existing vendor selector then performs the one pacing
operation; None and inactive Anti-Lag do not set paced or publish a pacing duration.
The existing diagnostic labels describe the pacing operation and input-to-simulation
interval, since a prospective sample can precede its consuming tick. FRAME_BEGIN reuses the
reserved token and publishes timing from that earlier operation.

Only a sampled reservation adopted by FRAME_BEGIN and subsequently bound to native
consumption promotes poll_begin_qpc into the CPU INPUT_SAMPLE stage. This actual
poll entry may predate FRAME_BEGIN. Neither RESERVE nor poll completion is used as
an input sample; no synthetic input event, input-received count or PCL ping is
created by binding. Pending unframed/window observations are not assigned to a
prospective token as consumed input. The simulation-start marker is emitted only
at the real SIMULATION_BEGIN after BIND. A substituted simulation start refuses
this prospective owner. Old packets without poll_begin_qpc remain unknown and do
not gain input ownership. The existing partial-appended-timestamp rejection is
preserved.

Aborted, superseded and unbound reservations withdraw unsubmitted tokens through
the existing owner lifecycle. A later callback observes implicit ledger withdrawal;
quiescent session teardown clears outstanding ownership. Provider generation must
still match the source. Measured FG retains native CPU stages, actual input,
submission/view, final scene/window, HUD, resource/reset and retirement checks.
Only FSR with pacing None, latency Off and no limiter may proceed without a pacing
operation; DLSS/XeSS still require actual provider pacing. The older input-keyed
path and rendered-source FSR path retain their existing behavior.

Focused source coverage extends native_cpu_reservation: synchronous callback
ledger reentry, one delivery for duplicate-refused facts, actual input predating
FRAME_BEGIN, no invented marker/pacing/input/PCL counts, real simulation ordering,
unknown old-size packets and abort-before-bind. Existing rendered-source and short
packet cases remain. The worker performed source review and diff whitespace checks
only, with no builds, tests, GPU runs, deployment or game launch. Coordinator batch
validation should include native_cpu_reservation_contract, d3d11_host_contract,
fg_session, latency_host and existing latency/provider contracts.

Skyrim INPUT_TIMELINE remains disabled. The later offline IOManager getter
resolution in the prospective-source note closes 331F570 virtual +80 controls
reachability. Bounded native consumption acceptance remains an integration
prerequisite; this implementation does not establish lower measured latency or
physical scanout.
