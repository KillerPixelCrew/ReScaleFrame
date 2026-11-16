# Actual latency boundaries, 16 November 2026

The user requires actual native source/input markers and forbids synthetic
markers. Automatic capture is disabled. SR/FG was accepted for the tested path;
latency integration now remains the active work.

The supplied Address Library ZIP was rechecked: SHA256
385ceddcf7c44d6c50cbf67eea82387f1e036447e1aabc7832ad690ce5c8bb56.
Its selected version-1-11-240-0.bin matches SHA256
65985cc2259384a13cffb538d74776e422e62b0cd3766485a000620242b72b06.
There are14 ZIP entries including directories,12 file members. The existing
address-library research remains reusable. Native Fallout4.exe matches
fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8.

Community Shaders revisione0e711594291c1a8e447ac758099492514810fbb
places BeginSimulation after its WindowsMessageLoop thunk and pairs simulation
end/render start through RegisterPreSwap. These reference placements are leads,
not proof of the actual sampled input or full simulation span in this executable.
REA/IDA is analyzing the matching executable; the C34FA0 query remains in flight.
The existing matched motion-history dossier for C34FA0 establishes actual player
update instructions and actor calls, but does not alone prove the complete
simulation boundary. No new native hook is claimed from that dossier.

The core marker validator now rejects substitute-input timing and derived
simulation stages. D3D and Vulkan sinks do not send timing markers for those
facts. D3D12 compatibility input-sample translation no longer fabricates a
simulation-begin event. Actual simulation-begin events drive that marker.
Render-submit no longer fabricates simulation-end to make marker order pass.
Derived ledger bookkeeping remains distinguishable from vendor markers; it is
not accepted as marker authority. Until actual native callbacks supply missing
boundaries, affected paths must refuse measured latency/FG admission rather
than claim valid markers. This source change is not deployed.

Game ABI24 appends an optional version3 receipt packet to the existing event
callback. It carries receipt identity and observed HWND without a frame claim.
Core admission validates session/frequency/flags, deduplicates a bounded64-ID
reordering window, and counts receipts separately from sampled frame input.
No adapter emits this extension yet, and no vendor notification routing is
claimed. The current D3D sink deliberately emits no vendor marker for it.

Runtime build and focused receipt/latency-event/reservation contracts passed.
The first latency contract expected an order-violation increment for a rejected
synthetic marker; that expectation was corrected to reflect refusal before
marker submission. No game/device run or new capture was performed.
Remaining: verify real simulation/input producers, wire actual receipts and
window provenance, route PCL notifications to live exact-owner tokens, finish
XeLL/Anti-Lag policy/capability review, and targeted live latency verification.

## Actual receipt producer wiring

Fallout4 now emits version3 from its already guarded RAWINPUT handlers,
controller delivery callback and successful selected PM_REMOVE calls. Each
callback gets a monotonic receipt ID. PeekMessage receipts preserve the actual
MSG.hwnd; raw/controller callbacks report window0 when no target is proven.
There is no guessed source-frame ID or vendor meaning in the adapter.

The shared core validates and counts these receipts immediately; the Runtime
helper exposes receipts-status independently of sampled-input/frame counters.
This closes receipt observation loss without routing unframed receipts to an
arbitrary frame token. Vendor PCL notification association remains unfinished.
The adapter and receipt fixture build; receipt, latency-event and reservation
contracts pass. The packet is not yet game-tested.

The fresh REA/IDA C34FA0 request remains in flight. OS inspection found its
idalib worker alive with increasing CPU use; no second import was started.
Retained exact-executable assembly confirms the player update's actual calls,
while complete simulation-span ownership still requires the enclosing native
call chain. The source changes deliberately leave missing markers unavailable.

## XeLL/Anti-Lag source-policy pass

Local XeLL header defines simulation0/1, submission2/3, Present4/5 and optional
input-sample6. The Vulkan endpoint previously emitted XeLL simulation-start
after sleep and input-sample on input-arrival events. It now uses actual
SIMULATION_BEGIN and INPUT_SAMPLE facts. The explicit execution endpoint emits
submission-end on the actual SUBMISSION end callback; PRESENT begin no longer
fabricates that missing boundary and refuses if it is absent. The legacy Present
helper also no longer emits render-end as a side effect. Required XeLL pacing
mode is unchanged; no second sleeper or adapter vendor policy was introduced.

Anti-Lag2 local SDK README requires Update before input polling and the existing
core owner implements that location and single-owner exclusion. Its DX12 FSR
end-render/private-data path remains distinct from DX11. This review is not a
hardware test. The mock-driver contract checks unsupported adapters, SDK failures,
mode/limit changes, duplicate frame updates and exclusion behind Reflex/XeLL.

Runtime build and four CPU-only contracts pass: latency_input_event_contract,
latency_antilag2_contract, native_latency_vulkan_contract and input_receipts_contract.
Two old Vulkan assertions encoded inferred marker placement; they now report
actual input/simulation/submission ends and assert that Present cannot fabricate
submission end. A duplicate CMake target added during discovery was removed in
favor of the existing contract targets in anomaly_support.cmake. No Anomaly game
fixture, GPU test, capture or game deployment was run.

## Matched native callback span

Fresh REA/IDA completed against the pinned executable. C34EB0 calls Poll1672540
atC34EC3, dispatches to PlayerControls via1673AE0 atC34EE7, then dispatches the
second native receiver atC34EFA. BD6850 calls this wrapper atBD6940 and the
matched OnIdle_UpdatePlayer C34FA0 atBD694C. Its later branches schedule/wait
native jobs. C339D0 contains the main-thread non-worker Poll/consume path and
calls Swap C33250 atC346AC, followed by worker mode/join atC346B8/C346BB.
C33250 is rendering: renderer state setup, world render calls C38E30/C38240,
UI paths and native presentation. Full returned pseudocode/evidence is retained
in actual-simulation-boundaries-rea-20261116.json. Filtered console summaries
were navigation aids, not complete native coverage claims.

The adapter reports SIMULATION_BEGIN immediately before the original verified
PlayerControls callback after exact source bind, on either the selected worker
or main path. It reports SIMULATION_END immediately before the guarded C346AC
Swap invocation only for the corresponding consumed/claimed source with an
observed begin. This spans consumption-driven native update work into the real
update-to-render boundary, rather than fabricating end at core render submit.
Existing callback/owner/queue/generation and lifecycle guards remain. Fallback,
menu and unmatched paths do not acquire synthetic simulation timestamps.

The core already stored derived bookkeeping for BIND. Actual replacement now
updates that bookkeeping and forwards the observed callback outside the ledger
lock, so only the actual timestamp can authorize a vendor marker. A regression
asserts derived begin/end are refused by marker admission, then observed
replacements are delivered once and admitted. Native reservation, receipt and
latency event contracts pass; Runtime and adapter built. No deployment or live
latency acceptance is claimed yet. PCL notification association remains separate.

## Actual-callback native trial

PID12004/report225425-009 refused prepare with ABI mismatch before hooks:
carrier still embedded game ABI23 while adapter/runtime used24. Controller
restored successfully. Added rsf_fallout4_trial build target for the coherent
carrier/adapter/runtime deployment set and invoked it before trial staging.

PID68740/report225643-400 prepared/started successfully with actual callbacks,
CaptureSRFailures=0 and no scripted capture. Gameplay status reported Reflex
owner1/mode1,966paced frames/5793marker calls, derived_boundaries0 and
order_violations0. After a bounded relative pan it reported2328paced frames,
13965marker calls and the same zero derived/order counts. These are actual
callback/SDK telemetry, not an end-to-end latency measurement.

Receipt telemetry independently recorded actual keyboard/mouse/message callbacks:
first gameplay snapshot9keyboard/169mouse/4563window messages/349ping candidates;
later60keyboard/1389mouse/11927messages/679ping candidates. Seven startup receipts
were rejected before a valid session; duplicates0. These do not claim per-frame
input attribution or SDK PCL ping delivery, which remains unfinished.

Runtime On/Boost modes applied. Off remained effectively On while the user had
DLSS-G enabled, as required by that provider. With FG explicitly disabled, the
separate Off check reportedFG0/effective_mode0 while paced_frames advanced28
across the bounded sample. Original settings were restored by byte comparison.
No capture-run performance numbers or vendor verification-tool acceptance are
claimed. PCL routing, XeLL/Anti-Lag hardware paths and scene/lifecycle coverage
remain open.

## Receipt routing implementation, not yet deployed

The generic D3D11 host registers its actual swapchain window/session using a
size/version-checked core call. Core routing validates that window still belongs
to this process and registered thread. An explicit receipt HWND must match;
a thread message with no HWND must arrive on that bound window's actual thread.
This thread-based association is counted separately from explicit-window matches.

Candidates are existing same-session/provider-generation owners with completed
pre-input pacing, actual simulation-begin timestamp no later than the receipt,
exact CPU stage mask7, no derived/substitute timing, no failure/end, and no
render submission started. Exactly one candidate is required. The original
receipt remains source0; it never modifies sampled input, frame masks or timing.
No new token, sleep, BIND or simulation boundary is generated. Streamline receives
only the real receipt through a generation-checked provider-lifetime call; its
own token lookup and coalescing remain authoritative. Missing/retired tokens
refuse rather than reacquire. Ordinary non-PCL messages remain observation-only.

input-routes exposes explicit-window/thread matches, unmatched receipts, successful
route calls, failures and last receipt/source. Successful route calls are not
unique SDK marker counts because provider coalescing remains once per token.
Pure selection tests cover ambiguity, source phase/timestamp, session and provider
generation. Runtime build passed after correcting missing result-type include
and using the actual swapchain description rather than adapter metadata.
Live SDK notification delivery and UI telemetry integration remain open.

## XeSS-FG switch rejected and shared sleep lock correction

The user reports that switching to XeSS-FG hung the game in trial
`native-raster-20261116-230933-130` (PID33372). Transient active XeSS status
and successful XeLL calls do not establish switch acceptance. The process had
already exited when investigated, so no hang dump was obtained.

Source inspection found the shared XeSS provider holding latency_guard across
xellSleep while older-frame markers and FG frame mapping also needed that lock.
The provider now serializes sleep separately and releases latency_guard during
sleep, publishing the new mapping only after success. The bounded actual-provider
SDK-double regression passed; this demonstrates lock progress, not native SDK
or game acceptance. Its timeout path now also releases the test lock before join.
The real switch remains unverified after this correction. Autocapture stays off.

The recurring hazard and required prevention checks are now recorded in
`.agents/skills/game-render-analysis/references/latency-sleep-locks.md`, linked
from the tracked rendering skill and required by the linked Reflex/PCL skill.
The repository-owned copy survives replacement of the ignored SDK checkout.

The next live trial `native-raster-20261116-232552-360` (PID16392) disproved
that the lock split alone fixed the hang. Two status reads stopped at source1799,
application_presents1801, XeLL paced_frames129, despite stale FG active1. The
process exited before MiniDumpWriteDump completed; hang.dmp is empty and is not
evidence. The controller restored the staged files. User also rejected the
waiting-for-SDK display and reported Off alongside XeLL pacing.

A second lock path was corrected: configure no longer blocks older-frame rendering
on a sleep in progress. Unchanged effective sleep parameters skip xellSetSleepMode;
changed parameters try-lock and defer before vendor mutations when busy. The
concurrency regression now covers this refusal and new-frame mapping invisibility.
Focused Release runtime build and regression passed. GPU completion before actual
sleep-mode changes still needs explicit ownership review.

Overlay text now reports effective On/Off and sleep-call count rather than using
that count as proof of active latency reduction. The forced-On explanation now
checks effective_reflex, not FG effective_mode, so suspension cannot hide it.
Intel's consumed guide requires XeLL enabled for XeSS-FG and calls in Off mode.
Rust overlay Release build passed; this is not a latency measurement.

## Confirmed Steam Present lock cycle and corrected dispatch

Trial `native-raster-20261116-232949-340` PID75624 reproduced the 128-tagged-frame
stall. The opt-in CPU watcher retained `stall-watch/threads.txt`, `stall.dmp` and
`report.json`. Main thread 0 waited inside libxess_fg Present, reached through
Steam's application Present handler. XeSS-FG Presenter Thread 1 (thread145)
waited in EnterCriticalSection from that same Steam handler. This explains why
queued FG counters temporarily said Active without continued physical output.

REA/IDA inspected the installed Steam overlay SHA256
21c0c1ddb4259c1156356bdfe8432dff504d52c7b96d19718688cff2d5a6e52f.
Present entry RVA9c130 acquires global critical section1834e8 at9c171, forwards
at9c24f, then releases at9c2f5. Present1 entry9bf50 uses the same critical section,
acquiring at9bf8f and releasing at9c10a. Existing RSF load-site relays dispatched
the facade while this lock remained held. Evidence is exported in
`.local/fallout4-research/xess-steam-present-lock-rea-20261116.json`.

The shared Steam integration now installs two additional expected-byte-guarded
MinHook entry hooks in the same cold queued installation/rollback set. An exact
registered RSF facade dispatches before entering Steam's critical section. Native
physical receivers call the original Steam handler through its trampoline,
retaining overlay pre/post work. No foreign lock is manually released; no overlay
is disabled. Original singleton and installer ownership remain unchanged.
Present entry bytes: `48 89 5c 24 08 48 89 6c 24 10 56 57 41 54 41 56`.
Present1 entry bytes: `40 53 55 56 57 41 54 41 55 41 56 41 57 48 83 ec`.
Focused `rsf_fallout4_trial` Release build passed.

Fixed trial `native-raster-20261116-233325-367` PID50264 passed the old stall:
Orca screenshot showed XeSS2x generating with414 sleep calls. A subsequent status
read observed FG Off at567 tagged frames and1093 output presents, followed by
XeSS active again at source2038,784 tagged frames,1602 output presents and28/56FPS.
XeLL paced_frames950; derived boundaries0, substitutes0 and order violations0.
These are live switch/progress receipts, not a claim of complete visual coverage
or measured latency reduction. Steam/other adapter paths still require their own
acceptance; no unrelated fixtures were run. GPU autocapture stayed disabled.

## XeSS performance correction and user acceptance

User reported XeSS-FG base rate23FPS versus over70 with DLSS/FSR. Inspection
found physical Present forwarded the native multi-refresh VSync interval without
normalization. Intel's consumed XeLL guide requires interval1 when VSync is on;
DLSS already warned that it clamps interval3. This was a pacing-contract defect,
not evidence of an incorrect hardware compute path.

The generic presentation bridge now normalizes nonzero intervals above1 only
when the prepared provider has FG enabled or XeLL low latency enabled. VSync Off
remains0; disabled FG/latency retains the application's interval. Normalization is
at the physical Present boundary after provider preparation, and its requested
and effective values are logged. The native application event remains unchanged.
Release runtime build passed after correcting the provider status member name.

Trial `native-raster-20261116-233650-937`, PID15252, logged requested3/effective1
for backend5. User said "Much better now." The immediate retained live status
reported XeSS SR854x534 to2560x1600, XeSS FG active2x,82 rendered/164 presented FPS,
source2182,541 tagged frames,956 output presents, and SR/FG/vendor results0.
This is a live sample and user acceptance of the improvement, not a controlled
cross-provider benchmark or measured input latency. The game was still running
when recorded. Autocapture remained off.

Loaded DLLs were the staged xess/libxess.dll2.0.2.68,
libxess_fg.dll1.3.1.78 and libxell.dll1.3.2.10. Source inspection confirms the
D3D12 graphics device is obtained from the host on the application's owning
adapter, or explicitly created using that adapter; XeSS receives this shared
device. No separate XeSS adapter choice exists. This source review does not
substitute for a captured adapter-LUID or GPU shader execution measurement.

## Sleep-mode settings completion guard

Follow-up source review closed the previously recorded missing queue-completion
check before xellSetSleepMode. XegSession now retains the supplied D3D12 queue and
lazily owns a fence/event. Only an actual change of effective sleep parameters
signals and waits for that queue with the shared bounded fence helper before
changing XeLL settings. A busy next-frame sleep still causes immediate deferral
before any vendor mutation. Failure to signal/complete refuses the change.
The normal per-frame path does not drain. Event ownership ends with the provider;
SDK contexts remain destroyed in XeFG-before-XeLL order by the existing owner.

Release runtime build and actual-provider CPU concurrency regression passed.
This follow-up is built but not deployed into the still-running PID15252; it
must receive a settings-switch game check before being called game-tested.
The queue receipt covers submitted work on the supplied provider queue; it is
not independent instrumentation of the SDK's private internal queues.

The engine tracker now retains the older rejected SR/FG trial as history and
points its latest tested SR/FG entry to the accepted moving-scene trial222102.

Watcher correction: the continued-stall-watch receipt in trial233650 is a false-positive while foreground0. Its main thread is in native SleepEx rather than vendor Present; it does not contradict the accepted interval correction. The watcher now resets its stall timer while unfocused. The retained 82/164 status is a last active sample, not ongoing background performance.

Shared-dependency check: Skyrim SE also consumes rsf_steam_d3d11_chain. Its Release adapter build passed with the pre-lock dispatch correction; no Skyrim deployment or game fixture was performed. The XeLL concurrency regression additionally checks that changing only FG frame count with unchanged sleep parameters progresses during sleep without calling xellSetSleepMode. That focused contract passed.

Follow-up observation: PID15252 remained alive/responding and continued native Presents after entering the pause menu. Orca screenshot472414c1 shows the native pause menu behind the RSF overlay, with FG waiting for a matching scene/window. Source0 and temporal-not-ready spatial recovery in that sample do not establish a new gameplay dropout. Windows refused foreground restoration with window_not_focused; no key was delivered. The settings-guard deployment check is waiting for user focus, rather than a crashed or blocked provider.

Focus later became available. PID15252 resumed with DLSS SR Ultra and DLSS-G active2x,78/156 FPS at source23053/application Present23054, SDK results0. This supersedes the focus blocker for that observation. Orca's subsequent key/click calls were delivered but did not achieve the intended menu actions; screenshots changed overlay page instead. No restart or guard deployment is claimed. Agent requested an idle-control interval before further input.

User acceptance of deployed trial233650: Everything seems to work perfectly fine now. This accepts the current running SR/FG path with the Steam Present-lock and physical interval corrections. The subsequent settings-completion guard is built/CPU-tested only and is excluded from this acceptance. No further menu restart was performed before this feedback. Autocapture remains disabled.

## PCL overlay telemetry implementation

Overlay ABI18 appends registered-PCL availability, actual keyboard/mouse/controller
receipt counts, and successful/failed/unmatched notification routing counts.
The shared FG stats adapter queries the existing core telemetry APIs; no adapter
policy, input synthesis or frame reassignment was added. C/Rust packet size is416
bytes, with layout checks and version rejection retained. The Frames panel labels
these as receipt/routing counts and explicitly explains SDK per-frame coalescing.
It does not equate a routed callback with a unique SDK ping or measured latency.

Native Release runtime and Rust Release overlay builds passed. All56 overlay tests
passed, including rendered-text assertions for the counter labels and coalescing
explanation. Deployment and live overlay observation are pending because the game
currently has the preceding build loaded. Earlier PCL SDK delivery evidence remains
valid for routing, not for this new UI. No verification executable was found in the
local vendor/reference inventory or standard NVIDIA installation directories searched;
this is a bounded search, not proof that NVIDIA offers no verification tooling.
Vendor-tool acceptance, controller and lifecycle checks remain open.

## NVIDIA verification tooling located; FrameView dependency confirmed

Official NVIDIA Reflex repository was fetched outside tracked source at
references/nvidia-reflex, pinned to c2522cee75be7a952ab29281c4cf6d978c5e2e0e.
Its Reflex_Verification directory supplies ReflexTestEnable.exe, ReflexTest.exe,
PrintPCL.exe, FVSDKSetup.exe and PCL ETW capture/cleanup scripts. This corrects
the earlier bounded-search result: the tools are available from upstream.
Authenticode signatures for PrintPCL.exe and ReflexTestEnable.exe are valid NVIDIA
Corporation signatures. No verification setup/cleanup batch or installer was run.

The consumed Streamline Reflex guide's verification steps require installing
FrameView SDK and restarting the system before ReflexTestSetup.bat. No FrameView
service or standard installation directory was found. Running PrintPCL.exe
--help returned ERROR: FvSDK_CreateSession -1, exit1. Therefore no vendor-tool
acceptance was obtained. This failure is retained as an environment dependency,
not attributed to the game's markers. Installed Windows Performance Toolkit
xperf.exe is available; NVIDIA's supplied ETW script names PCL provider GUID
0d216f06-82a6-4d49-bc4f-8f38ae56efab. No ETW collection session was started.
