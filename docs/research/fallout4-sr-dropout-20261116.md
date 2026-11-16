# Native SR dropout, 16 November 2026

The native-only trial at commit `4b2fa3643617f5db996dc270c8381b50718b9d04`
failed visual validation. The user reports repeated SR dropout and jitter.
The retained log confirms repeated failed temporal evaluations and switches
between temporal SR and spatial reduced-frame recovery. Spatial recovery
preserves a defined output after reduction; it does not establish stable SR.

## Trial and retained evidence

The installed executable is Fallout4.exe 1.11.240.0, SHA256
`fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`.
The controller directly launched PID47776 at 19:26:32.3617053 UTC. Its native
module checks excluded F4SE, non-system D3D11 and launchers. The deployed Runtime
SHA256 was `e2c4958ada60bdc04ada49a4ab1b9f268fc813f436b1ed420949a3154b9309d9`;
Entry was `9b5c79df0b2dd2e5036b5df1cc71bbcff8773b1a124030b8746b40849f8d4e1f`.
These are the previously verified provider-concurrency/recovery binaries,
not the later unbuilt FSR/XeSS changes.

The authoritative ignored report is
`.local/fallout4-research/native-raster-20261116-202630-191`.
Its final loader log contains 241242 bytes and 2624 lines, SHA256
`9efbca81c28447cdcc938510e920b989a94ad0518ed07131f52500cc86a3cab0`.
`sr-dropout-final-analysis.json` retains the extracted rows and limits.
Initial observation showed Balanced selected; root later requested Quality1
with FG and latency Off. Settings roundtrip succeeded, but the immediate status
was spatial recovery with temporal result -3 and no attempted evaluation. This
request is not proof of provider execution.

## What the log proves

The last SR counters are 3824 evaluated and 778 refused at native frame7562.
There are 668 logged result -10 transitions followed by result0; 575 return
success on the immediately following native frame. For example, frames7534,
7551 and7561 fail and frames7535,7552 and7562 succeed. Result logging is
transition-based, so these counts are not the proportion of all rendered
frames that failed. The counters include startup and settings changes.

The bounded recovery rows contain 21 temporal dispositions3 and 11 spatial
dispositions4. They distinguish actual temporal result0 from attempted result
-10 and unattempted result -3. The spatial/temporal alternation agrees with the
reported symptom; a same-source moving capture is still needed to establish
the visual consequences of each transition.

The first recorded native failure is source1574's predecessor-Present
dependency timeout, detail8 at the existing 20ms deadline. The same source
subsequently fails the exact shared Streamline token lookup (`sr-token`,
native -8), mapped by the host to SR -10. This identifies that source's failed
boundary, not a vendor image-quality rejection or every later failure.

Five distinct worker callback phases also reject native-cycle term14:
frame-return, reserve-return, bind-return, sampled and consumer-begin. Each
records native mode0 with copied joining0 after the expected worker, vtable,
destructor, Run and event checks. Later native frame/window/running/exit reads
were short-circuited and remain unknown. They must not be inferred from the
incoming proof values printed in those rows.

## Source and native ordering defect

The exact native BD51C0 join checks mode1, writes mode0 at BD51D2, then waits
for worker completion event+60. The worker signals that event after the
selected JobListEnd callback returns. Mode0 is therefore valid while that
callback runs inside a selected Main join, but also occurs in unrelated paths.

The adapter publishes an exact-cycle joining flag before calling the native
join. Worker callbacks copy that flag before reading native state. Main can
publish joining and clear native mode after the copy but before the worker's
mode read, leaving the copied false value stale. The ignored
`cpu-join-snapshot-race-audit-20261116.json` records the native instruction
proof, source anchors and possible interleaving. This is a concrete source
race matching the refused comparison. The log does not sample the concurrent
join lease and does not prove it explains every dropout.

A correction must revalidate only the same predecessor's selected active join
and then the complete native cycle, preserving raw/poll/cursor, owner, frame,
lifecycle and final metadata checks. Mode0 alone must remain insufficient.
No unconditional mode admission, extra sleep or fake CPU event is justified.

## Exit and limits

The process exited with code0 before the controller's world handoff completed.
Root sent no close action; the exit method is unverified. The controller
reported live-window absence, observation_completed false and restored true.
An independent check verified all21 destination hashes or original absences,
including ENB restoration, with no game/launcher process remaining. The custom
INI hash remained unchanged. This is restoration evidence, not a normal-Quit
test result.

Root armed capture successfully while the pause menu was visible, but no
complete matched final capture was retained before exit. No relative-mouse
input was sent. Moving vectors, shader/buffer coverage, native TAA history,
stable FG/latency, all-quality visual acceptance and later compatibility remain
open. The earlier PID56312 rejected flicker evidence remains separate.

## Join snapshot correction

The worker now retries a failed mode comparison only when the selected worker
callback still matches the future source and exact predecessor, and the current
cycle has a selected active join, is swapped and closed, and is not invalid.
The lease is read under worker_guard. The lock is released before repeating the
complete native proof; native completion cannot signal until this callback
returns. The retry permits mode0 through that lease, with all remaining native
checks and caller input/metadata checks intact. Other failures do not retry.
No deadline, vendor policy, or input event semantics changed.

This addresses the source race identified above. It does not establish that the
20ms predecessor timeout or every SR refusal has the same cause. Native moving
acceptance remains pending; no game files were changed by verification.

The focused worker-handoff regression forces the stale-false snapshot before
Main publishes its join. It accepts the exact join and rejects a changed native
frame/source, different worker, expired join and invalid cycle. Release adapter
and fixture build succeeded; fallout4_worker_handoff_contract passed. This is
synthetic ordering coverage, not native visual acceptance.

Further source tracing finds pace_cpu_frame aborts the acquired shared token
when the predecessor Present is still pending after 20ms. The later sr-token
failure for source1574 follows that abort. Changing that deadline or resurrecting
the token is not yet justified: pacing, CPU admission and token retirement must
remain coherent. The runtime path has not been changed in this correction.

## Rejected join-fix trial and deadline review

Trial `.local/fallout4-research/native-raster-20261116-204917-056`, PID18964,
loaded the join retry. The user rejected Quality1 with the same symptom.
The retained user-rejected-quality1.log includes renewed consumer-begin term14
and spatial/temporal alternation. Native-size observation earlier reached2250
successful evaluations/16 refusals; these are not matched-scene performance
comparisons. The Quality1 capture is partial, SR0/matched Present-3, and does
not prove a captured failed SR evaluation. CloseMainWindow was sent by the
agent; exit was C0000005. Controller result reports restored=true and no forced
kill. The direct world-close fault remains open.

NVIDIA's linked SR, FG and Reflex/PCL checklists are now readable after repairing
four junctions whose former .local/streamline/sdk target was absent. Their source
is the existing reference checkout's Streamline/plugins/streamline-agent-skills.
The checklists require explicit frame-token lifetime, ordered sleep, and safe
shutdown; they do not prescribe a20ms deadline or endorse an unbounded wait.

A source-only proposal removed the20ms deadline and built the Runtime. It was
never deployed. Review found drain_future refuses retirement while future.busy
is true, including during quiesce. A callback waiting indefinitely cannot rely
on that deferred abort as its cancellation signal. The proposal was removed
before another run. The next repair must address cancellation delivery and
predecessor progress together, retaining one token and one sleep per source.

## Cancellation delivery repair

`drain_future` now permits cancellation notification during the synchronous
RESERVE callback while retaining the busy ticket and deferring FRAME_END and
record removal until actual worker return. Other busy worker phases retain
the existing retirement restriction. `withdraw_reserved_sources` marks the
owner failed but defers token abort while pacing is in flight; `pace_cpu_frame`
retires a withdrawn token after its callback returns and does not publish a
successful paced state for that withdrawn source. The20ms deadline remains
unchanged pending the remaining progress/lifetime audit.

Release Runtime/adapter and focused fixtures built. The new
native_cpu_reservation_contract scenario holds a RESERVE callback on a worker,
dispatches QUIESCE abort from Main, and verifies the callback observes ABORTED
before returning. Both that test and fallout4_worker_handoff_contract pass.
This proves ledger notification concurrency, not vendor sleep/token lifetime
under a GPU workload or native game acceptance. No deployment was performed.

## Cancellable predecessor wait trial

After the cancellation repair, the core now waits for the predecessor result
or reservation withdrawal instead of aborting an Open reservation at20ms.
Capacity cleanup and reservation-slot reuse exclude active pacing callbacks.
Acquisition completion preserves a cancellation that arrived concurrently,
instead of overwriting frame.failed with acquisition success. Release build
and the two focused contracts passed. These checks do not establish GPU token
lifetime or absence of native wait cycles; the new native trial is required.

Trial `.local/fallout4-research/native-raster-20261116-210310-776`, PID62468,
was launched with the backed-up native-only controller. Outcome pending.

PID62468 outcome: the user reports improved rendering, then a hang switching
DLSS to another backend. Windows reported not responding. `hang.dmp` retains
thread contexts and stack memory. `hang-thread-candidates.json` is a raw pointer
scan, not an unwound call stack; candidate addresses do not prove call chains.
The process exited before the agent attempted termination; no kill was sent.
Controller exit0/restored=true is recorded, but the termination method is unknown.
The log ends with native1758 state-change diagnostics and the agent's Quality1
settings application. It does not record a completed user backend switch.
Backend-switch progress and predecessor cancellation are the next investigation;
this trial is not full SR acceptance or FG validation.

## Unpublished predecessor Present cancellation

REA/IDA opened the exact Runtime SHA25695e282db3ba425e228b49649a9839b5c31ad5a6457b1dc53b0867ac8069d974d
and resolved candidate RVA1659F to function16410. Its decompilation identifies
the reservation-read/WaitForSingleObject loop in pace_cpu_frame. The exported
runtime-wait-rea.json retains actual tool evidence. The thread scan is still
not a stack unwind, so it identifies a plausible wait site, not a complete
proven deadlock chain.

Source review finds close_frame can close a constructed source without a
sealed Present phase, then Main reaches its native worker join. A reserved
successor can otherwise wait for a Present outcome that was never published.
close_frame now refuses only the future whose predecessor exactly matches that
closing unsealed source, then delivers its abort through the repaired busy
RESERVE cancellation path before Main joins. It does not report a fabricated
Present, admit invalid camera inputs, or change vendor policy in the adapter.
Release adapter build and existing worker/reservation contracts passed. These
tests do not exercise the complete native switch path; game retest is pending.

## Switch cancellation retest PID65288

Trial native-raster-20261116-210904-445: the user confirms backend switching
works again, but rejects all SR scales below DLAA as dropping out. The last
logged SR transition is result0/evaluated1360/refused0 at native3571. No host
result-10 transition appears. The32 bounded recovery rows contain one initial
unattempted-3 spatial recovery followed by31 successful temporal recoveries.
This differs from the earlier repeated token/evaluation refusal chain. It does
not prove every frame reached SR or establish image quality. Requested/effective
backend and quality changes are recorded from the panel; the user acceptance
is limited to switching. A requested capture did not produce a new complete
artifact before exit. Controller exit0/restored=true; method of exit unverified.

The remaining reduced-only defect needs matching source/buffer/history and
post-upscale consumer evidence. Successful SDK evaluation must not be treated
as proof of complete producer coverage or correct native TAA/history ownership.

## Reduced-only image follow-up

Reviewed observe_effects/emit_sr, finish_recovery, observe_taa and the camera
raster/jitter conversion. native_raster_snapshot_current checks source/resource
ownership, not equality to the old viewport; changing to output resolution does
not by itself explain failed TAA exclusion. The old source12559/native12932
capture contains a populated1707x1067 scene input, but no complete final output.
It cannot prove the user's latest failing frame or downstream correctness.

Added at most32 TAA-gate receipts, one for each observed combination of original
native result, imagespace scope, defined full output, temporal publication and
owner-qualified exclusion. The receipt preserves existing short-circuit owner
reads and return behavior; it adds no setting or rendering policy. It records
source/native frame and recovery token to distinguish native TAA running after
SR from other input/history/consumer faults. Release adapter build passed;
undeployed, no new visual acceptance.

## TAA diagnostic trial PID50468, ongoing

The user rejects the same reduced-resolution issue in report
native-raster-20261116-211410-112. Native1734/source1306 has a complete capture
with matched Present0 and SR0, but frame.json shows2560x1600 input/output:
this is a native-size frame before settled Quality, not reduced acceptance.
Its TAA receipt has original1/full_output1/temporal1/owner_current1 and
native_taa_runs0. That observed frame excludes double TAA only for itself.
A later source1314 receipt has spatial full output and native TAA also excluded.
A second capture was armed after1707x1067 evaluation was logged; it was still
pending at the latest observation. The process was live/responding with the
shared overlay open. No final reduced-frame capture or acceptance follows.

PID50468 capture3 (scene-20261116-212001-333-122317015-50468-3) is now complete:
source7833/native8145,1707x1067 input to2560x1600 output, SR0, matched Present0.
Camera data is present and frame time18ms. Backend output and final colour were
visually inspected: both are populated to full extent without an obvious
partial rectangle in this static sample. Their brightness differs across native
postprocessing and preview encoding; raw numeric equivalence is not asserted.
The user still rejects reduced temporal behavior. sr_constants.json records
reset1 and previous jitter0; engine_pass flags23 also includes RESET. This
single capture after focus transitions cannot establish repeated resets. Adjacent
settled evaluations and the exact history-reset reason are required next.
Capture2 was spatial recovery with camera_available=false, matched Present-3;
its sr_result0 alone was not a temporal evaluation. Preserve it as partial.

## Reset provenance instrumentation

The adapter now logs up to32 distinct camera reset reason masks from already
copied values: bit0 no history,1 invalid reprojection/raster history,2 camera
owner change,3 native frame gap,4 extent change,5 raster change,6 previous VP
mismatch,7 previous origin mismatch. Zero is also observable. The reset
predicate remains the same OR of those conditions; no history guard was relaxed.
Producer snapshot invalidation now has its own reader receipt instead of sharing
state-changed with native-state reread. TAA receipts use their own bounded mask
rather than the unrelated16-message diagnostic budget. Release adapter and
fallout4_camera_contract passed. New diagnostics are not native acceptance.

## Confirmed timing-only refusal and repair

PID8348/report native-raster-20261116-212821-694 records native2739/2739,
mask0x200: delta_seconds alone changed from0.0180000011 to0.0340000018.
Owner, camera/cache, texture mappings, TAA state, viewport selectors, ratios
and viewport metadata remained equal. Subsequent accepted frames report the
native-frame-gap reset reason. This directly identifies a false render-input
identity failure caused by sampling the mutable timing global twice.

same_native_state now compares renderer/input identity while timing is retained
from the first validated snapshot and passed through to SR. Both reads still
validate finite nonnegative timing. No camera, texture, frame, viewport or ratio
identity guard was removed. Tests assert that timing updates preserve input
identity while every other checked field continues to refuse changes. Release
adapter/Runtime build and input-state/camera contracts passed; undeployed.

The shared D3D11 host now exposes saved CaptureSRFailures through its generic
overlay descriptors, default Off. It captures failure-time resources for the
same failing source, not a later successful source, with at most3 automatic
captures per host session. Existing active/manual captures are preserved and
planning-only calls do not trigger automatic capture. Spatial recovery retains
the negative temporal result in sr status rather than a misleading default0.
The bounded policy contract tests disabled/success/busy admission, failed
evaluation, unattempted recovery and cap preservation across toggle changes.
GPU capture-content validation remains pending; successful-but-wrong output
still needs sequence capture, and missing upstream adapter inputs are represented
only where a committed reduced-frame recovery reaches the host.

## Timing repair and automatic-capture trial PID74532

Report native-raster-20261116-214152-765 deployed Runtime51f4bf4470e95b0ff9a9395de42a803833b9097a127fdbfb8961c281a278facd
with the timing identity repair and automatic capture controls. Three automatic
failure bundles were retained for native1929,1933,1944 at1485x928; all classify
as spatial recovery with sr_result-3/camera unavailable/missing final image,
not successful temporal evaluations. The3/session cap held. No timing-reread
mismatch was logged. Native1933 now explicitly fails producer-changed: the
prepass depth lease validation, not the native-state/timing comparison.

The sequence harness retained ready1707x1067 status, then a WinError5 report as
the process exited before its sequence finished. Controller records exit0 and
restored=true; no root close/kill was sent and exit method is unknown. No new
visual acceptance or FG validation follows. Automatic failures are analyzed in
report/automatic-failures.json; raw bundles remain under captures/website.

Added bounded depth-lease refusal receipts using existing sequential checks:
verified1, valid2, identity4, pre-CPU8, sample16, equality32, post-CPU64. Change
bits after successful sample are mode/selectors1, intervals2, jitter4, ratios8,
near-inverse16, near-camera32, world-camera64, unjittered-camera128. Unknown
partial values are qualified by reached/passed bits. Original guards and short
circuit order remain. Adapter built; these new receipts are undeployed.

Harness correction: the first sequence implementation emitted a label rejected
by the result writer's sr/fg-only whitelist. The whitelist now explicitly admits
sequence_capture, automatic_sr_failure and spatial_recovery; only sr overwrites
the temporal result. A filesystem-only C++ test verifies actual output files and
negative temporal result retention. Previous mock-only sequence tests did not
cover the writer contract and were insufficient evidence for end-to-end operation.

The adapter additionally offers a recovery_native_output texture before native
Present under existing exact native frame/window/context/chain and raster-owner
checks. This uses rsf_d3d11_host_capture_texture's matching capture identity guard
and does not claim temporal readiness or a matched Present. This will preserve
native downstream pixels when failed temporal admission prevents WINDOW evidence.
Source and build checked; actual failed-frame capture verification remains open.

## Render lease versus input provenance, PID55388

Report native-raster-20261116-214918-669 supplies discriminating depth receipts.
At source968/native1398 reached0x7f/passed0x3f, sample and exact copied-depth
comparison pass, including unchanged jitter and ratios; the final CPU source
check alone fails. At source1038/native1468 reached0xf/passed0x7, the initial
CPU source check fails before any depth reread. These identify input-provenance
refusal, not a changed depth producer. The trial's sequence timed out with no
selected bundles; no image-quality acceptance is inferred.

The adapter now has render_source_is_current for its existing guarded render
association: enabled lifecycle, exact Swap nesting and identity, constructed
unclosed cycle, execution thread, native frame and Main owner. Camera, screen,
resource, depth and viewport readers retain their independent checks. Later
input-provenance invalidation no longer erases this render lease. Original
cpu_source_is_current and pre-input/adoption/Present-phase gates remain strict;
no input marker, sampled source or FG readiness is fabricated. Rendering consumers
use the render lease; core FG/latency still inspect their CPU facts.

Source review also found the SR pass retained UNKNOWN screen after a separate
copy had passed gameplay classification. It now receives the same verified
screen value. This repairs capture-sequence gameplay selection without changing
its guard. Release adapter builds and existing camera/input/worker contracts
passed; new native validation remains pending. Root requested CloseMainWindow
on the exact owned PID55388 after retaining pre-close.log; outcome is separate.

## Consecutive reduced SR and motion-sensitive FG, PID57728

Report native-raster-20261116-215357-561's automated sequence completed four
consecutive native1398..1401 captures at1707x1067 to2560x1600, all evaluated-
presented, all reset0. Final-image adjacent normalized RGB MSE was approximately
1.87e-6,1.87e-6,1.48e-5; this static sample is not moving-scene acceptance.
FG was requested through the runtime API, initially inactive while backgrounded.
After foreground gameplay, status reported effective DLSS, active1, valid output
statistics,90rendered/180presented FPS and source5481. A later source snapshot
reported1098 tagged frames, matched generation2, paced1/failed0/completed1.
The user reports FG suspends with camera movement; this is failed motion
acceptance despite static functional activation. The later FG sequence timed
out waiting for stable foreground SR and supplies no FG image acceptance.

The log's swap-claim refusal for source5336 has receiver cursor2845 unchanged
but queue tail2846. The failed term7 was tail==consumed-cursor, not a failed
native owner, worker join or consumed sample. After consumption, later enqueued
input is pending for a future consumer and is not evidence that this sample
changed. The adapter now compares the receiver's actual consumed cursor against
the ticket at swap/adopt, keeps exact poll-generation/native-cycle checks, and
removes queue-tail equality at joined completion. Before-bind queue/raw checks
remain strict; after-consumer proof still requires the ticket's consumed cursor.
No marker semantics or vendor policy changed. Release adapter and existing worker
handoff contract passed. This correction is not yet deployed or motion-validated.

## Computer Use and FG capture follow-up, PID27512

Report native-raster-20261116-220424-513 deployed the consumed-cursor correction.
Four initial captures at native763..766 are complete/reset0 but contain only
pre-SR FG depth/motion, not prepared FG metadata. A second sequence required
foreground active FG and also lacked fg_constants. Source review identifies
FG record.view_id was unset and its separate resource generation was compared
against the SR record by rsf_input_dump_wants. The corrected join verifies the
exact supplied native pass and matching record session/source/view while saving
FG's own generation. A no-device file-output regression proves generation99
joins and a view14 record cannot overwrite capture view13. Runtime/test built
and test passed. Current running PID27512 uses the prior Runtime; correction
is undeployed.

The explicitly invoked Computer Use skill was loaded with guidance/API and
confirmation rules. sky selected the returned Fallout4 window, activated it,
and performed an absolute drag and key action with fresh snapshots. The images
remained effectively stationary; those inputs do not establish native relative
camera movement. A user question asks whether foreground-checked relative input
in the harness may be used outside this skill's API-only restriction. No such
input has been sent, and moving FG remains unverified.

## Automated relative pan and second consumed-predecessor repair

The user explicitly authorized foreground-checked relative mouse input outside
the Computer Use API restriction. tools/pan-game-camera.py sent120relative
packets ofdx2 to PID27512. Before/after Computer Use images visibly changed the
camera orientation. pan-right.json contains32 status samples:6active and26
with frame inputs/submission unmatched. This fails motion acceptance.

The retained gate1 receipt identifies bit10 (binding_ready) missing. It still
required the consumed predecessor's old raw revision and queue tail to remain
unchanged when reserving the next poll. Those values can advance with pending
mouse input without changing its consumed cursor. Reservation entry now retains
same native state, consumed cursor and exact next generation; the callback's
predecessor protection similarly ignores later raw/tail updates while the new
native Poll has not begun. New-poll sampling/binding raw stability checks remain.
The worker regression now accepts later raw/tail arrivals for that consumed
predecessor and still rejects consumed-cursor advances, intervening polls,
wrong owners and withdrawal. Build and worker contract passed; undeployed.

The native controller reached its scene/quit handoff timeouts with PID27512
still live. The process later disappeared during Computer Use observation; no
agent kill was sent and exit method/code were not retained by the timed-out
controller. Restoration was resumed only after fresh empty process checks and
verified21entries through tools/restore-capture-trial.ps1. The separate receipt
resumed-restoration-20261116-221916-968.json preserves this recovery; the original
restored=false timeout result remains historical rather than being overwritten.
