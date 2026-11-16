# Fallout 4 native scale captures, 16 November 2026

The question is whether a generation-only future poll was incorrectly invalidating
an input-unbound render lease, preventing an actual SR frame from reaching capture
completion. This trial also checks standard SR scale changes against the retained
scene. It does not complete moving shader coverage, FG, latency or compatibility.

## Identity and changes

Installed Fallout4.exe1.11.240.0 SHA256
`fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`.
Coordinator commit `e83bb70d864873fd868cbdf6c5f563bb1d724130` ran as PID47344,
start2026-11-16T17:24:10.6134707Z, report
`.local/fallout4-research/native-raster-20261116-182408-464`.

- Runtime: `22830370ac079440177912e1b77680b37d55b7b1cd43915b6e2a68a4f70ce772`.
- Entry: `c5d02f9c350e9520d9564df50b6d06d847066ff93a7f486b55d56cfa67e1fb92`.
- Direct native launch. First/latest module receipts prove F4SE0,
  non-systemD3D110 and loader0. ENB was quarantined with its exact backed-up hash.

The adapter correction preserves an input-unbound current renderer only for a
qualified unmatched poll with unchanged native owner/state and no lifecycle,
raw-writer or overlapping-poll failure. Every future ticket still refuses;
bound predecessors retain strict invalidation. It does not manufacture input
binding, simulation, pacing or FG eligibility. The full Release VS2026 gate
passed224 tests,14 skipped,238total in110.40seconds; Rust fmt/Clippy were clean
and the extended production-helper handoff contract passed0.01seconds.

The earlier PID71716 actual Quality1 captures were partial with SR0 and
present_result-3. In this trial completed final-image captures are available.
This supports the render-ownership correction; it does not establish the cause
of the user's earlier duplicated image.

## Actual evaluations and matched Present

The retained ExitSave opens the stationary first-person shower. Public settings
were applied live, with FG/latency Off for the scale captures. All output sizes
are2560x1600. Capture directories remain under the game ReScaleFrame capture
root, outside Git; exact paths and artifact hashes are recorded in the report's
`post-close-disposition.json`.

| Quality | Render size | Source / native frame | Captured / listed | Result |
| --- | --- | --- | --- | --- |
| 1, Quality | 1707x1067 | 6352 /6812 | 26 /40 | complete, SR0, matched Present0 |
| 2, Balanced | 1485x928 | 16600 /17060 | 26 /40 | complete, SR0, matched Present0 |
| 3, Performance | 1280x800 | 22836 /23296 | 26 /40 | complete, SR0, matched Present0 |
| 4, Ultra Performance | 853x533 | 24000 /24460 | 26 /40 | complete, SR0, matched Present0 |
| 0, return native | 2560x1600 | 26408 /26868 | 16 /30 | complete, SR0, matched Present0 |

Each record matches its actual frame metadata and final colour. Unavailable
optional images remain unavailable; complete does not mean every listed product
exists. Root inspected the stationary outputs and the first reduced final image:
no nested/duplicated rectangle was visible in these observations. This is a
bounded scene result, not broad visual/performance acceptance or a repair claim
for the earlier rejection.

## Numeric reconstruction checks

The existing metadata-aware `audit-reduced-quality-20261116.py` was reused without
source changes on all four reduced captures. New outputs are
`native47344-quality{1,2,3,4}-numeric-audit.json` and matching logs under the
ignored research directory. It validates row pitches/formats and reads actual
stored products; it does not replay the vendor's temporal algorithm.

| Quality | Native colour crop to SR input | Copied depth-domain replay max error | Five auxiliary nearest replays |
| --- | --- | --- | --- |
| 1 | exact | 4.47e-8 | exact |
| 2 | exact | 7.46e-8 | exact |
| 3 | exact | 5.75e-8 | exact |
| 4 | exact | 5.96e-8 | exact |

Every captured input pixel in these scenes belongs to the world domain, with
no near-domain or clear samples. These results do not validate first-person
normalization. The replay uses serialized float32 coefficients with CPU double
arithmetic, so small GPU float/FMA differences are expected. Depth, motion,
Pip-Boy, VATS and refraction nearest expansions match their recorded convention.
Reinsertion is accounted for by unsigned packed R11G11B10 quantization.
Tiny stationary motion does not establish independently moving object history.

## Input and FG limitation

FG1/latency1 settings were also applied with foreground1 at Quality1. The saved
snapshot reports provider latency owner1, paced_frames3, marker_calls9,
derived_boundaries6 and order_violations0. FG remains active0/tags0 with
"Frame inputs or submission not matched". Rendered sources still lack the
required bound input owner; private BIND emissions followed by reason4 aborts
do not prove successful adoption or core acceptance. These limited provider
counters are not end-to-end rendered-source latency acceptance.

A Sky absolute drag moved the pointer without an observed camera rotation.
It is recorded as an unsuccessful movement attempt, not moving-game coverage.
No first-person hands, independent skeletal motion, particles, tessellation or
generated-geometry binding census was captured in this trial.

## Close and exact restoration

The controller completed its capture observation, then its120second native Quit
handoff expired before root finished closing. Its original result remains
observation_completed=true/restored=false with the cleanup timeout retained.
No forced close or kill occurred.

Root subsequently observed World -> native Save-and-Quit MainMenu -> actual
main menu -> native Quit Desktop confirmation. A retained process-object query
matched PID47344's exact creation time before GetExitCodeProcess returned0;
`post-close-exit-query.json` records that proof. This is distinct from the prior
direct world-to-desktop shutdown AV and does not repair that path.

The exact-run recovery helper was reviewed after64 functions-only checks. Its
complete21-entry preflight, fresh no-game/launcher queries, backup/hash checks,
durable post-exit loader/settings seals and per-write journal completed in
`resumed-restoration-20261116T1739498960236Z-dac148946b964e5596ef558cb9da86a6.json`.
Root independently verified all21 original destination states, exact ENB and
user settings restoration, and unchanged custom INI. The original timeout,
files manifest and result were preserved rather than rewritten as success.

## Remaining work

The earlier duplicated-image failure remains unresolved. Active draw/dispatch,
buffer/history, moving/near coverage, failure recovery and resize/backend/window
transitions still need validation and required repairs. Bound input adoption,
actual FG output and complete latency remain blockers. F4SE/ENB compatibility
follows native completion. Readiness flags remain0.

## Downstream receipt deployment, PID64776

Commit a99d155 deployed the compiled downstream diagnostics as Entry
`d0784e75cc475ca14629cf182da59158900056a64ef437f786e4930720366cb6`,
with unchanged Runtime22830370. Direct native PID64776 started at
2026-11-16T17:50:58.4740058Z; report is
`.local/fallout4-research/native-raster-20261116-185056-143`.
Repeated module receipts verified F4SE and non-system D3D11 absence.

Quality1 source6569/native7022 completed26 of40 products with SR0 and
matched Present0. Return-native source16176/native16644 completed16 of30
with the same results. FG remained active0/tags0. Saved latency snapshots
had owner0/paced0/markers0/order0, and the retained pre-close log contains
no prospective source emission or downstream receipt. This trial therefore
cannot localize the prior reason4 aborts. The first-distinct native gate
receipt remains mask3f9ff, lacking sealed/present at that poll.

Root rebound only the prior observer's exact Entry hash and fresh PID/report
configuration. All16 native/owned-detour guards passed. REA retained
`ev_11044b61541499dd2d41bfa4e9a83db62a5fb420675ea3040337f46a4e780b92`;
the durable512-row trace spans0.2875122seconds and stops at its row budget,
not15seconds. It includes15 worker Poll/controls/UpdatePlayer sequences on
TID67832 and main rendering/join on TID48216. Native Present entry is before
the adapter hook body arms its CPU phase, and instrumentation changes timing.
Polls overlapping that observed invocation do not prove the private phase
was armed or justify loosening source/admission gates. No live source or
end-to-end latency acceptance follows from this trace.

Root observed Save-and-Quit to actual MainMenu and confirmed QuitDesktop
within the controller handoff. This time the controller completed with
exit0, observation_completed=true, restored=true and no experiment/cleanup
error. Root independently checked all21 original destination states against
the backup manifest, including ENB and user settings. No forced close or kill
occurred. These clean close/restoration facts are separate from unresolved
input binding, earlier duplicated images and required moving/shader coverage.
