# Native SR planning history and copied depth metadata, 16 November 2026

Question: why can a steady source retain RESET at every actual SR evaluation
when its engine pass has no RESET flag? The coordinator's retained
source4121/native4598 capture had engine flags22, but assembled SR constants
reset1 after more than1500 evaluations. Host logs alternated planning result-3
and evaluation result0 on each native frame. These are prior live receipts,
not a new worker experiment.

Source inspection confirmed the trigger in `Host::process`: every
`RSF_GAME_RENDER_RECONFIGURE` pass reached the non-evaluation branch and set
`reset=true`, even after an unchanged successful `prepare_pipeline`. The
subsequent evaluation copied that latch into its pass. This mechanism needs
no game convention or camera substitution to explain the repeated resets.

## Narrow shared repair

The existing private `prepare_pipeline` now returns its already-computed ready
predicate. A preparation-only call preserves the reset latch when that predicate
succeeds. It still returns NOT_READY to retain native temporal processing for
that planning call. It neither evaluates nor clears a pending reset.
Requested settings generations and successful provider/quality/output/preset
changes retain their existing reset producers. Failed preparation, actual
failed SR and disabled SR still request reset. The no-camera planning early
return, cold scene ownership, actual fallback result and public ABI are unchanged.

The existing explicit hidden `d3d11_host_shared_sr_device` fixture now drives
real host planning/evaluation pairs. Source1 explicitly refuses because its
pre-configuration FRAME_BEGIN was not retained; it completes Present before
source2 starts the first real owned CPU/token interval. Subsequent sources
include two valid planning calls plus the existing no-camera
refusal before actual evaluation. It changes Quality to native through the
public overlay settings API, and derives actual input dimensions from the host
plan. It also exercises a real SR refusal with absent depth and an explicit
engine RESET. It requests the existing capture path and checks
`sr_constants.json`, which records the assembled backend frame actually offered
to the provider, rather than copying the branch's Boolean into a mock.

| Source | Expected actual backend reset |
| --- | --- |
| 1, cold source without retained CPU/token | Exact EVALUATE_FAILED, no assembled backend constants; complete Present |
| 2, first owned successful evaluation | 1 |
| 3, stable planning/evaluation | 0 |
| 4, real Quality-to-native change with repeated planning | 1 |
| 5, stable native | 0 |
| 6, actual missing-depth SR refusal | Exact INVALID_ARGUMENT, no successful backend evaluation |
| 7, recovery after refusal | 1 |
| 8, stable recovery | 0 |
| 9, explicit engine RESET | 1 |
| 10, following stable source | 0 |

These are authored expectations, not completed checks. The fixture keeps FG Off,
retains its output-pixel/debug-queue checks and expects eight successful pipeline
evaluations and exactly one pipeline refusal for the cold source. The missing-depth
refusal occurs before the pipeline. It writes newly requested diagnostic bundles under the caller's
scratch SDK root, preserves pre-existing bundles and cancels each request after
reading its own assembled constants. It makes no completed-capture, physical
scanout, gameplay, latency or visual-quality claim.

## Existing capture metadata gap

`rsf_input_dump_pass_metadata` already receives a full-size current render pass,
but engine_pass.json omitted its inline ABI23 `depth_domains` facts. It now
appends requested/struct_size/version/domain_count, clear enable/value, reserved
words and both fixed inline domain slots. Each slot includes classify bounds,
endpoint flags/reserved word, actual allocation viewport XYWH and sampled depth
interval, and all16 row-major source-clip-to-target-clip matrix values. Existing
render/output rectangles remain the active-region facts. All-zero legacy data
is explicit requested0/size0/version0/count0 with zero slots.

The writer copies both slots regardless of the advertised count, so a malformed
count cannot overrun the inline array and unused or invalid facts remain
inspectable. Existing number/array writers preserve float roundtrip precision
and emit null for nonfinite values. This records supplied facts without
validating or changing normalization policy. No ABI, export, option, pointer
lifetime or graphics processing path was added.

The existing input_dump WARP test now requires exact fractional viewport/depth
bounds, both endpoint rules, every element of two distinct matrices and explicit
legacy zeros. It also checks nonfinite JSON safety, bounded malformed count,
short-pass refusal and unchanged selected metadata after a stale native identity.
Those cases remain in the existing `input_dump` CTest target.

Worker checks were scoped source review and `git diff --check` only. No build,
device/GPU/game/UI/attach or deployment was performed. The coordinator owns the
coherent native gate and explicit build of the EXCLUDE_FROM_ALL
`rsf_d3d11_host_shared_sr_device` target; default CTest does not run that device
regression. Its hidden invocation after rebuilding is:

```powershell
& './build/windows-x64/bin/Release/rsf_d3d11_host_shared_sr_device.exe' 'D:/Coding/ReScaleFrame/.local/fg-provider-sdk-comparison-20261115-202429'
```

Use a scratch root with the matching vendor directories and no game installation.
Actual device reset sequences and new captured descriptor contents remain
unverified until those coordinator checks finish.

## First coordinator run and fixture cold-start correction

The coordinator built Runtime and the explicit hidden fixture successfully.
The initial build command also named nonexistent target rsf_input_dump; changing
the command to rsf_input_dump_test corrected that invocation, not source code.
The input_dump CTest passed in0.16seconds, including the new descriptor checks.

`planning-history-device-20261116.log` exited1 at source1. Host initialization
happens in its first actual scene call; Host::cpu correctly drops an earlier
FRAME_BEGIN while bridge_bound is false. The shared-host evaluation then
requires the absent source token and returns native NOT_READY(-8), mapped to
pipeline/host EVALUATE_FAILED(-10), before sr_constants exists. The fixture's
original assumption that this source could both bootstrap and evaluate was
wrong. That failed log is retained; it establishes no reset-regression result.

Only the fixture was corrected: source1 now requires that exact refusal, absent
CPU ownership/token and backend constants, zero evaluated/one pipeline-refused
counts, then completes Present. Every later source requires its real CPU begin
and token before planning. The measured reset sequence starts at source2 and
real-change/failure/explicit-reset cases shift through source10, still totaling
eight actual successful evaluations. No production ownership/token/readiness
condition was loosened and no arbitrary failure was ignored. Scoped source and
diff checks passed; the corrected device rerun and complete new gate remain
coordinator-owned and unrun by this worker.

## Coordinator validation

Windows Release VS2026 `verify-planning-history-20261116.log` passed with
223 tests passed,14 skipped in128.36seconds and clean Cargo fmt/Clippy.
The separately rebuilt hidden fixture passed in
`planning-history-device-r2-20261116.log`: cold source1 refused exactly as
specified, then actual backend reset values for sources2,3,4,5,7,8,9,10 were
1,0,1,0,1,0,1,0. Source6 exercised the missing-depth refusal. Eight successful
evaluations completed with FG Off and output RGB approximately
0.24939,0.499268,0.749512. This validates the planning-history behavior on the
device, including preservation of genuine resets, rather than only the branch.

The resulting Runtime SHA256 is
`8570ccc20a05fcde6c533e8bd3748c827cf6d1ed842062c1e90596e5bf2375b3`.
Entry remains
`463b1eed47090204dbb301b0332ca5cb9e5938907a67274994d59980e5aed0c7`.
This batch is built and device-tested; its next game capture must still verify
stable history and the newly serialized same-source depth descriptor.

## Plan-only capture and status correction

The subsequent native-only trial exposed a separate generic host defect. Root
reported source16183/native16705 captured only the three native inputs at the
planning rectangle, with SR result NOT_READY(-3), before actual evaluation.
The current baseline was c32d8f7 with the coordinator's full Release gate of
224 passed and14 skipped. This receipt does not invalidate the earlier measured
reset sequence, but shows that its capture request was placed too late to test
planning itself.

Source review found `Host::process` calling `capture_inputs` before testing
RECONFIGURE. That function exchanges the pending capture request, opens a frame
record and snapshots resources. Planning then published NOT_READY into both
the overlay report and `last_result`, even after an unchanged successful plan.
An actual evaluation could succeed immediately afterward, causing alternating
preparation/evaluation result logs and transient not-ready status. A plan is
only a configuration operation: it produces neither a render capture nor a new
evaluation result.

The minimal correction leaves capture acquisition exclusively in non-planning
calls. A successful unchanged plan returns NOT_READY as before, while retaining
the last actual evaluation report and result, including an actual refusal.
Unchanged means the settings generation, running pipeline, backend, quality,
preset and output dimensions already match before preparation, and preparation
still reports ready. Cold, changed, disabled or unsuccessful preparation does
not qualify and continues publishing preparation-only NOT_READY with no
effective backend. A no-camera size plan has the same status rules; it never
evaluates or reads camera matrices. Pending reset state is neither cleared nor
manufactured by the unchanged-plan return. No provider, token, pacing, owner or
capture completion rule changes.

The existing real shared SR device fixture now requests capture before the
plan/plan/evaluate sequence, asserts that planning creates no bundle, then
requires the actual source's successful SR status, selected raster dimensions
and backend reset constants. It retains the exact cold refusal, missing-depth
refusal and eight-value reset sequence. Additional unchanged plans after each
actual evaluation must preserve its success or refusal when Present samples
the public report. A final real quality change must instead publish NOT_READY
and no effective backend. The fixture still cancels its assembled-input dump;
it makes no complete final-image or matched-Present capture claim.

Only source/diff and UTF-8 checks were performed by this worker. The new device
fixture and full Release gate are pending coordinator execution; the earlier
224/14 gate does not validate this correction. The original failed run and
capture remain evidence, with no new game or visual acceptance claimed.

## Coordinator capture and diagnostic batch validation

The coherent Windows Release VS2026 gate
`verify-inventory-capture-diagnostics-20261116.log` passed:224 passed,14 skipped,
238 total in118.80seconds, with clean Rust fmt/Clippy. The explicitly rebuilt
hidden shared-SR device fixture passed in `capture-plan-device-20261116.log`: a
capture request survived plan/plan calls and recorded the actual evaluation
source/raster; unchanged plans preserved actual success/refusal status, while
a genuine quality change cleared stale output status. The existing actual
backend reset sequence remained1,0,1,0,1,0,1,0, with exact cold and missing-depth
refusals. FG remained Off. This fixture does not claim complete final-image
capture or a native game result.

Built Runtime SHA256 `22830370ac079440177912e1b77680b37d55b7b1cd43915b6e2a68a4f70ce772`
and Fallout4 Entry SHA256 `be2e0bbff6f030a2e69eade3bd3ea57f0d2ac54f996b00d6820b32d48034109f`
identify this batch. CPU conjunction and reduced-scope receipts are built but
not observed in a new game run; no image, source-binding, FG or latency repair
is claimed from these diagnostics.
