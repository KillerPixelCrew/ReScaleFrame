# Scoped D3D11 creation beneath external wrappers

Question: preserve external device/context/Present work while providing the
application virtual buffers before an independent HWND chain escapes. The exact
ENB0.505 and Skyrim1.7.104 identities, constructor and forwarding behavior are
in [the adapter ownership audit](skyrim-enb-chain-ownership-20261116.md).
[The On12 note](optional-d3d11on12-loading-20261115.md) retains failed late
replacement experiments, captured AVs and measured E_ACCESSDENIED. Late outer
replacement independently bypassed the external wrapper's Present work. No
extra owned native reference was established, so force-release was never valid.

This implementation reuses the generic factory hooks and create_bridge, adding
an explicit adapter-owned transaction instead of changing wrapper globals or
resolving an unwrapped creation API. Host ABI6 adds presentation_bridge mode3
and size/version checked rsf_d3d11_creation_scope with window and token. The
adapter calls rsf_d3d11_host_creation_begin before its complete native creation
call, then rsf_d3d11_host_creation_end exactly once on the same thread. End takes
borrowed existing rsf_d3d11_native_chain outputs, or null for native failure.
Native arguments, device/context, original HRESULT and outer swapchain remain
the adapter's responsibility and are never replaced/released by End.

Factory interception forwards unchanged outside that explicit thread/window
scope. One process-wide token prevents overlapping or nested transactions. Only
the first eligible creation is captured; nested SDK work remains excluded by
the existing creating guard. Ordinary modes0/1/2 retain their startup mechanisms.
The captured inner facade supplies its already allocated shared D3D11 textures
for GetBuffer during external construction, without creating a physical HWND
owner. This supports native texture metadata and uploads, including the audited
48-byte raw SetPrivateData annotation. GetDesc/GetDesc1, fullscreen description,
HWND and pending Present-count queries use application metadata. Physical-only
operations refuse during the creation transaction rather than starting nested
presentation. Physical creation/provider ownership remains on the normal later
Present path, so SR and vendor FG remain available through the same bridge.

End consumes the valid scope even if its result is malformed or refused. A bad
scope header, wrong token or wrong thread leaves the actual owner scope open for
a correct End. A captured-but-refused bridge is faulted, cannot become an admitted
owner and cannot later Present. End validates the captured inner identity against
the final outer chain's canonical IUnknown, native HWND, device, context device,
and outer GetDevice identity. The host retains the verified outer reference.
Only that explicitly admitted alias and its actual inner owner satisfy is_owner;
an arbitrary pointer returning a similar interface is not registered implicitly.

The engine keeps calling outer Present, so external pre/post work surrounds the
actual inner Present. The bridge callback retains the actual inner pointer; no
callback source identity is rewritten. Host::present verifies that admitted
inner against the retained outer's canonical identity. Existing native_window
matching already compares canonical IUnknown while retaining the outer WINDOW
lease and its frame/view/scope facts; no change was needed there. The existing
native_scene path verifies the outer GetBuffer resource identity. Host resource
device checks now compare canonical identities, preserving rejection of foreign
devices. Input/token/pacing selection and measured versus rendered provenance
are unchanged.

## Focused checks and limits

The existing host contract fixture adds scope size/version/owner and unsupported
mode refusal without dereferencing graphics pointers; unknown mode is now4.
The existing explicit rendered-FG device fixture accepts optional --scoped.
It exercises nested refusal with untouched token, empty cancellation, malformed
result closure, real inner creation, constructor-time GetBuffer/annotation,
preserved outer output and canonical alias admission. Its forwarding outer
fixture performs real pre/post Present work. Every source checks that the actual
inner event matches the outer WINDOW and that outer pre/post totals survive late
generation switches, SR Off/On and negative source cases. The original unscoped
fixture remains available with its original command line.

These are added checks, not claimed passing device results. This worker performed
source review and git diff --check only, with no builds, GPU/game runs, install
changes or commits. Coordinator checks: eng/verify.ps1 Release VS2026, build the
existing excluded rsf_d3d11_host_rendered_fg_device target, then run it once with
the scratch runtime root and --scoped. Its existing exit77 remains incomplete,
not acceptance. A bounded actual ENB run must establish that native creation
reaches the intercepted factory, the audited ENB startup operations accept the
virtual resource contract, outer Present remains active, the physical HWND chain
is created once and SR/FG work across scene entry. Full ENB shader behavior,
fullscreen/resize, other wrappers, physical scanout and visual quality are not
established by a forwarding fixture or source review.

## First coordinator gate and fixture ordering correction

The combined Release VS2026 verification log
`.local/skyrim-research/verify-enb-scoped-20261116.log` records exit0,
219 passed, 15 skipped, and clean formatting/Clippy. The separately compiled
excluded fixture's first --scoped run exits1; its full receipt is
`.local/skyrim-research/enb-scoped-device-20261116.log`. Scope admission,
canonical outer ownership, constructor-time virtual buffers and first Present
succeeded. Bootstrap source1 matched actual native presentation, but FRAME_END
was refused and the CPU snapshot was absent. This is not a successful full
device regression or ENB acceptance, and no subsequent ENB run was performed.

Source tracing identifies a fixture ordering error: it omitted the existing
rsf_d3d11_host_bind_presentation handshake before source CPU facts. Host::cpu
correctly drops bridge events until bridge_bound; FRAME_BEGIN and SIMULATION_END
therefore never reached the ledger. The first process_upscale configured the
host mid-source, after FRAME/SUBMISSION had also been ignored. FRAME_END then
reached the ledger without a begun source and returned0. Native ownership could
still match the later WINDOW/scene lease, so that accepted record did not prove
CPU-frame completeness.

The fixture now explicitly binds the verified returned application chain after
startup Present and before publishing its first source, matching the adapter's
existing binding service. Binding initializes window/overlay ownership, not scene
readiness or SR. The runtime CPU guard, token/latency policy and source assertions
are unchanged. This correction is harness-only; local validation is source
review and git diff --check. Coordinator should rebuild/rerun the excluded
fixture with --scoped before any ENB experiment; the broad gate already passed
and no production source changed in this correction.

## Scoped fixture passed; actual DLSS switch exposes forwarding singleton conflict

The corrected excluded fixture passed (exit0) in
`.local/skyrim-research/enb-scoped-device-bind-20261116.log`: 51 sources,
37 captures, 35 SR evaluations and 32 generated tags/callbacks, including its
outer-wrapper and canonical WINDOW assertions. This is a rendered-source FSR
fixture. It does not establish actual ENB plus DLSS-G/Reflex or the installed
Steam/RTSS routing behavior.

Actual ENB startup and SR subsequently ran for approximately1000 evaluations in
PID8316, `.local/skyrim-research/input-acceptance-20261116-004814`. DLSS-G enable
and Reflex application preceded C0000005 after source1044 final-done. That last
receipt is not a fault address: final-copy queues D3D11 work and an unflushed
Signal, not GPU completion. The switch code retains the facade, outer alias and
application D3D11 shared textures while replacing only the drained physical
D3D12 chain/provider. No freed facade was established by that source audit.

Successive passive receipts localized and then distinguished the dispatch issue:

- PID46312, `input-acceptance-20261116-004946`: DXGI+8e3a null read through
  RTSS+72ed1, Steam+9c24f and ENB+6c6fd. R14/R15 equal the logged application
  facade, not the old/new physical chain. The caller has passed the virtual
  receiver to DXGI's concrete implementation. This alone did not locate the
  dispatch mutation.
- PID54816, `input-acceptance-20261116-005605`: exact ENB hash verified; its
  outer member28, inner Runtime vtable131908, Present98e50 and entry detour to
  Steam9c130 remain unchanged between baseline and AV. Vtable replacement was
  therefore not the measured mechanism. The handler's forwarding storage had
  not yet been observed.
- `input-acceptance-20261116-010318` missed attachment timing. It adds no
  dispatch evidence and must not be counted as a successful observation.
- PID51604, `input-acceptance-20261116-010450`, REA
  `ev_11437f3723372aa92903837403f6a3af7028a542ebfc6c53992ee9d837ad1e15`:
  Steam global1834b8 initially points at7ff9a1680080. Its saved Runtime prologue
  and following relative jump return to Runtime Present+5. After switching it
  points at7ffab6c30c00, whose indirect route through7ffa88dd0170 enters
  RTSS+72db0 and ultimately DXGI. The facade1eefd8d01f0, its vtable and its
  entry route to Steam remain unchanged; the AV receiver remains that facade.
  This is direct evidence of changed singleton forwarding, not a changed facade
  slot or a claim based only on final-done. The session detached cleanly.

Read-only exact Steam DLL disassembly (SHA256
`21c0c1ddb4259c1156356bdfe8432dff504d52c7b96d19718688cff2d5a6e52f`)
matches this receipt: handler9c130 forwards normal Present through global1834b8
at9c23e..9c24d, retaining the receiver. Installer9ca76..9ca9a passes that same
global output to helper95dd0 when registering another vtable Present target.
The hook can therefore remain installed on the virtual implementation while
its common original pointer is retargeted to a native implementation.

### Remaining generic contract gap

The demonstrated correction needed is separation of application-wrapper hook
registration from runtime-private physical/provider creation. One forwarding
slot cannot safely represent two incompatible concrete Present receiver types.
The application must still run ENB/Steam/RTSS pre/post work, and private chain
creation must retain the required vendor upgrades.

No supported implementation of that separation is established by the current
interfaces. RSF's existing creating guard only prevents its own factory hooks
from recursively capturing SDK work; it cannot suppress external registrations.
The DLSS provider currently calls host.factory->CreateSwapChainForHwnd, then
rsf_streamline_host_upgrade_chain. The host already requests Streamline's
eUseManualHooking, so simply enabling that existing flag is not a repair.
No reviewed setup field provides a per-chain external-overlay registration
opt-out. The current trace brackets a whole provider switch, not those two
registration boundaries individually.

The smallest missing prerequisite is a registration-side contract for private
creation: identify whether the singleton is changed by native factory creation
or the subsequent SDK upgrade, along with the chain/interface target that is
registered at that exact boundary. A bounded receipt immediately before/after
those two existing operations, or the audited installer invocation, can resolve
this without another shader, capture or pacing framework. Only then can a
supported internal-only creation route be selected and checked to preserve the
application hook registration and required vendor proxy. If no such route exists,
receiver-aware forwarding after external pre-work and before typed native code
needs its own proven callback/lifetime contract; none is currently available.

No production edit is made in response to the singleton receipt. Overwriting
Steam's global per frame is racy and module-specific; stripping its stable
detour discards application work; jumping into an arbitrary hook interior lacks
a valid contract; fabricating a native DXGI object or force-releasing someone
else's references is invalid. No game/vendor-module-name branch, overlay-disable
fallback or unsupported private factory bypass is introduced. These experiments
all retain verified backup restoration/no remaining game process per coordinator.
This final step is source/receipt review and documentation whitespace checking
only, with no worker build, game/device run, installation mutation or commit.


### Factory registration boundary and downstream dispatch, 16 November 2026

Coordinator report `input-acceptance-20261116-011617`, PID42964, REA
`ev_f961e331d757b913dc8728d2450f719fc1aed65ddcc0f4d598111d704eced99a`,
contains all three hook-ready receipts and the armed receipt. Its ten bounded
creation rows resolve the preceding registration question: the Streamline
factory call invokes the DLSS-G creation path, which invokes the external
installer for the native DXGI chain. The accurate installer stack includes
Steam9d031, sl.dlss_g672cc/4fc45/54fbb, sl.interposer29158 and Runtimea5112.
Helper95dd0 changes original1834b8 inside that factory call. Factory-after
returns a Streamline proxy; GetNativeInterface returns a different native chain,
so the inlined upgrade helper skips UpgradeInterface. No second upgrade call
or second registration is established by this run. Coordinator reports clean
trace detach, all four backed-up files restored and no remaining game process.

The bounded script and its mock checks are preparation evidence, separate from
that coordinator-owned game observation. The current Release has no matching
PDB; RuntimeA510F/A5112 factory call/return and A5144/A514B native-interface
call/return were identified by exact-image static disassembly, not symbol lookup.
The unsuccessful 010318 attachment remains non-evidence.

The approved narrow repair separates exact-build external compatibility in the
adapter from a generic runtime receiver service. Host ABI7 adds the size/version
checked `rsf_d3d11_host_dispatch_present` request. It checks the exact registered
inner pointer without QI or dereferencing an unowned receiver. A valid unowned
call sets handled=0 and preserves the native result and arguments. A known owner
sets handled=1, including an unavailable or faulted owner, and returns its actual
HRESULT through the request. Invalid service calls do not authorize native
fallthrough. Frame/camera and game ABI versions are unchanged; existing shared
host consumers compile against the host version macro.

The inner virtual Present now delegates to a separate nonvirtual, non-inlined
body. The service invokes that body directly after external pre-work, avoiding
reentry through the same detoured virtual slot. The caller must retain its normal
live COM receiver lease throughout the synchronous call. Exact raw comparison
uses that lease; a temporary owner AddRef keeps the bridge alive across callbacks
and is released on return. No host/provider mutex spans the call, no external
continuation is retained by the runtime, and the process-lived host remains valid
while its adapter is inactive. A captured but not yet admitted owner returns a
handled DXGI_ERROR_INVALID_CALL. Cancellation/fault does not make a live known
facade eligible for a native-object fallback.

Adapter-owned compatibility is separately responsible for exact image and byte
guards, cold-start patch admission, thread-safe instruction publication and
process-lifetime module/cell pinning. The agreed mechanism redirects the two
verified forwarding-pointer load operands to a stable dispatcher pointer cell;
it leaves the external original slot and installer untouched. An unowned physical
or provider receiver uses that original slot's current native continuation.
This preserves the required vendor factory hooks and lets external Present
pre/post work surround the real continuation. It is not a per-frame singleton
restore, private factory bypass, or named-module policy in shared runtime code.
Adapter implementation and publication safety remain under its owner's review.

Focused source coverage now checks service owner/size/version refusal and
unowned-pointer/result preservation. The existing explicit --scoped device
fixture routes outer Present through the new downstream service, refuses its
canonical outer alias, checks normal/TEST unadmitted owner failures, and asserts
outer pre/post and handled totals across all later source and FG phases. These
new checks have not been built or run by the worker. Required coordinator checks
are the Release gate, the rebuilt explicit --scoped fixture, then the bounded
actual external-wrapper SR/FG transition; the earlier fixture/gate receipts do
not validate this new dispatch or the adapter patch. No worker game/device run,
installation change, build or commit was performed for this change.


### Emitted Present tail jump, 16 November 2026

The next coordinator Release gate passed 219 tests with 15 skipped, formatting
and Clippy clean, and the updated explicit --scoped fixture exited zero. The
actual `input-acceptance-20261116-013358` run, PID42072, installed both adapter
forwarding hooks and admitted the scoped outer chain, then exited C00000FD
before FG apply. Its loader log reaches native presentation binding. This is
not successful external-wrapper acceptance and does not establish a crash stack.

Read-only disassembly of the exact built Runtime SHA256
`0e0258a1ef80a20c0e96c675bff6d4295d20051fd144a0f9c8add79d3e8dccc7`
verifies an emitted-code defect in the proposed continuation isolation:
virtual Present RVA98ed0 is `e9 ab 2d 00 00`, a direct jump to the nonvirtual
body at9bc80. The exported dispatch service atBEB0 calls internal dispatch9e4b0;
its owned path at9e517 also calls9bc80. Therefore source-level noinline alone did
not separate the external entry's hook target from the internal continuation.
Present1's successful path also tail-jumps to9bc80 at98ec8.

The exact Steam installer supplies hop count1 at9ca81, through95dd0 to95220.
Its loop95297..952d4 recognizes EB, E9 and FF25 and follows the destination
before installing the hook. This makes the initial Present jump expose9bc80
as the registration target, while the runtime dispatch calls that same body.
That is a verified hook-reentry mechanism; attributing the observed C00000FD
specifically to this cycle remains an inference without the failure stack.
Independent adapter review agrees with the emitted-code identities and found no
relay/ABI defect in the inspected Entry image; receipts are
`.local/skyrim-research/runtime-present-tail-audit-20261116.txt` and
`.local/skyrim-research/entry-present-tail-audit-20261116.txt`.

The narrow shared repair keeps the noinline body and adds a volatile HRESULT
store/load after its call in virtual Present. The post-call observable operation
prevents replacing the wrapper with a tail jump, while retaining every HRESULT.
Present1 retains its existing Present delegation and now also stores/loads the
result after the call. No wrapper, forwarding cell, installer, ABI, ownership or
provider policy is changed. The explicit --scoped fixture now checks the actual
public Present entry bytes before external registration, rejecting the initial
jump forms followed by the audited installer. This is an emitted-code regression
check that fails the preceding binary, not proof of game operation.

Only source review and `git diff --check` were performed by the worker. Coordinator
must rebuild, verify the public entry has a real call/return boundary and that the
service still calls the separate body, then run the focused --scoped fixture and
bounded actual external-wrapper transition. The earlier successful gate/fixture
precede this tail-call repair. No worker build, device/game run, deployment or
commit was performed.


### Post-isolation run and reset triage, 16 November 2026

Coordinator's focused rebuild and explicit --scoped fixture passed after the
volatile result barrier. Exact Runtime SHA256
`e4a218853fc1cafe273ed77be74c749afedb22ee985ef9bf5eb34cdc948bc7a2`
has a real SUB/CALL9bcb0/store/load/RET Present entry at98ef0, while internal
service dispatch9e547 calls9bcb0 directly. The emitted-code receipt is
`.local/skyrim-research/present-isolation-rebuilt-20261116.txt`. The preceding
full Release gate passed 219 tests with 15 skipped and clean formatting/Clippy;
that gate and the later focused checks are distinct validations.

Actual coordinator run `input-acceptance-20261116-013840`, PID34560, completed
the 45-second SR observation and survived six seconds after the DLSS-G/Reflex
apply. The loader records a successful generation2 provider switch, post-switch
physical Present progress and ongoing SR/Reflex reports, correcting the earlier
immediate transition-crash outcome. However, the sampled status still has
fg_active=0, tagged=0 and reason "Resetting temporal history", with foreground=0.
Provider output330/base56 does not override the missing admitted/tagged frames
or prove generated-frame delivery. The close exit remains C0000005 despite
observation_completed=true. All four files were restored per the coordinator
and restoration receipt; no game remained. This is limited coexistence progress,
not successful FG or normal shutdown acceptance.

Read-only source triage locates the reset reason at native_fg.cpp's prepare:
continuous requires prepared inputs and no RESET/NO_FG bit on the selected pass,
and no NO_FG bit on the matched WINDOW lease. RESET has flag value1 and NO_FG has flag value16.
The reason therefore groups a temporal reset and explicit frame exclusion; it
does not by itself prove a camera-history error. With that gate false, the core
configures FG Off and returns before tagging. This matches prepare-configure=-8
followed by prepare-configured=0: eligibility refused, provider configure succeeded.
Reason precedence can hide other simultaneous invalid predicates, so this label
does not prove that CPU, HUD-less or identity checks otherwise passed.

Adapter presentation_inputs.cpp sets RESET when its history conjunction fails:
camera_frame history validity, same camera, consecutive native frame, unchanged
render size, previous view-projection matching the last current view-projection,
and exact previous/current origin match. It independently sets NO_FG when the
native CPU owner is ineligible or the screen is not gameplay. Later ownership
propagation can also add NO_FG. SR metadata always carries NO_FG and is not itself
an FG input lease. The current log has no FG eligibility diagnostics identifying
selected pass flags or WINDOW flags, so it cannot discriminate these causes.
The smallest next reset receipt is the existing FG eligibility diagnostic plus
matched WINDOW flags; if RESET alone is established, only then identify the
failed adapter history predicate with a bounded adapter-owned receipt. No
history check should be relaxed from this status string alone.

Foreground is not tested in this core prepare gate or the reviewed DLSS-G
configuration path. The DLSS foreground query reports driver diagnostics; the
frontend also exposes foreground as status. render_host.cpp only chooses the
background wording after more specific FG reasons, and overlay_api.cpp derives
this reset wording directly from the native FG reason. Thus foreground=0 is not
the source of this core rejection. This source review does not establish whether
vendor/driver activity has additional focus requirements; the core is already
withholding FG here for a separate documented reason.

No static close-lifetime defect is established. The host retains its outer chain
and graphics COM leases, host suspension only changes the active flag/generation,
and the adapter compatibility relays/modules remain process-lived. Those facts
are not a proof of safe external teardown, and the earlier ENB-only close AV
remains a separate baseline limit. No speculative lifetime repair is made.

For the smallest discriminating close receipt, reuse the existing passive
`.local/skyrim-research/input-acceptance-20261116-010450/trace-before-creation-boundaries.js`
with its first config line removed and a new exact owned PID/exclusive output
path prefixed. It has no hardcoded Runtime offsets or old Runtime hash guard:
it records current module bases, exception PC/registers/accurate-or-labelled-fuzzy
stack, separate AV/non-AV budgets, termination calls and guarded external Present
identity. Keep its 30-second bound and durable flush, arm after the post-apply
status sample immediately before the coordinator's owned CloseMainWindow, and
explicitly detach. Attaching during the initial 45-second wait would expire
before close. No new factory/upgrade trace is needed for this question. The
coordinator owns orchestration, the close action and backup restoration.

This triage changes documentation only; the worker performed no build, device or
game run, deployment, production fix or commit. `git diff --check` passed.


### Bounded prepare continuity receipt, 16 November 2026

For the next coordinator-owned run, the existing prepare-configure stage now
includes selected_source, window_native, selected screen, pass_flags,
window_flags, inputs, pass_reset, pass_no_fg, window_no_fg and continuous.
The existing source/native_frame fields identify the WINDOW source and selected
native frame. The boolean fields show each term of the existing continuity
predicate without changing that predicate. This uses the same existing 96-row
per-generation stage budget and requested-FG gate; no option or logger is added.
The stage output buffer expands from256 to512 bytes to retain the details;
maximum-width integer formatting occupies216/256 stage bytes and356/512 final
row bytes. Source whitespace checking and this formatting bound passed; no
build or game/device run was performed. Source is frozen for the coordinator's
focused rebuild and single combined eligibility/close-AV observation using the
adapter-prepared `.local/skyrim-research/trace-close-termination-20261116.js`.


### PID49592 admission triage, 16 November 2026

Coordinator run `input-acceptance-20261116-014431`, PID49592, completed the
observations but still tagged zero frames. The full controller receipt is
`.local/skyrim-research/enb-continuity-close-game-20261116.log`. Final fg_reason=3
is precisely NO_SUBMISSION, displayed as "Frame inputs or submission not matched";
it is not INVALID_INPUT (reason7). That reason precedes RESET in prepare's
priority order and can mask simultaneous NO_FG/RESET bits. No production repair
is justified solely by the changed display wording.

The initial stage receipts distinguish the continuity failures: source1074 and
1076 have flags23 (PRIMARY2 + AFTER_SIMULATION4 + NO_FG16 + RESET1); source1075
has flags6 and continuous=1; sources1077..1080 have flags22 (no RESET but NO_FG).
All listed WINDOW flags are6 and screen8 is GAMEPLAY. Thus persistent RESET is
not the explanation for all excluded frames, and at least one continuity-valid
frame was refused elsewhere. The zero-tag result cannot be attributed to focus
or a provider error from these receipts.

Producer tracing: cpu_events.cpp cpu_render_identity supplies source/submission/
scope/screen and initially sets AFTER_SIMULATION and NO_FG, clearing NO_FG only
when cpu_rendered_source_eligible reports an active render scope, exactly one
primary World entry and GAMEPLAY. presentation_inputs.cpp repeats that eligibility
check on the explicit FG_INPUTS lease. At FINAL_SCENE/WINDOW it propagates a later
native refusal from the same source/scope. Shared native_sr_capture_fg copies the
explicit lease, may add RESET for its own session/history/size/rect discontinuity,
and synchronously swaps that lease into native_fg_capture. The slot stores those
flags. Existing prepared same-identity inputs are retained idempotently rather
than recaptured; their flags can precede the later WINDOW facts. The stage data
alone does not identify which producer checkpoint supplied NO_FG. No adapter
predicate is relaxed and no runtime promotion of an excluded frame is introduced.

The precise final core rejection is the submitted_frame conjunction in prepare:
matching CPU-owner source, submitted=true, render_complete=true, pacing admission,
failed=false and matching provider generation. For this measured-input DLSS path,
pacing admission requires frame.paced; the FSR-only no-pacer exception does not
apply. Submission begin records submitted only for a matching, nonfailed CPU
owner; submission end records render_complete. Prospective admission separately
requires successful token acquisition, pacing, reservation adoption/bind and the
matching provider generation. Any of those facts may explain NO_SUBMISSION.
The controller does not include that source's CpuOwner snapshot, and bounded
CPU/prospective logs report early sources rather than1074..1080 or final1399.
Aggregate marker_order_violations and input-event counts do not identify which
per-source conjunction failed and must not be used as that proof.

Exact missing facts for a shared fix are one same-source prepare receipt of
CpuOwner id/session, submitted, render_complete, paced, prospective, failed and
provider_generation versus current generation, plus CPU read success/failed/
stage mask and reservation state. Existing debug FG eligibility can distinguish
submission versus CPU/identity/HUD-less checks, but cannot by itself split that
submission conjunction. Exact missing adapter facts are the FG_INPUTS original
flags and cpu_rendered_source_eligible operands (active scope, primary World
entry count and screen) for that same source, versus FINAL_SCENE/WINDOW flags.
These are a narrow scope request to the coordinator/adapter owner, not new
instrumentation or a new observation performed by this worker. No shared defect
has been proven by the current source/receipts, so there is no speculative code
change. Close-AV analysis remains with the separately assigned adapter worker.
Source review and `git diff --check` only; no build, game/device run or deployment.


### Complete typed continuation and CPU receipt, 16 November 2026

The coordinator/adapter's corrected PID49592 close-AV correlation identifies
AV RDI1e1e4c8bc50 as the application facade, matching loader line349, rather
than old physical1e2b4e04fe0 or new physical1e2b4f22600. The independent exact
installer audit identifies SetFullscreenState's singleton forwarding as another
wrong-receiver route. The absence of a Steam frame is consistent with that
handler's tail forwarding, not proof of a physical-chain lifetime failure.
The earlier unproven close-lifetime discussion is superseded by this identity
and method-specific evidence. Exact compatibility locations and patch ownership
remain in the adapter's continuation research; shared runtime has no module,
RVA or named integration policy.

The complete audited installer set is Release, Present, SetFullscreenState,
ResizeBuffers, Present1, SetColorSpace1 and ResizeBuffers1. The agreed ABI8
contract is one size/version checked rsf_d3d11_chain_dispatch tagged request in
presentation's d3d11_chain_dispatch.h, with method constants1..7, a typed native
argument union and signed HRESULT/unsigned reference-count result union.
The exported host dispatch validates the tag and header before classifying the
exact inner receiver; unowned arguments/results remain untouched. The earlier
Present service delegates to this contract. Native parameter/array pointers are
synchronously borrowed, with native buffer_count semantics. Host consumers use
the host version macro; game/frame/camera ABIs are unchanged.

All seven virtual entries now call distinct noinline owned bodies and retain a
volatile result store/load after the call. Present1 and ResizeBuffers1 owned
bodies call their private Present/ResizeBuffers siblings directly, avoiding
public sibling detours. Fullscreen/resize/colour bodies keep their prior native
semantics and results, including invalid arrays/dirty-region refusal. This is
receiver-correct forwarding, not a synthetic fullscreen no-op or hook bypass.
Release is special: after exact identity classification it bypasses admission,
takes no extra COM lease, consumes one caller reference and writes the unsigned
return without subsequent object access. Public Release has the same post-call
barrier. Prepublication create failure calls release_owned directly, since such
an object is not yet in the registry. Other methods retain a temporary live COM
lease and do not hold host/registry locks across callbacks/destruction.

The existing CPU diagnostic now includes all NO_SUBMISSION terms: expected
WINDOW source/session, owner id/session, submitted, render_complete, rendered,
paced, prospective, no_pacer, owner_failed, owner/provider generation and the
submission_ok result. It also carries CPU read/failed/stage mask/flags,
reservation read/state/abort/predecessor result/poll begin versus owner readiness,
reserved_bound and cpu_valid, plus the remaining valid conjunction booleans.
CPU/reservation snapshots are separately labelled read-only diagnostics, not
new admission inputs; they are fetched only after budget admission. To align
late-switch sources with adapter samples, the same96 maximum per generation is
partitioned into32 ordinary stage rows and64 prepare rows: first8 prepare calls,
then source%128==0. Maximum-width formatting is516/640 stage bytes and1214/1536
final bytes; source-level sampling/format bound checks and diff-check passed.

Focused coverage adds ABI layout/tag/size/version checks and unowned preservation
for all methods. The existing --scoped fixture checks all seven emitted virtual
entries for followed entry jumps; exact one-reference unadmitted Release; handled
refusal before admission; every canonical outer alias remaining unhandled without
reference mutation; and admitted fullscreen/colour operations plus Present1 and
both resize error semantics. Existing Present dispatch and outer pre/post totals
continue through later source/FG phases. The bridge fixture's final cleanup now
uses exact dispatch and checks that last Release returns zero and clears ownership.
These new source checks have not been built or run by this worker.

Source freeze requires coordinator ABI8 integrated rebuild/export/layout checks,
inspection of every emitted virtual entry and owned call boundary, the focused
--scoped fixture, then a bounded actual wrapper transition/close observation
using the adapter's complete method routing and producer samples. The final
Release check in the existing bridge fixture is also available for its normal
explicit device invocation. No worker build, game/device run, deployment or
commit was performed. Unrelated work remains preserved.


### Explicit FG provenance overwritten by SR, 16 November 2026

Coordinator integrated gate `verify-enb-chain-dispatch-fixed-20261116.log` passed
219 tests with15 skipped, formatting/Clippy clean. The --scoped fixture passed
51 sources/32 generated callbacks and the plain bridge fixture passed final
Release clearing ownership. The optional hidden --enable mode failed its active
generation assertion, which remains a limit. The seven rebuilt public call/return
boundaries were recorded in `abi8-emitted-boundaries-20261116.json`.

Actual `input-acceptance-20261116-020919`, PID20280, exited zero normally, with
no AV in REA trace `ev_a03626511fc9698905fa4ed90edaca1baded123631523b76ea6e1c17372503dd`;
the coordinator restored all four files. FG acceptance still failed: matching
sources1152/1280/1408/1536 show native FG_INPUTS and WINDOW flags0x6, active
scope, one World entry, eligible1, bound1 and refusal0. Core prepare has matching
source/session/generation, submission_ok1, reserved_bound1, cpu_valid1,
matched/hudless/identity/scene/vsync1, but retained pass_flags0x16 and continuous0.
These receipts supersede the earlier missing-fact gap and isolate the added
NO_FG to shared slot metadata rather than the producer or CPU admission.

The exact writer is rsf_native_fg_evaluate's successful tail assignment
slot.pass=scene. Explicit FG normalization first retained the eligible FG_INPUTS
lease; subsequent SR evaluation used its conservative SR metadata (which always
carries NO_FG in this adapter) and replaced that slot's admission metadata. The
initial bound_source merge hypothesis is disproven for this path: native_fg_scene
returns before that merge when the pass already supplies source_frame_id. No
change to bound_source or engine producer conventions is justified.

The narrow correction preserves an already prepared, same-identity FG_INPUTS
pass through subsequent SR metadata updates, using the existing holds identity
check and existing pass role. SR-only/fallback paths retain their previous
metadata behavior. A repeated actual FG_INPUTS lease can add RESET/NO_FG refusal
bits but cannot clear an earlier refusal. An explicit FG_INPUTS lease arriving
after a fallback SR capture performs its normal checked resource copy, replacing
the fallback contents/provenance rather than inheriting its conservative flags.
Final WINDOW NO_FG still independently rejects generation; resource, identity,
CPU/pacing, scene and vendor admission checks are unchanged. No new setting,
ABI, provenance framework or adapter-specific runtime branch is introduced.

The existing focused device fixture now gives only its SR callback conservative
NO_FG metadata while retaining eligible explicit FG_INPUTS and WINDOW leases.
Its existing SR-On tag/callback assertions therefore cover the measured overwrite
failure. A new negative source supplies a later genuine explicit NO_FG lease,
then a duplicate eligible lease and eligible WINDOW, requiring unchanged tag
count and refusal; this prevents clearing a real producer refusal while fixing
SR metadata contamination. These additions are source coverage only until the
coordinator rebuild/rerun. `git diff --check` passes; no worker build, device/game
run, deployment or commit occurred. Source is frozen for focused checks and the
next bounded actual acceptance, with the prior normal exit kept distinct from
unproven FG success.


### Provenance acceptance and inactive provider limit, 16 November 2026

Coordinator focused rebuild and --scoped regression
`explicit-fg-provenance-device-20261116.log` passed:53 sources,39 captures,
37 SR evaluations and33 tags/generated callbacks. Actual run PID52400,
`input-acceptance-20261116-021712`, used Runtime SHA256
`1b7ce92a78b863ad7d37112585d7ef9e0118e851809884ea8fd4eb242820c15b`,
exited zero and restored all four files. Matching producer/core samples1152,
1280,1408 and1536 now retain pass_flags6/WINDOW6, continuous1 and every admission
conjunction true. The formerly added shared NO_FG is absent. The actual final
sample has317 successful tags. This verifies the metadata repair and normal
exit on that owned run; it does not prove actual DLSS-G output or visual quality.

`explicit-fg-provenance-game-20261116.log` samples source1458 with reason3,
active0/effective0, vendor_status0, provider outputs327, foreground0 and base/
output54/54 fps. The reason describes that sampled source, not all317 earlier
tagged frames. The loader independently contains27 provider mode/activity change
rows:14 mode-On rows, no active1 rows and no vendor error status. Off/On pairs
usually differ by one reported output (for example316/317 and327/328), consistent
with a refused source temporarily disabling generation and the following eligible
source enabling it again. A concrete initial refusal, source1134, has submitted0,
paced0, failed1 and owner_generation0 versus provider_generation2 despite complete
native CPU facts. Source1135 onward and later sampled sources pass. Later isolated
Off events often coincide with one SR-refused frame followed by SR recovery, but
the log does not identify every per-source failure. No policy should promote
those excluded frames merely to keep a provider enabled.

Source review separates reporting from activity: prepare configures Off when the
full source admission fails, configure resets provider temporal history on a
mode change, and the next eligible prepare enables/tag inputs. DLSS poll runs
after physical Present; it marks active only when preparation was enabled, SDK
status is eOk and numFramesActuallyPresented is greater than1. The shipped SDK
header documents that counter as frames presented since the previous GetState
call. Cumulative outputs and the output/base rate therefore remain separate
measurements from requested mode or tag count. Frontend status also masks effective
activity for the current pending refusal; a one-shot inactive snapshot alone
would be insufficient. Here the separate mode-On logs never observe activity
and the reported output rate remains1x, so the remaining limitation is not
explained solely by the final invalid-source sample.

Neither the core prepare/configure path nor the reviewed public SDK guide
establishes a foreground admission requirement. The current run nevertheless
never supplies a verified foreground1 acceptance interval. Its tags, successful
SDK calls and vendor_status0 do not prove that the driver generated/presented
interpolated frames. The smallest next discriminating observation is a bounded
interval with verified foreground1, consecutive eligible/tagged sources and
mode-On, comparing the existing SDK-derived output count/rate and active state.
This uses existing status/receipts and changes no settings or framework beyond
the already requested FG/Reflex selection. If it remains inactive in that
condition, the next exact missing vendor fact is the raw post-Present SDK
numFramesActuallyPresented for those same enabled sources, together with SDK
status and readiness of its existing input-processing completion fence; that
can distinguish absent output from asynchronous sampling without guessing an
SDK focus rule. No background rule or unconditional provider-On fallback is
introduced from the present evidence.

No further concrete shared repair is established by this review. The worker
changed this research note only, with `git diff --check` passing and no build,
device/game run, deployment or commit. Source remains frozen for coordinator
selection of the bounded foreground/output acceptance condition.
