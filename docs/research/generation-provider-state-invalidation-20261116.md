# FSR and XeSS generation state invalidation

Source/test freeze, 2026-11-16, after the separately verified DLSS/bridge snapshot
checkpoint `4b2fa36`. This batch is not built, executed or deployed by this worker.
It does not change that checkpoint's binaries or the coordinator's live trial.
The earlier DLSS-specific mechanism and validation are recorded in
[its note](dlss-generation-state-invalidation-20261116.md); those sources remain unchanged.

## Proven remaining race

The same shared provider lease that permits worker abort also reaches FSR and XeSS.
Before this change, FSR abort wrote plain history_valid, prepared_enabled and
state.active, and XeSS abort wrote plain history_valid and state.active. Configure,
prepare, after-Present and generic session_status accessed the same session fields
without a common synchronization boundary. Thus a copied bridge status alone could
not repair the provider's own races, and a late successful SDK operation could restore
history after abort invalidated it.

The affected owners are `runtime/backends/fsr/src/fsr_generation.cpp` and
`runtime/backends/xess/src/xess_generation.cpp`. No presentation bridge, native FG,
D3D11 host, adapter, CPU hook or game configuration was changed in this batch.

## Small common CPU mechanism

`runtime/backends/common/generation_state.h` supplies a private copied-state mutex,
status/options snapshot, history/prepared validity, monotonic prepared-source identity
and invalidation revision. Its methods copy CPU fields only, take no callbacks and
expose no lock to the caller. They make no SDK, native graphics, sleep, wait or logging
call. No public ABI, setting, provider selection or game policy changed.

Each provider reads a snapshot before external work. Successful generation changes
and abort advance the revision and clear prepared/history/activity. Preparation stages
clear activity conservatively; successful completion can restore history only when
its captured revision still matches. Successfully processed but invalidated sources
remain consumed for ordering. A stale active observation can merge actual statistics
without restoring activity. New valid sources observe invalid history and retain the
existing reset-constant calculation.

FSR's successful resize/context recreation also invalidates history using its original
revision. It cannot read a new revision after external work and silently absorb an abort
that occurred during that work. No state mutex covers context recreation or its existing
SDK drain wait.

The shared observation operation only copies explicitly supplied fields and merges an
explicit presented delta. Providers still compute their own values and validity flags.
This is not a shared SDK polling loop or a new graphics framework. The DLSS helper was
not generalized into this helper: its vendor capability merge and copied nonnull
retirement semantics differ and already have separate tested ownership.

## Provider contracts preserved

| Contract | FSR | XeSS |
| --- | --- | --- |
| Configure | Repeated mode/count requests still return without reset; accepted changes configure SDK generation disabled, then report effective Off until preparation. Unsupported Reflex/limit requests still refuse. | Existing count cap, XeLL settings and SDK enable order remain. Mode/count changes reset history; latency-only changes preserve history and do not undo a concurrent abort. |
| Prepare | Actual FFX configure/dispatch errors retain FEATURE_FAILED. Prepared history becomes valid only after dispatch succeeds. Abort after external work produces NOT_READY instead of late history resurrection. | Existing local 32-bit ID mapping, native command/resource checks, tags/constants and enable calls remain. Actual SDK failures retain FEATURE_FAILED. Disabled frames still call SDK disable; its failure takes precedence over invalidation refusal. |
| History | Same reset flag, prior validity and source-gap calculation, plus existing resize reset. SDK prepare failure remains invalid history. | Same reset flag, prior validity and source-gap calculation. Prepared reset/gap logging remains on its presenting owner. |
| Counters | Atomic generated/presented/dispatched callback counters are unchanged. No-UI generated_callbacks uses generated callbacks; HUD-less uses dispatched outputs. These are not confirmed generated display frames. | Successful present-query framesPresented contributes to total_presented, independently of whether its interpolation result succeeded. No callback totals are invented. |
| Present statistics | Existing GetLastPresentCount delta and wrap handling stay on the owner. Query failure adds no DXGI delta and does not claim TOTAL_PRESENTED validity for that observation. Absolute callback fields remain separate. | A negative query result still returns FEATURE_FAILED. A positive warning still returns OK without adding frames or validity bits. A successful query adds its count and validity bits; frameGenResult remains vendor_status even if it reports interpolation failure. |
| Pacing | begin/marker remain their existing no-op validation; no pacing counter or sleep was added. | Existing latency_guard, SDK sleep, local frame IDs and marker deduplication remain. Each successful XeLL begin increments its accounting counter under the short copied-state boundary after the existing SDK call. |
| Retirement | Null fence/value 0 remains unchanged. Existing synchronous game-queue ownership and caller completion fence still apply. | Null fence/value 0 remains unchanged. No new retirement query or wait was added. |

Provider status now returns a protected copied status. Owner-only FSR sequence, DXGI
baseline and callback observation baseline remain owner-only. FSR callbacks retain their
existing atomics and separate capture guard. XeSS ID mapping remains under latency_guard;
no copied-state lock is held when entering that guard or an SDK operation. The existing
XeLL guard still covers its SDK calls; this batch does not replace that synchronization.

Abort has no new SDK side effects. FSR/XeSS still return their immediate validation/abort
results, and invalidation does not erase GPU/callback lifetime obligations. Provider
object destruction still relies on the separately fixed bridge lifetime exclusion.

## Authored checks, not executed evidence

`tests/generation_state.cpp` exercises the same private helper used by both providers.
The narrow CMake target is `rsf_generation_state_test`, registered as
`generation_state_concurrency`. It is SDK-free and creates no window, device or live
process connection.

The authored cases cover:

- Distinct configured/effective generated counts and unchanged requested options.
- A deterministic external-work gap with another CPU thread aborting before late
  prepare completion; stale success and pre-resize reset must refuse without restoring
  history. A fresh source can rebuild history, and disabled completion preserves ordering.
- Generation changes versus latency-only XeLL changes, including abort before a
  latency-only publication. No successful preparation is fabricated from that publication.
- FSR absolute callback fields and validity replacement staying distinct from its
  cumulative DXGI delta; missing DXGI counts never become callback-based display counts.
- XeSS warning/error observations retaining previous successful counters/validity,
  and successful-query counts staying independent of interpolation activity. These
  are copied-publication inputs, not simulated executions of vendor SDK return paths.
- Two separate 4096-iteration, three-thread observation/abort/reader cases checking
  coherent status, exact presented deltas and distinct FSR/XeLL accounting. The XeLL
  counter calls here test CPU accounting only, not actual sleep or measured latency.

Provider SDK result branches, FSR DXGI wrap arithmetic, callback ownership and both
null-retirement functions were source-reviewed rather than tested with a driver by
this worker. Actual provider/device fixtures and coherent Release verification remain
coordinator work. The focused CPU command after that build is:

```powershell
ctest --test-dir build/windows-x64 -C Release -R '^generation_state_concurrency$' --output-on-failure
```

The five-file code/test SHA manifest is
`.local/fallout4-research/generation-state-fsr-xess-freeze-20261116.json`.
Only source inspection and focused whitespace checks were performed by this worker.
No build, test execution, device/game run, process query, UI action, staging, deployment
or commit occurred. This source fix is not live FG, quality, physical scanout or latency
acceptance, and no result for the coordinator's unchanged live DLSS checkpoint is inferred.
