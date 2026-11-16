# Latency sleep lock hazards

## Required review before integrating or changing a pacer

Never hold a lock across `xellSleep` that an older frame needs to submit markers,
prepare FG resources, submit rendering, or finish Present. Sleep can depend on
that older frame progressing. Sharing its lock with those callbacks creates a
circular wait. Audit the complete caller chain, including orchestrator provider
leases, settings changes, cancellation, and teardown, not just the SDK wrapper.
Apply the same dependency review to other vendor sleeps and blocking waits.

Include third-party Present hooks in that dependency graph. Fallout 4's repeatable
128-frame XeSS stall survived the sleep-lock fix: a CPU dump showed the application
waiting inside XeSS Present while its presenter thread waited for Steam's global
Present critical section. REA confirmed Steam holds that lock across its downstream
call. Dispatch an exactly registered RSF facade before entering that lock; retain
the full Steam handler for native physical receivers. Never release a foreign lock
manually, disable an overlay as the delivered fix, or infer success from queued FG
counters while the physical presenter is blocked. See the 2026-11-16 latency note
and `integrations/steam/src/d3d11_chain.cpp` for fingerprint/byte-guarded ownership.

Keep sleep serialization separate from frame-map and marker synchronization.
Reserve the new SDK frame identity under the state lock, release that lock for
sleep, then publish the usable mapping only after successful sleep returns.
Older published mappings must remain usable during the wait. Reject duplicate
begin calls and preserve provider lifetime throughout the call. A separate mutex
alone is insufficient if an outer lock still blocks the progress dependency.

Do not bypass the hang by inventing markers, marking an unfinished frame complete,
adding a second pacer, or publishing the new frame before sleep returns. Markers
must come from actual engine/input events. Cancellation and retirement must not
wait while holding a lock needed by the work they are waiting for.

Review `xellSetSleepMode` separately: Intel requires GPU activity to finish before
changing its settings. Serialize settings with sleep without blocking older-frame
progress. Do not assume a CPU mutex proves GPU completion or SDK thread safety.
Consult the consumed SDK documentation for concurrency and lifecycle requirements.

Verify physical presentation cadence separately from CPU sleep. XeLL requires
DXGI sync interval 1 whenever VSync is enabled. A native interval of 2, 3, or 4
must not silently throttle the FG output chain. Preserve VSync Off as interval 0
and report requested versus effective intervals. In the Fallout 4 trial, forwarding
interval 3 produced about 23-28 base FPS; normalization to 1 yielded a live sample
of 82 base/164 presented FPS with XeSS FG active. This was not a matched benchmark
and must not be generalized into an intrinsic XeSS-versus-other-vendor result.

## Focused regression and acceptance

Use the actual provider begin/marker implementation with a bounded SDK double:
make next-frame sleep wait for a prior-frame marker, deliver that marker from
another thread, and require both calls to finish. Check new-frame mapping remains
unavailable until sleep succeeds, duplicate begin rejection, and failure handling.
Release test synchronization locks before joining workers, including timeout paths.
This test must not create a graphics device or visible window.

Then validate the reported backend switch in the real game, including switching
away again. Record provider generation, frame progress and the user's observed
result. A passing concurrency test or transient active FG counter does not prove
the native hang fixed. Keep failed runtime acceptance explicitly failed until
the affected transition is verified. Respect disabled autocapture settings.

## Evidence and scope

On 2026-11-16 the user reported recurrence across three integrations. Fallout 4's
XeSS-FG switch hung; source inspection found `XegSession::latency_guard` spanning
`xellSleep` while marker and frame preparation also required it. The shared fix
is in `runtime/backends/xess/src/xess_generation.cpp`, with regression coverage in
`tests/xell_sleep_concurrency.cpp`. The lock cycle is demonstrated by the bounded
test; it is not yet proven to be the sole cause of the reported native hang.
Do not infer identities or evidence for the other two incidents from this note.

See `docs/research/fallout4-actual-latency-boundaries-20261116.md` in the repository
and the consumed `vendor/xess/doc/xell_developer_guide_english.md`.
Upstream: [Intel XeLL developer guide](https://github.com/intel/xess/blob/main/doc/xell_developer_guide_english.md).
