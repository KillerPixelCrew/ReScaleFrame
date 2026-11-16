# DLSS generation copied-state and abort invalidation

Source freeze, 2026-11-16. This change follows the separately committed recovery
baseline `ceaf1bd`. Root approved the short copied-state/revision design after the
provider audit identified a real shared-access defect. This worker performed source
inspection and whitespace checks only. Build, CPU-test execution, device checks and
live acceptance remain coordinator-owned and are not claimed here.

## Proven access gap

The presentation bridge permits a worker abort while the presenting owner configures,
prepares or polls the provider. Previously, `dlss_generation.cpp::abort_frame` first
called `rsf_streamline_host_abort`, then wrote plain history_valid, prepared_enabled
and state.active. Prepare, configure and poll accessed those fields without a common
synchronization boundary. The generic session_status callback also copied the plain
status while owner and abort writes could occur.

Removing a worker's SDK status query cannot repair those provider-internal writes.
Protecting the initial history read alone is also insufficient: an abort can invalidate
history during external SDK work, followed by late prepare success setting it valid
again. The question was how to synchronize CPU state without extending a lock through
SDK sleep, physical Present or GPU completion waits.

## Implemented owner boundary

`runtime/backends/dlss/src/dlss_generation_state.h` is a private SDK-free helper used
by the real DLSS provider. A short mutex protects copied status, options, history,
last prepared source, a monotonic invalidation revision and the copied retirement
fence/value pair. Its methods only copy or update CPU fields; they accept no callbacks
and expose no retained lock. No public ABI, option, SDK call, poll, sleep or per-game
policy was added.

Provider methods read a snapshot, perform their existing external operation, then
publish a bounded CPU update:

- Configure preserves its original validation, SDK option and Reflex results. A
  successful generation change increments the revision and invalidates preparation
  and history. A latency-only change updates options/effective Reflex without replacing
  newer history or invalidation state.
- Prepare validates the same frame, resources, command and token. Staging clears
  prepared activity conservatively while SDK tagging is pending. Constants use the
  copied prior history and source, retaining the existing reset calculation. Only
  successful tagging can publish valid prepared history.
- The existing host token marker mutex still protects token/constants/tags use. It
  is released before the final session-state publication. No session-state mutex is
  held across any SDK operation or host token call.
- Abort retains the existing token-abort call and immediate error result. Only a
  successful token abort increments the session revision and clears history,
  prepared enablement and activity. It neither disables SDK generation by a new
  option call nor erases resource retirement obligations.
- Poll performs the same existing vendor queries into local state, then merges only
  observed capability/statistic fields. Presented counts accumulate against current
  state rather than overwriting concurrent pacing increments. Configured/effective
  modes and options are not replaced by a stale poll copy. Active publication requires
  the captured revision still to match and successful prepared history still to exist.
- Status returns a protected copied status. Retirement returns a protected copy of
  the last observed fence/value pair, with its existing borrowed-until-destruction
  meaning. Begin-frame pacing accounting and the destruction mode decision also use
  the copied-state boundary.

The host cache remains an owner-published capability copy; worker abort does not
write it. Immutable session host/UI fields and chain lifetime remain under the
existing provider/lifecycle owner contract. The separate bridge status publication
and destructor-lifetime exclusion work is required for that boundary and is not
implemented or claimed by this provider change.

## Abort versus late success

A prepare captures revision R. If an abort completes while external SDK work is in
progress, it advances the revision to R+1 and clears prepared/history/activity.
A later prepare completion carrying R returns NOT_READY and cannot restore those
fields. Its successfully processed source remains consumed for monotonic ordering,
so the same source cannot be accepted again as a new preparation.

If completion wins first, a subsequent successful abort clears the published state.
If an abort happens before staging, revision mismatch refuses staging. A fresh source
at the new revision observes invalid history and therefore requires reset by the
existing constants calculation. A successful newer generation can later establish
its own history; an older revision cannot replace its configuration.

A poll started before abort may still contribute actual observed presented statistics
and retirement information, but cannot publish active from that stale revision.
No SDK result is fabricated or suppressed. An error before tagging success remains
an error and cannot make preparation active. The change conservatively invalidates
pending preparation rather than treating an incomplete tag operation as valid history.

## Authored validation, not executed results

The new `dlss_generation_state_concurrency` test compiles the same private production
helper without loading a vendor SDK, creating a window or device, or querying a live
process. Its cases are:

1. Ordinary successful preparation and observed activity/statistic publication.
2. A deterministic presenting thread paused between staging and completion, with
   another thread completing abort in that external-work gap. Late success must be
   refused, source ordering retained, and next-source history invalid.
3. Stale pre-SDK staging and stale active-poll rejection, while presented totals and
   the actual copied retirement pair remain available.
4. Latency-only configuration preserving valid history, generation changes refusing
   old completion, Off accounting, and latency-only publication preserving a prior abort.
5. A 4096-iteration three-thread poll/abort/reader case. Coupled capability and
   retirement fields must stay coherent; presented and pacing counters must retain
   their exact accumulated values. These are CPU accounting assertions, not proof
   of SDK sleep, measured latency, generated display frames or GPU retirement completion.

The test represents the SDK gap with latches; it does not pretend a vendor call was
executed. No successful mock provider result substitutes for a live result. The
coordinator must build the real provider path and run its appropriate existing device
fixtures separately.

The narrow CMake registration is beside the existing FG provider tests. Focused command
after the coordinator build:

```powershell
ctest --test-dir build/windows-x64 -C Release -R '^dlss_generation_state_concurrency$' --output-on-failure
```

The code/test manifest is
`.local/fallout4-research/dlss-generation-state-freeze-20261116.json`. It lists
`dlss_generation.cpp`, the private helper, the focused test and the narrow
`tests/CMakeLists.txt` change. No presentation bridge, native FG, host, adapter,
CPU hook or existing presentation-bridge fixture was edited by this worker.

The source fix addresses a proved data race and invalidation-loss mechanism. It does
not attribute a user image, prove live FG activity, establish latency improvement or
replace the pending coherent coordinator gate and native validation.

Root subsequently ran the coherent Windows Release VS2026 gate successfully:
225 passed/14 skipped/239 total in112.89 seconds, Rust fmt/Clippy clean,
including the actual helper concurrency fixture0.01 seconds. The explicit
snapshot test, hidden SDK marker/Present/resize/retirement regression and
rebuilt shared-SR recovery fixture passed. The latter initially failed on a
missing panel DLL; root release-built the real panel and supplied its exact
path before the passing rerun, preserving the failure and all assertions.
Logs, current Runtime/Entry/panel hashes and undeployed scope are in
`provider-concurrency-built-20261116.json`. Native testing remains pending;
these synthetic/device results do not identify the rejected image's cause.
