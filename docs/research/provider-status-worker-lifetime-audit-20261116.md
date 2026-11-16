# Worker provider status and lifetime audit

## Question and method

Can prospective CPU pacing read provider state concurrently with Present, and
what protects the provider session during that read? This is a source-only
generic audit. It does not attach to the game or attribute the rejected FG
flicker/split image to a race. The ignored
`provider-status-worker-lifetime-audit-20261116.json` pins reviewed files and
anchors. The previously frozen native_fg diagnostics hash remains
`e6b3975242e257413d85e868b36795995730dcc937e95872936835d7b3ed97c3`.

## Actual call paths

SOURCE_RESERVE dispatch calls `rsf_native_fg_cpu`, acquires the existing source
token, then calls `pace_cpu_frame` on the calling CPU thread
(`native_fg.cpp:799`,813). This path does not marshal to the graphics thread.
The measured integration supplies it from its worker. `select_cpu_pacing`
holds AntiLagOwner.guard and directly calls provider.status after independently
loading the provider API and session (`native_fg.cpp:622`,639). The XeLL status
augmentation repeats that raw-access pattern at line680.

DLSS provider.status is `rsf::session_status<DlssSession>`
(`dlss_generation.cpp:694`). Its entire operation is a plain struct copy of
session.state (`fg_helpers.h:116`). It does **not** call DLSSGGetState. Actual
DLSSGGetState is in poll at `dlss_generation.cpp:201`, called by after_present
at line678 and during initial creation. poll writes the plain state fields at
lines204 through241. Configure writes them at lines359 through362, prepare at
lines631/632 and abort at line691. DlssSession has no state mutex or atomic
snapshot (`dlss_generation.cpp:172`).

The graphics-owner callback lease in `d3d11_present_bridge.h:21` passes the
session to prepare. The bridge invokes prepare on its Present caller at
`d3d11_present_bridge.cpp:712`,768; native_fg directly calls status, configure
and prepare from that lease. After physical Present, the bridge invokes the
completed latency callback, then provider.after_present, then retirement at
lines637 through640. Retirement directly reads provider status
(`native_fg.cpp:1593`). These graphics-owner calls are ordered on that thread,
but do not share AntiLagOwner.guard with the worker.

A legitimate overlap exists even without switching: the completed latency
callback records predecessor Present return and completes the source
(`native_fg.cpp:1669`, `rsf_native_latency_present`), allowing the waiting
successor worker to enter selection before the graphics thread's following
after_present/poll has finished. No cross-thread happens-before edge protects
the subsequent plain session.state writes from the worker's whole-struct copy.
The source permits this interleaving; this audit did not measure its occurrence
or any corrupted value in the game.

## Lock and lifetime graph

| Lock or lease | Protected path | Missing connection |
| --- | --- | --- |
| AntiLagOwner.guard | Worker pacing selection and its local state | Present never takes it around provider state mutation |
| provider_guard, shared try-lock | with_provider wrappers for pace/begin/marker/acquire/input/abort/retire | Raw provider/session getters and worker status bypass it |
| provider_guard, exclusive | switch_provider, including old-session destruction | Does not exclude raw worker status; ordinary configure/prepare/poll do not take it |
| physical_guard | Physical swapchain pointer operations | Does not protect DlssSession.state or session lifetime |
| creation_guard | Initial bridge creation | Not a worker status lease or ordinary Present state lock |
| host.marker_guard/reflex_guard | Streamline host tokens/markers/Reflex operations | Cached provider-state copy takes neither; poll writes several state fields before marker_guard |
| Host.presentation_guard | Host render/capture operations | Native CPU selection does not take it |
| native_fg settings_guard/cpu_guard | Copied settings/status and keyed CPU owner metadata | Released before provider calls; neither protects the provider's own state |
| Transfer.vendor_guard | GPU transfer slot retirement/unknown-use metadata | Not held around provider.configure/prepare/status/poll |
| Caller COM lease | Scoped dispatch retains Bridge across Present callbacks | Worker raw session pointer has no COM/provider reference |

`with_provider` at `d3d11_present_bridge.cpp:1245` takes a shared try-lock and
passes the API/session/host as one tuple. Replacement at line361 takes the
exclusive lock, clears active_session and destroys the old provider at
lines377/378, then publishes the replacement and increments generation at
line395. The worker instead uses two independent accessors at lines1227/1228.
Atomic pointer loads avoid a race on the pointer variable, but neither keep the
pointee alive nor pair its concrete session type with the correct API. A
switch between those loads can yield an API/session mismatch; destruction
after the load can leave a dangling pointer during session_status. The
exclusive switch lock cannot exclude a reader that never acquires it.

Bridge destruction also clears the globals and destroys its provider without
taking provider_guard (`d3d11_present_bridge.cpp:407`,421). The scoped dispatch
COM lease at line1194 protects the Bridge on that caller, not a worker using
the global accessors. Thus the destructor does not currently establish the
same exclusion even for the shared-lease wrappers. Actual final-release
concurrency was not measured, and the runtime may retain objects on some
failure paths; that does not supply a general session-lifetime contract.

## Verdict and minimal repair proposal

This is a concrete source-level synchronization/lifetime defect, not merely a
missing annotation. A permitted worker status read can conflict with plain
provider state writes, violating the C++ data-race rules. Independently, the
raw session getters do not prevent use after replacement/deletion. No observed
flicker, skipped Present or native-8 refusal is proven to originate here.

Do not repair this by taking a shared provider_guard only around status: that
can cover replacement lifetime but still races with owner-thread writes.
Do not serialize Present, configuration, SDK calls and CPU pacing under one
large mutex.

The smallest generic direction is a copied status snapshot published by the
existing graphics/lifecycle owner, with backend, provider generation and valid
state in the same protected value. Publish after existing state-changing
owner operations using their existing status reads. The worker reads that
copy through a short snapshot lock and never dereferences provider/session or
calls SDK status for this purpose. Invalidate before replacement/unpublication;
publish the new generation only when ready. Unavailable/stale snapshots remain
explicitly unavailable under current refusal policy, not fabricated provider
capability. No game/vendor selection branch, source ledger, token, pacing wait
or new SDK poll is needed.

Any remaining cross-thread provider operation still needs the existing matched
API/session/generation lifetime lease. Destruction must participate in that
same lifetime protocol, with lock order reviewed against existing replacement
and retirement; a snapshot alone does not repair direct mutating calls or
final-release lifetime. Existing shared lease wrappers need not gain a lock
held across physical Present or a second pacing wait. This proposal requires
root review before code changes.

Worker Reflex status is a separate call: `rsf_streamline_host_latency_status`
at `dlss_generation.cpp:572` takes reflex_guard and marker_guard and queries
Reflex/PCL state. poll's DLSSGGetState is a different SDK entry point. The
cached C++ provider-state race above is proven without asserting an SDK
internal thread-safety failure. Broader SDK guarantees and the retained
process-owned host lifetime are separate from the unleased provider session.

Only the new note and ignored receipt were written. No frozen production
source, build, test, device, process or game state changed.

## Authorized snapshot implementation, source freeze

Root authorized the copied-reader/lifetime repair after reviewing the audit.
The earlier no-production-change statement describes that audit phase. This
new source phase changes only the presentation bridge/header, its private
snapshot helper, the two native_fg worker readers and the existing bridge
fixture. The earlier diagnostics remain unchanged. Root's reported224/14
Release and hidden marker pass precede this change and do not validate it.

`rsf_d3d11_present_status_get` is a size/version-checked C boundary returning
copied rsf_fg_status, backend, provider generation and validity. Short headers
return INVALID_ARGUMENT; mismatched versions return ABI_MISMATCH. No valid
snapshot returns NOT_READY and a cleared state with valid0. Snapshot validity
means the copied owner observation is available, not that generation is active,
the source is admitted, or its provider supports a requested feature.

The new private `ProviderStatusSnapshot` owns no pointers or resources. Its
mutex covers only copying, activation, invalidation and publication. Activation
begins a new backend/generation epoch with no valid state. Invalidation also
closes publication, rejecting a late stale copy even for the previous epoch;
owner reactivation is required. A transient failed cached observation clears
validity without closing that epoch. Publication checks the complete state
header and matching backend/generation.

The owner publishes after initial/lazy physical setup, completed replacement,
prepare callbacks, existing provider.after_present and resize's Off
configuration. Those observations call provider.status only, which is the
cached-copy function for current DLSS, FSR and XeSS providers. No new SDK poll
or physical Present is added. State is copied outside the snapshot mutex and
then published briefly. Existing owner status uses remain owner-only; the two
worker paths never load a provider/session pointer. Pacing selection uses the
backend from the same successful snapshot; absent state retains the existing
unavailable-provider behavior, with no new fallback selection policy.

Replacement invalidates before clearing or deleting its published session.
Destruction takes provider_guard exclusively to drain already-entered shared
provider leases, invalidate the snapshot and unpublish the session. It releases
that lock before the existing GPU drain, resource retirement or provider
destruction. Later wrapped calls cannot acquire the old session. Each Bridge
retains the concrete provider_backend that created its session, and uses that
API for retirement/destruction rather than a possibly changed global backend.
The existing failure path that retains GPU-reachable storage after a failed
drain remains a limitation; this change does not call retained storage a safe
shutdown or claim it was freed.

Lock order is provider_guard then the short snapshot mutex at lifecycle
boundaries. A worker takes AntiLagOwner.guard then the snapshot mutex, releases
the snapshot lock before any host/provider pacing call, and never acquires
provider_guard while holding the snapshot lock. Snapshot operations call no
logger, COM object or provider. No snapshot lock spans physical Present, SDK
sleep, CPU dependency wait or GPU drain.

The existing `tests/d3d11_present_bridge.cpp` now includes a pure compiled
`--status-snapshot` mode before any graphics setup. It exercises the actual
cache and cold exported query: short/version headers, unavailable output,
late-publication rejection, wrong backend/generation, transient clear/recovery,
two concurrent readers across4087 publication epochs, invalidation and a
retained value after publisher storage destruction. This does not fabricate a
provider or claim provider/device acceptance. The normal bridge fixture also
runs these checks before its existing scenarios. A real bridge destruction/
provider-switch device run is still coordinator-owned and required; these
cache tests do not dynamically prove that lifecycle path.

The remaining mutating-wrapper audit found a separate concrete race:
DLSS abort_frame clears history_valid/prepared_enabled/state.active while
owner configure/prepare/poll use those fields. The shared provider lifetime
lease alone does not synchronize mutation. It was immediately reported to
root, which assigned its provider-private repair to another worker. No vendor
backend implementation is changed here, and the snapshot is not presented as
repairing that race. XeLL begin_frame and other provider mutations still need
their own synchronization contracts; copying status is not a substitute for
them.

The ignored `provider-status-snapshot-freeze-20261116.json` pins this source
batch. Only UTF-8/LF, diff and source checks were performed by this worker;
compiled fixtures, full Release, device and game validation remain root-owned.

## Root verification

The coherent bridge and DLSS state batch passed Windows Release VS2026:
225 passed,14 skipped,239 total,112.89 seconds, with Rust fmt/Clippy clean.
The production copied-state concurrency test passed0.01 seconds. The separate
compiled `--status-snapshot` case and the exact two-argument hidden SDK marker/
Present/resize/retirement fixture passed. The rebuilt shared-SR recovery fixture
also passed after root supplied the freshly release-built Rust panel through
`RSF_OVERLAY_DLL`. Its first run failed because panel DLL error126 prevented
public Present status sampling; that failure is retained and no assertion was
relaxed. The full receipt is `provider-concurrency-built-20261116.json`.

Runtime SHA256 is
`e2c4958ada60bdc04ada49a4ab1b9f268fc813f436b1ed420949a3154b9309d9`.
The adapter remains `9b5c79df...`, carrier `e4a809e7...` and fresh panel
`ebea30f0...`. These binaries remain undeployed at this checkpoint. The
[provider-local fix](dlss-generation-state-invalidation-20261116.md) covers the
reported DLSS mutation race; equivalent FSR/XeSS abort fields require separate
review/repair. These checks do not establish that a race caused the user's
flicker or prove native visual, FG, latency or compatibility acceptance.
