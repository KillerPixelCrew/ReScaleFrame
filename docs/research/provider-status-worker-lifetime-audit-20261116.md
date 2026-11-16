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
