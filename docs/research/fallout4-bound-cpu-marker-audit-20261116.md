# Bound CPU, marker and capture audit

## Question and evidence

Can the first active native-only FG trial identify a remaining generic marker
admission or closure defect, and does its anomalous output FPS prove bad pacing?
This audit reads durable copies, not the live process. It uses report
`native-raster-20261116-192707-484`, PID56312, CPU intention commit `e495934`,
Entry SHA256 `f7b95e792207530bc11e5ddab53165514af9e4489057cde3911f3c570f3318af`
and Runtime SHA256
`22830370ac079440177912e1b77680b37d55b7b1cd43915b6e2a68a4f70ce772`.
The ignored `cpu-marker-audit-192707-20261116.json` retains hashes of the copied
logs/status, current reviewed source and two partial capture manifests, plus
bounded counts and source/line anchors. Current source review is not a new
runtime test or proof that another worker's pending changes were deployed.

## What the chronology establishes

All three loader copies contain the same78 host CPU acknowledgements:13
source0 INPUT_EVENT receipts,33 FRAME_BEGIN,31 accepted FRAME_END and one
rejected FRAME_END. The rejected row is line238, source6826, stage4, flags0.
The source0 receipts have WINDOW_MESSAGE|INPUT_UNFRAMED flags0x14; acceptance
does not claim their attribution to a future source or a vendor input marker.
No host acknowledgement of stages7 through10 survives. Adapter messages saying
those facts were emitted are not core acceptance acknowledgements.

The host budgets in `d3d11_host.cpp:110` reserve24 startup rows,53 sampled rows
and independent first-stage refusals. Early loading consumes the sample budget;
the last ordinary sampled fact is source3456. The absence of later successful
BIND rows is therefore not evidence that BIND failed. No `FG/Reflex` per-marker
debug rows exist in these copies; `native_fg.cpp:52` requires the existing debug
setting for those rows, including rate-limited errors.

The pre-FG copy has17 order-violation deltas. The runtime switch to effective1,
generation2 appears at line553 in the longer copies;18 deltas precede it.
The first FG copy sums58 and the later copy117. These are cumulative snapshots,
not independent runs or counters aligned with the status query that reports57.
The longer copy includes16 bounded downstream refusal rows:9 consumer-return
with reached/passed0x3/0x1 and7 bind-return with0x803/0x801. Both identify the
native-cycle proof bit, not its internal subpredicate. No mode, loading or
controller-state explanation follows from those masks.

Public status independently shows source13077, generation2, known real input
timestamp, paced1, failed0, completed1 and tagged2538. The later retained status
shows source25659 with the same successful owner state, tagged14027 and
latency-paced18063. These establish actual bound-owner/FG admission at those
snapshots. They do not establish every intervening source, scanout, moving
quality or input-notification/PCL coverage.

## Order counter interpretation and remaining evidence

The label `marker_order_violations` combines two distinct paths:

- `native_cpu.cpp:401` increments it for rejected CPU stage/timestamp ordering.
  An unflagged FRAME_END at partial mask7 is deliberately refused; explicit
  skipped mask7 closes without becoming complete. A previously failed frame
  can also reject a later end without incrementing this counter again.
- `native_cpu.cpp:494` increments it for a marker attempt missing predecessor
  marker bits. It does not mean the vendor accepted an out-of-order marker.

Consequently the source6826 end rejection cannot identify the failed predicate
or be assigned to a vendor marker. Its before-mask, timestamps and reservation
state were not copied. The current generic fallback guard remains present:
`native_fg.cpp:975` requires input/sample facts and simulation-start marker
before inference; prospective submission additionally requires a bound poll and
verified post-update primary association. Submit end checks accepted start and
mask dependencies at line904; Present start checks accepted submit end and
mask15 at line1022. Late FRAME_END remains keyed to its own slot, while
`native_window.cpp:37` reads the render thread's WINDOW lease, not the newest
worker FRAME_BEGIN. No concrete new generic admission defect is established by
these receipts. Preserve the refusals rather than interpreting them as missing
native SIM markers or relaxing them for this game.

The discriminating missing evidence is one failing source's pre-dispatch
stage/marker masks, failed/ended state, incoming stage/flags/QPC and preceding
QPC, reservation state, plus the caller/result if the failure was a marker
attempt. Existing aggregate counters and exhausted acknowledgements cannot
reconstruct that sequence. The native-cycle internal predicate is a separate
adapter evidence gap, not a proposed generic policy change.

## FPS anomaly

`fg-status.txt` reports base80/output10106 with output_fps_valid1. Valid
statistics21 means total-presented, latency-available and activity are available;
it does not validate generated-presented or callback counters. Their zero values
are not measured zero generated output.

`overlay_api.cpp:282` computes output FPS from the change in vendor total over
at least500ms. Its baseline resets on backend change or counter regression,
not on every statistics-validity/provider-generation transition; there is no
plausibility bound. `dlss_generation.cpp:216` accumulates
numFramesActuallyPresented. The matched SDK header `sl_dlss_g.h:156` defines
that field as frames since the last GetState call, so this accumulation is
consistent with the declared contract. The programming guide's per-application
example explicitly assumes querying after each Present. An initial hypothesis
that every status poll necessarily double-counts a per-frame multiplier was
rejected after reading that header and the guide's preceding caveat.

The anomalous sample could reflect a counter/baseline discontinuity, but there
is no copied interval baseline, generation/validity transition or raw SDK delta
to establish which. Neither10106 nor the root-observed81/162 overlay proves
scanout or latency. No pacing change follows from this statistic.

## Two partial FG-On captures

Retained captures `scene-20261116-193130-167-115805843-56312-1` and
`scene-20261116-193313-148-115908828-56312-2` name sources15309/native15502 and
23503/native23447. Both have SR0, present_result-3,25 of40 images captured,
unavailable final_color and partial status. Their matched_present=true is not
independent proof of an actual matching Present: `input_dump.cpp:477` sets
presented=true even when complete is called with NOT_READY.

The current host explicitly closes a pending capture with NOT_READY at
`d3d11_host.cpp:278` when WINDOW match, session/source/native/view identity or
scene-to-Present path fails. That check runs before final_color acquisition.
This explains the observed metadata combination as a supported failure path,
but the manifests do not identify which disjunct fired. The current WINDOW
matcher is thread-local and does not consult worker CPU frame identity, so a
late successor FRAME_BEGIN alone is not a demonstrated cause. Exact failing
WINDOW/capture tuple and scene path at that callback are missing. Capture
completion/metadata is a separate host ownership issue for its current owner;
these partial bundles cannot establish complete FG output or generated scanout.

No production files, tests, settings, processes or devices were changed or used.

## Final authoritative log correction

The earlier sections describe the bounded copies available during the first
audit, not the final run. Root subsequently provided the final report's
`loader.log`:758715bytes,7684lines, SHA256
`e587d0426833686d7b6c757b497302e1b3595307f20bffdff881b966299ca8ba`.
The ignored JSON now retains this additional receipt separately. Its counts
supersede any inference of steady operation from isolated successful status:

- 1244 effective-state rows:622 mode1/active1 and622 mode0/active0, strictly
  alternating with1243 changes between consecutive logged rows.
- 852 Streamline out-of-order skipped-present warnings,188 sync-mutex skips
  and82 async-mutex skips.
- 243 summed latency-window order-violation deltas. This still combines CPU
  stage and marker-attempt refusals, rather than proving243 vendor marker errors.
- The final host SR row at line7597 reports21447 evaluated and241 refused,
  native29173. Emitted SR result groups are233 success,230 result-10 and2
  result-3. These change/budget-limited log rows are not per-frame rates and
  must not replace the actual host counters.

The user reported FG flicker and a defective split image. Visual validation is
rejected. Neither the prior81/162 observation nor a successful bound status
closes this failure. No retained source-matched screenshot establishes which
transition or resource produced the split image.

### Located transition and limits

Final lines655 through683 provide a concrete local sequence:

1. Source10298/native10656 is eligible with CPU mask15, submitted1, paced1,
   owner_failed0, generation2 and valid1. Line663 reports mode1/active1.
2. Lines666/667 report native SR NOT_READY(-8), mapped to host result-10 on
   native10657. Line668 names source10299 with mask7, submitted0, paced0,
   owner_failed1, owner_generation0 and owner_ready0. The real poll/reservation
   remains known, but cpu_valid0, submission_ok0 and valid0 prevent generation.
3. Lines670 through676 contain async mutex and out-of-order skipped presents;
   line678 reports mode0/active0. Source10300 remains ineligible at line679.
4. Source10301/native10659 is eligible again at line681, with mask15 and valid
   submitted/paced ownership; line683 reports mode1/active1.

Later out-of-order warnings at line7204 and a sync-mutex skip at line7212 show
that the warnings are not confined to activation. This chronology establishes
adjacency and the recorded local eligibility reasons, not whether option
changes caused the SDK warnings or vice versa. It cannot identify the internal
native-cycle subpredicate or explain all622 Off transitions.

### Current generic transition mechanics

`native_fg.cpp:1389` forms valid from matching WINDOW/scene, prepared inputs,
HUDless, completed submission, identity, supported screen, continuity, VSync
and CPU ownership. At line1401 it configures the requested mode for valid
frames and Off otherwise. Thus source10299's logged missing submission and
failed/unpaced owner already suffice to select Off without inventing a missing
native simulation event. A prepared-input failure can also configure Off at
line1506; the bounded receipts do not distinguish every occurrence.

The DLSS provider at `dlss_generation.cpp:334` suppresses unchanged option
calls. Reflex-only changes preserve generation history at line349. A real
generation mode/count change calls SetOptions, then updates configured and
effective mode, clears active and invalidates provider history at lines358
through362. The Off/On policy can therefore create actual SDK configuration
transitions when eligibility changes, rather than merely toggling overlay text.
The next shared SR evaluation also resets its translated history when the
cached generating state changes (`native_fg.cpp:1262`). This is the existing
generic mechanism, not permission to keep generation enabled for a failed
owner or to bypass any admission guard.

At retirement, `native_fg.cpp:1523` logs effective changes from provider status.
The provider computes active from prepared_enabled, successful SDK status and
numFramesActuallyPresented>1 (`dlss_generation.cpp:241`). That activity test
depends on the SDK query interval; however every effective row here changes
mode as well as active. Activity sampling alone cannot explain the recorded
mode0/mode1 alternation. The log does not contain a complete per-Present option
and eligibility trace, so it cannot prove how many transitions were caused by
each failed prerequisite. Prepare receipts are limited to the first8 calls,
then source multiples of128, capped at64 rows per provider generation
(`native_fg.cpp:510`).

### SR refusal chain

These result spaces must remain separate:

| Receipt | Meaning on this path |
| --- | --- |
| Native DLSS result-8 | RSF_DLSS_ERROR_NOT_READY returned before successful SR evaluation |
| Pipeline/host result-10 | EVALUATE_FAILED mapping of that native refusal |
| Host result-3 | RSF_ERROR_NOT_READY, including preparation-only or other host readiness refusal |

`dlss_pipeline.cpp:857` calls the shared native FG/SR bridge; a nonzero result
is logged as the native result and mapped to EVALUATE_FAILED at lines879
through895. `native_fg.cpp:1248` can return native-8 for absent scene identity,
camera-motion inclusion or the exact source token; line1250 can refuse shared
transfer initialization. Deeper shared evaluation also refuses absent shared
host/id, initialization/device/options state or the exact token
(`dlss_streamline.cpp:441`,450,586). The source10299 owner failure is compatible
with unavailable token ownership, but the log does not record the token lookup
or the precise returning branch. Calling this a proven token defect would
overstate the evidence. Nor is native-8 proof that the vendor executed SR and
rejected image quality.

The remaining discriminating evidence is the exact failed owner-acquisition or
native-cycle predicate, the returning native-8 branch, and each affected
Present's eligibility/configuration/prepare outcome and provider state before
and after it. Preserve all existing refusal and ordering checks. This update
changes only research and ignored analytic data; no new source, build, process,
device or visual experiment was performed.
