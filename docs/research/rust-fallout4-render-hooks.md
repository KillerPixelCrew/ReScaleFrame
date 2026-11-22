# Fallout 4 World and Present admission in Rust

This source port starts at `1a87cbca` and uses the existing Fallout 4 AE 1.11.240.0 evidence for
executable SHA-256 `fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`.
The [native callback analysis](fallout4-render-integration-20261116.md) identifies the ordinary
WorldBegin parent and outer Present owner. The [reduced recovery analysis](fallout4-native-reduced-recovery-20261116.md)
explains why pre-Present diagnostic capture can proceed without temporal or FG admission.

| Boundary | Admission and ownership |
| --- | --- |
| Before original WorldBegin | Rust requires return `0xc382a5`, a live adapter, the current host session's nonzero source without a submission, and no nested World guard. Open FRAME, auxiliaries, a recovery token or the resolution owner's pending work block admission before the CPU owner may bind a submission. C++ keeps the ordinary World guard on the hook stack and opens FRAME then SUBMISSION only after its double native owner and COM checks. |
| Before original outer Present | Rust requires return `0xc335ec`, renderer `0x3a25d00` relative to the image, and an open SUBMISSION. The C++ reader verifies the active renderer-window cell at `0x38e13b0` against `RendererData+0x58` before reading its chain at `+0x18`. Rust matches the chain from that cell and the frame, view, window, context and chain from the native owner against the FRAME lease. |
| Matched Present | C++ can capture reduced recovery output while the typed native framebuffer lease is live. Rust then requires the current render source and asks the existing screen facts path to revalidate any leased input. A sticky refusal withholds SUBMISSION completion. Success closes SUBMISSION before beginning the CPU Present phase and calling the original. The phase end uses the identity copied at phase begin on the C++ stack. |
| After Swap | The existing cleanup balances any refused SUBMISSION, ends WINDOW and FRAME, retires recovery and auxiliaries, and restores resolution state. A pending obligation retains its owner and blocks the next World admission. |

The boundary carries copied scalar identities and returns admission decisions. C++ keeps the typed
native trampolines, `LastError` restoration, COM references, callbacks and capture. The GameSDK ABI
and native guard bytes are preserved. Prepass and depth capture, FG input publication before SR,
reduced FINISH/RETIRE, logical framebuffer handoff and native TAA remain in their existing order.

Source comparison checked the original hook calls, exact caller and receiver constants, owner
predicates, capture-before-admission order and retained cleanup against `1a87cbca`. Pinned Rust
1.94.1 formatting, locked offline Cargo metadata and whitespace checks passed. Build, link,
Clippy, tests, deployment and device/game checks remain with the user. Matched capture through
Present, moving shader/buffer coverage, reduced recovery and TAA history across quality changes,
and stable FG/latency still require runtime evidence. See the [shader and buffer audit](fallout4-full-shader-buffer-audit-20261116.md).

## Hook registration, admission and retirement in Rust

This source port starts at `17904281`. It moves the render hook lifecycle out of
`render_hooks.cpp` without new research: every RVA, expected byte and call pair is the value the
C++ owner already checked, with the evidence in the [native callback analysis](fallout4-render-integration-20261116.md),
the [depth-domain notes](fallout4-depth-domains-20261116.md), the [reduced consumer notes](fallout4-reduced-consumers-20261116.md)
and the [Address Library cross-check](fallout4-address-library-20261116.md).

| Owner | Rust (`render_hooks.rs`) | C++ (`render_hooks.cpp`) |
| --- | --- | --- |
| Hook records | The eight RVAs and expected entry bytes in the original creation and enable order, and each record's created state. | Each typed detour and its typed MinHook trampoline, which the detour calls on its own C++ frame. `remove_hook` clears that trampoline only after a successful `MH_RemoveHook`. |
| Verify | Entry bytes, then the resolution, screen-query and depth-domain guards, TAA vtable `0x2915eb0` slot 8, nine producer calls, seven outer Present instructions and eight World/imagespace calls, in the original short-circuit order. Session and prepare add the render-input guards after them. | None. |
| Prepare | Refuses a live adapter, any busy scope, missing `process_upscale` or `render_pass`, or a guard mismatch. Resolves the five Runtime exports by name, binds them to the callbacks even when the negotiation then refuses, and requires both auxiliary exports and `render_config`. Creates missing hooks in order. A failure removes this call's hooks in reverse; a refused removal keeps its record and logs the original rollback text. Host services bind only after every hook exists. | The bound exports and services the callbacks read. |
| Start | Rechecks busy and verify, queues every created hook, sets live only if all queued, then applies. A missing record or refused queue or apply clears live, queues every created hook disabled and applies again. The enable receipt is unchanged. | `MH_QueueEnableHook`, `MH_QueueDisableHook`, `MH_ApplyQueued`. |
| Quiesce, stop | Quiesce clears live (release). Stop quiesces, refuses while busy, disables every created hook through the shared disable leaf (`MH_OK` or `MH_ERROR_DISABLED`) without short-circuiting, then requires not busy. Trampolines stay for a later start. | None. |
| Busy and live | Busy is the C++ scope counters, then the resolution owner. Live is a Rust atomic read with acquire by every callback through `rsf_fallout4_render_live`. | Detour-in-flight, WINDOW, FRAME, auxiliary and recovery counters, unchanged. |

The six `RSF_FALLOUT4_*_RENDER` subsystem operations and their `subsystem` leaf are retired from
`rsf_fallout4_platform`; `platform_abi.rs` was edited to match the bindgen layout for the shorter
table. The C++ subsystem wrapped these calls in a catch that answered `RSF_ERROR_NOT_READY` on a
throw. None of the moved calls can throw: MinHook, `GetProcAddress`, the guarded reads and the
noexcept leaves return codes, and the host log keeps its guarded leaf. The GameSDK ABI, the
Runtime export names, the refusal and receipt texts and the engine policy boundary are unchanged.
Recovery reservation, SEAL/FINISH/RETIRE, auxiliary retirement, FRAME/SUBMISSION/WINDOW scope and
every producer callback body stay in C++ for a separate slice.

A scripted comparison against `17904281` matched all eight hook records, seven instruction guards,
seventeen call pairs in order and every refusal and receipt text. Pinned Rust 1.94.1 formatting,
locked offline Cargo metadata and whitespace checks passed. Build, link, Clippy, tests, the shared
host fixture, deployment and game checks remain with the user; this is a source port, not a
runtime result.

## Reduced-render recovery transaction in Rust

This source port starts at `c163aead` and moves the reduced-render recovery transaction from
`render_hooks.cpp` into `render_hooks/recovery.rs` without new research. The transaction, its
native seams and the Runtime contract are the ones in the [reduced recovery analysis](fallout4-native-reduced-recovery-20261116.md);
the reduced-frame ABI is the existing `rsf_d3d11_host_reduced_frame` (ABI 1, 56-byte request) in
`d3d11_host.h`. No option, fallback branch, graphics measurement or Runtime change is added, and
the domain facts the adapter selects (the five raster resources, their kinds, the VATS restore flag
and the output-owner association) stay in the adapter.

| Owner | Rust (`render_hooks/recovery.rs`) | C++ (`render_hooks.cpp`) |
| --- | --- | --- |
| Token and verdict | The per-thread token, `committed`, `sealed`, `finished`, every RESERVE/SEAL/FINISH/RETIRE/CANCEL request and the verdict from its result, disposition and flags. FINISH sets the World full-output and temporal flags through `resolve` and refuses the source unless temporal output was published. | `World::full_output_resolved`, `published` and `refused`, which the TAA gate, SR publication and receipts keep reading. |
| Surface plan | Five surfaces in lease order (depth/stencil, then motion, Pip-Boy, VATS, refraction as colour), integer render rect as source, output rect, zero operator offsets, the original names, VATS with `RESTORE_CONTENTS`. | The texture pointers, from the held lease. `reconstruct` still requires all five when a token is owned. |
| Identity | The `NativeOutputOwner` recorded at reservation and the exact comparisons at confirmation, SEAL and FINISH. | Typed raster reads, `same_object` COM identity of colour, depth, motion, Pip-Boy, VATS and refraction (plus Context and chain at SEAL), and the full-output preflight call. |
| Leases | When to adopt the candidate, when to discard it (a drop guard at the end of every step, as the C++ stack lease ended) and when to release the held lease. | `recovery_inputs` (held) and `recovery_candidate` COM leases on the World. |
| Runtime effect | The request contents and when to call. | The `reduce` leaf: the held pass as owner, the held Context, `try`/`catch` answering `RSF_ERROR_NOT_READY`. |
| Receipts and retention | Once-per-stage refusal texts (eight stages), the 32-row FINISH receipt budget, and the process-wide owned-token count that `busy()` now reads beside the C++ scope counters. | Native detour frames, `LastError`, the World reset points, which also reset the Rust state. |

The C++ World fields `recovery_flags` and `recovery_disposition` were written and never read, so
they are not carried over; each verdict reads the request it just made. Admission, prepass,
imagespace, Effects, TAA receipts, Present capture and the post-Swap reset read the token or
`committed` through `rsf_fallout4_recovery_token` and `rsf_fallout4_recovery_committed`.

### Traces

- **Refusal.** Stage 0 (no live adapter, no export, an owned token, no FRAME or a changed source)
  and stage 1 (raster lease or full-output preflight missing) happen before any token and do not
  refuse the World. The resolution owner then makes no ratio commitment and native ratios stay.
  Stage 2 (RESERVE refused, or source, output owner or any of the six resources changed after
  RESERVE) cancels a returned token. A CANCEL that allows the next reduction drops the token and
  the held lease, so the refusal leaves the World unrefused; a refused CANCEL keeps both, refuses
  the World and blocks the next World admission. Stages 3 to 7 refuse the World and retain the
  token.
- **Partial ratio write.** `committed` is set before `rsf_fallout4_resolution_configure` writes
  any ratio. When the write fails part way, configure returns false, the World is marked
  raster-refused while the resolution owner is busy or refused, and the committed token stays
  owned. The prepass and Effects seams stay selected while a token is owned, even after quiesce,
  so the SR seam SEALs the produced originals and its scope guard FINISHes the spatial recovery
  even though SR is refused.
- **Retry.** RETIRE is attempted at the original Effects return, at the selected imagespace exit
  and after Swap. A FINISH that did not define full output leaves `finished` false and is retried
  at the next point while the producer snapshot and output owner still match. Once `finished`
  is true, a later FINISH answers the World's `full_output_resolved`. The imagespace exit clears
  that flag after its own retirement attempt. In the C++ source, and in `8ca09fef`, retirement went
  through that answer, so a RETIRE refused twice inside imagespace was not sent again after Swap
  and the token stayed owned, contrary to the analysis note's statement that Swap cleanup retries
  retirement. The follow-up below fixes this.
- **Post-Swap retention.** `finish_render_scope` closes SUBMISSION, WINDOW and FRAME, then
  retires recovery, drains auxiliaries and finishes the resolution source. The World and the Rust
  state reset only when no resolution, auxiliary or token obligation remains. An owned token
  keeps `busy()` true, so stop refuses, and the World admission refuses `recovery_owned`. A
  committed token whose frame never reaches the SR seam is never sealed; post-Swap FINISH then
  refuses at stage 6 and the token stays owned for the process, as before.

The `reduce` leaf answers without calling the Runtime when the export or the held Context is
missing. RESERVE in the C++ called the Runtime unconditionally after stage 0; a held lease always
has a Context there, because the raster reader requires a nonzero immediate context.

A scripted comparison against `c163aead` matched all eight refusal texts by stage, the FINISH
receipt format and the five surface names. Pinned Rust 1.94.1 formatting, locked offline Cargo
metadata and whitespace checks passed. Build, link, Clippy, tests, the shared host fixture,
deployment and game checks remain with the user; this is a source port, not a runtime result.
Open game checks: a reduced quality with temporal SR published, a spatial FINISH after temporal
refusal, a forced ratio-write failure, a RETIRE refusal inside imagespace, quiesce with an owned
token, and VATS/Pip-Boy contents after RETIRE.

### Retirement after the imagespace exit

Two answers that the port above shared now stay distinct. `finished` is the token's own record
that its one FINISH resolved full output; the World's `full_output_resolved` is the per-imagespace
flag that the TAA gate and receipts read, and the imagespace exit clears it. Standalone FINISH,
which the SR seam's scope guard calls, still answers the World flag for a finished token. RETIRE
now reads `finished`: when it is set, RETIRE goes to the host on the same token, surfaces and held
lease without a second FINISH or any output publication. When it is not set, the guarded FINISH
admission (seal, current producer snapshot, same output owner, output scope) runs first, as
before. RETIRE still drops the token and lease only on `RSF_OK` with `NEXT_REDUCTION_ALLOWED`; any
other answer retains both and keeps the World refused for the source.

Call order against `8ca09fef`, for a token that FINISH resolved at the SR seam:

| Seam | Before | After |
| --- | --- | --- |
| Original Effects return | RETIRE (World flag still set). | RETIRE. |
| Selected imagespace exit | RETIRE if still owned, then auxiliaries, output scope end, flag cleared. | Unchanged. |
| After Swap | FINISH answers the cleared flag; no RETIRE, token owned for the process. | RETIRE if still owned, then auxiliaries and the resolution source. |

A token whose FINISH did not resolve full output follows the same path as before at every seam.
The Runtime contract requires only the same owner for RETIRE, including while suspended. The
post-Swap RETIRE runs after the native consumers and before the World can admit the next producer.

`render_hooks/recovery/tests.rs` adds a focused, unrun regression with fake leaves: a finished
token retires after the World flag was cleared without another FINISH, standalone FINISH still
answers the cleared flag, a refused or withholding RETIRE retains the token and the retry sends
only RETIRE, and an unfinished token still stops at the guarded FINISH. Source review compared the
three call sites in `render_hooks.cpp`; pinned Rust 1.94.1 formatting and whitespace checks
passed. Build, tests and game checks remain with the user; the open check is a forced RETIRE
refusal inside imagespace that drains after Swap, with VATS contents restored correctly.

## SR and HUDless publication in Rust

This source port starts at `8570bec4`, which includes the recovery transaction and the RETIRE
retry above, and moves the SR seam (`emit_sr`, called before the original imagespace Effects
range) and the HUDless seam (`emit_hudless`, called after the native HUDless viewport selection)
from `render_hooks.cpp` into `render_hooks/publication.rs` without new research. The seams, their
producers and the screen facts are the ones in the [native callback analysis](fallout4-render-integration-20261116.md),
the [depth-domain notes](fallout4-depth-domains-20261116.md) and the [reduced output scope notes](fallout4-native-reduced-output-scope-20261116.md).
No option, GPU work, shader, profile, address, Runtime export or GameSDK field changes.

| Owner | Rust (`render_hooks/publication.rs`) | C++ (`render_hooks.cpp`) |
| --- | --- | --- |
| SR admission | SEAL before any reader and FINISH when the step returns (after the input lease is released); live, imagespace, source, no published inputs, no raster refusal, no open auxiliaries, bound `process_upscale`; held-recovery resource match; producer LTRB/camera/output extents (`rsf_fallout4_resolution_inputs`); FRAME open with the same native frame, view, window, raw Context and chain; same-source depth snapshot. Every World refusal and its diagnostic text. | The typed producer read on the depth snapshot, the six-resource COM identity against the held recovery lease, the depth-domain composition. |
| Pass identity | Session, source, submission and scope, primary/after-simulation/NO_FG flags, unknown screen, SR role and pass key `0x21fa283`. The FRAME pass opened at WorldBegin uses the same Rust function. | None. |
| SR-off and planning | The RECONFIGURE planning copy, the `render_config` verdict (SR off when disabled at the same output size) and its exact place after planning and before the screen gate. | The guarded Runtime calls. A planning throw answers refusal as before; a `render_config` throw, which the C++ owner let reach the Effects detour's catch, answers `failed`, and the detour logs the same callback-failure receipt. |
| Screen verdict | Unrefused gameplay, current source, unchanged leased screen and camera state, FRAME and SUBMISSION open, else World refusal; `pass.screen` and NO_FG from the World refusal; the eight-row screen receipt. The outer Present revalidation calls the same verdict on the World pass. | `classify_native_screen` against the screen basis, which calls the native menu and camera queries; the leased World screen. |
| Callback order | FG_INPUTS begin and end on the input Context, source recheck, scalar World identity with every borrowed resource pointer cleared, then the SR-off return, output-ready receipt, evaluation, evaluated receipt, output scope end on failure without a token, publication flags. HUDless: framebuffer read, five identity and extent stages with once-per-stage receipts, screen verdict, FINAL_SCENE begin and end, WINDOW begin. | `host.render_pass` with its catch, `reconstruct` with its auxiliary policy and catch, `begin_output_resolution`, `end_output_resolution`, `report_resolution_state`. |
| Leases | When to read and release the SR input and framebuffer leases, and which resources the World retains. | `World::sr_inputs` and `World::hudless`, the colour, depth, motion and final-colour retention, World Context and chain, the WINDOW counter. |

The C++ `rsf_fallout4_recovery_seal` and `rsf_fallout4_recovery_finish` exports had no other
caller and are removed; the SR step calls the Rust functions directly. Publication shares the
recovery leaves' report, refusal, source and resolve leaves and adds `NativePublicationLeaves`
(22 entries, size-asserted on both sides).

### Refusal and retirement traces

Source comparison walked every return of the original `emit_sr` and `emit_hudless`.

- **SR refusals without World refusal**: admission, producer read, held-resource mismatch,
  producer extents (diagnostic), planning throw, screen gate after the verdict already refused,
  SR off. **With World refusal**: SEAL refused, FRAME lease mismatch and depth mapping
  (diagnostics), source change after FG_INPUTS, output scope refused (NOT_READY receipt first),
  failed evaluation without a token whose output scope end is refused. Each keeps its original
  order of refusal, receipt and return.
- **FINISH and retirement**: the FINISH guard runs on every return, after the input lease is
  released, as the C++ scope guard ran after the stack lease. A failed evaluation with a token
  keeps the output scope for FINISH. RETIRE at the Effects return, the imagespace exit and after
  Swap is unchanged C++ calling the Rust transaction.
- **Published SR**: World `published` set; `full_output_resolved` set only without a token, and
  with a token FINISH then decides both flags, as before.
- **HUDless**: stages 0 to 5 report once each and never refuse the World; a later screen change
  refuses the World and the FINAL_SCENE and WINDOW passes carry NO_FG.

### Differences

- The SR input and HUDless framebuffer leases are World fields instead of stack locals, released
  by drop guards where the stack leases ended. A World reset during either step would end them
  early; no reentrant path reaches one.
- `Fallout4 HUDless callback failed; native drawing retained.` is gone. Every call in the
  original `emit_hudless` was noexcept or guarded, so its catch was unreachable. The SR catch is
  kept only for the one call that could reach it.
- The framebuffer extent leaf skips `GetDesc` for a null texture and then refuses at stage 5. A
  successful framebuffer read always returns a texture, so the original never reached that case.
- The HUDless receipt formats pointers as sixteen uppercase hex digits, which is MSVC `%p`.

A scripted comparison against `8570bec4` found every SR, HUDless and screen receipt text and all
six HUDless stage names in the new sources, except the unreachable receipt above.
`render_hooks/publication/tests.rs` adds unrun fake-leaf checks of the published SR order, SR
off, a `render_config` throw, the HUDless FINAL_SCENE and WINDOW order and a foreign framebuffer.
Pinned Rust 1.94.1 formatting, locked offline Cargo metadata and whitespace checks passed. Build,
link, Clippy, tests, the shared host fixture, deployment and game checks remain with the user;
this is a source port, not a runtime result. Open game checks: SR published at native and reduced
quality, SR off with FG inputs, a HUDless refusal stage, and Present revalidation after a screen
change.

## FRAME, SUBMISSION and WINDOW scope in Rust

This source port starts at `0c6db204`, which includes the SR and HUDless publication above and
the RETIRE retry, and moves `begin_render_scope`, `close_submission` and `finish_render_scope`
from `render_hooks.cpp` into `render_hooks/scope.rs` without new research. The owner cells, the
WorldBegin pass key `0x21f0ac0` and the Present and Swap seams are the ones in the [native callback analysis](fallout4-render-integration-20261116.md);
the post-Swap retirement order is the one in the [reduced recovery analysis](fallout4-native-reduced-recovery-20261116.md).
No option, GPU work, address, Runtime export, GameSDK field or vendor and producer policy changes.

| Owner | Rust (`render_hooks/scope.rs`) | C++ (`render_hooks.cpp`) |
| --- | --- | --- |
| Lease admission | Owner read, COM lease, second owner read compared whole (renderer, device, Context, chain, window, graph, view, frame), current source, then adoption; any failure refuses the World with the original diagnostic. | The typed owner read with the accepted window, the immediate Context, device and chain-device COM identity and output window checks, and the candidate Context and chain leases that adoption moves into the World. |
| FRAME identity | Struct size, the shared pass identity, FRAME role, native frame, view keys, window keys, pass key and swapchain on the World FRAME pass; the SUBMISSION copy with its role. | None. |
| Begin order | FRAME open, FRAME begin callback, SUBMISSION open, SUBMISSION begin callback, the once-per-process announcement. | The open flags and the FRAME counter. |
| Balanced close | An open SUBMISSION ends once, with NO_FG when the World is refused, at the admitted complete outer Present or after Swap. | The Present match, admission dispatch, CPU Present phase and its announcement are unchanged. |
| After Swap | No source: nothing. Changed source refuses. A withheld SUBMISSION refuses, reports and closes. World pass NO_FG on refusal, WINDOW end, then FRAME end with NO_FG on refusal. RETIRE through the recovery transaction, auxiliary drain, resolution source finish (refusal on failure), the `source_retired` receipt on the input or FRAME pass, and the World and recovery reset only when no resolution, auxiliary or token obligation remains. | The guarded auxiliary restore, `finish_resolution_source`, the WINDOW and FRAME counters and `World = {}`. |

`NativeScopeLeaves` has 16 entries, size-asserted on both sides. Its notify, diagnostic and
report_state entries bind the same C++ functions as the publication leaves, so the pass callback
guard, the shared 16-row diagnostic budget and the resolution receipt are unchanged; the scope
table carries them so it does not depend on publication internals. The scope reuses the recovery
leaves' report, refuse and same_source and calls the Rust RETIRE directly. `NativeRenderOwner`
replaces the C++ `Owner` (64 bytes, frame at 56) and `NativeScopeWorld` is 80 bytes. The
`rsf_fallout4_render_identify` export lost its only caller and is removed. The CPU owner still
calls `finish_render_scope` through `rsf_fallout4_cpu_platform`, which is unchanged.

### Differences

- The device and chain-device references are released when the COM lease leaf returns, before
  the second owner read, instead of at the end of the begin step. The Context and chain leases
  are still held across that read, as a World candidate discarded by a drop guard.
- The FRAME pass is composed on a copy and written back before any callback. The World was just
  reset and nothing reads it in between, and both callbacks still receive the World fields.
- The COM lease leaf catches C++ exceptions and returns a refusal, so the candidate leases are
  discarded and the standard owner refusal is logged. Pass callbacks, logs and the Runtime
  auxiliary call retain their native exception handling.

### Traces

Source comparison walked every return of the three original functions. A refused owner (first
read, COM lease, second read, comparison or source) opens nothing and refuses the World before its
diagnostic, as before. A completed Present closes SUBMISSION before the CPU Present phase. The
post-Swap end keeps the original sequence of refusal, diagnostic, callbacks, counters, RETIRE,
drain, resolution finish, receipt and conditional reset; RETIRE still goes through the retry that
uses the token's own `finished` record.

A scripted comparison against `0c6db204` found all sixteen receipt texts of the original
`render_hooks.cpp` in the new sources. `render_hooks/scope/tests.rs` adds unrun fake-leaf checks
of the begin order and FRAME identity, a changed owner and a refused COM lease, a single refused
SUBMISSION close, the post-Swap WINDOW then FRAME order with NO_FG and reset, retention with open
auxiliaries, and the no-source return. Pinned Rust 1.94.1 formatting, locked offline Cargo
metadata and whitespace checks passed. Build, link, Clippy, tests, the shared host fixture,
deployment and game checks remain with the user; this is a source port, not a runtime result.
Open game checks: FRAME and SUBMISSION begin before WorldBegin with the expected keys, SUBMISSION
completion before Present, a refused or unpresented SUBMISSION balanced after Swap with NO_FG on
WINDOW and FRAME, an owner change across the lease, and retention while an auxiliary, token or
resolution obligation remains.

## Imagespace, Effects and TAA seams in Rust

This source port starts at `fab9fe5a`, which includes the scope port above and the COM lease
catch, and moves the remaining first-party choices of `observe_imagespace`, `observe_effects` and
`observe_taa` from `render_hooks.cpp` into `render_hooks/imagespace.rs` without new research. The
three imagespace calls, the Effects call inside imagespace, the native imagespace manager cell and
the TAA vtable slot are the ones in the [native callback analysis](fallout4-render-integration-20261116.md)
and are still checked by `verify` (`WORLD_CALLS` and the TAA slot); the exit and Effects-return
retirement order is the one in the [reduced recovery analysis](fallout4-native-reduced-recovery-20261116.md),
the `ui_return` and `ui_restored` receipts are the ones in the [reduced output scope notes](fallout4-native-reduced-output-scope-20261116.md),
and the TAA exclusion is the callback analysis rule with the bounded receipt from the [SR dropout notes](fallout4-sr-dropout-20261116.md).
No option, GPU work, shader, address, Runtime export, GameSDK field or vendor and producer policy
changes, and nothing visual is meant to change.

| Owner | Rust (`render_hooks/imagespace.rs`) | C++ (`render_hooks.cpp`) |
| --- | --- | --- |
| Imagespace entry | Selection in the original short-circuit order: not already inside imagespace, FRAME open, live on the current source or a recovery token owned, then one of the returns `0xc383c7`, `0xc39d6f`, `0xc39e13`. A selected call sets the World inside imagespace. | The return address and LastError save, the original call after the LastError restore. |
| Imagespace exit | RETIRE (with its diagnostic), auxiliary drain (with its diagnostic), `ui_return` receipt, output resolution end (refusal on failure), `ui_restored` receipt, refusal and diagnostic while output state stays owned, then the World leaves imagespace and clears `published` and `full_output_resolved`. | The scope guard whose destructor calls the exit when the original returns or unwinds, inside its own LastError save and restore; the guarded drain and `end_output_resolution`. |
| Before Effects | Live or token owned, return `0x21fa288` and arguments `(0, 13, 1, 1)`, then the native manager read compared with the argument; the `pre_sr` receipt on the FRAME pass with `NOT_READY`, the SR seam (`publication.rs`) and the callback-failure diagnostic when it answers failed. | The typed read of the manager cell `0x3e5e4b0`, the LastError save and restore around the original. |
| After Effects | The same return and arguments, without the live or manager check: the `effects_return` receipt, RETIRE when a token is owned (with its diagnostic), then the drain when auxiliaries are open (with its diagnostic). | The native LastError captured after the original and restored after the Rust step. |
| TAA | The original answer is excluded while the World is inside imagespace with full output resolved and the raster snapshot is current (or, without a valid snapshot, the source is current). One receipt per observed combination of original, imagespace, full output, temporal and exclusion bits, with the original text. | The original query first, then the LastError save and restore around the Rust gate. |

`NativeImagespaceLeaves` has 8 entries: the World read (72 bytes), enter, leave, the manager read,
and the same drain, `end_output_resolution`, resolution receipt and shared diagnostic functions the
scope and publication tables bind. `NativeEffectsCall` (32 bytes) carries the return address and
arguments. Both are size-asserted on both sides. Rust uses the recovery leaves' report, refuse,
same_source and raster_current, calls RETIRE and the SR seam directly, and reads
`rsf_fallout4_resolution_output_busy` directly, as `output_resolution_busy` did. The
`rsf_fallout4_render_emit_sr` and `rsf_fallout4_recovery_retire` exports, the C++
`retire_recovery` wrapper and `NativeSrOutcome` lost their only callers and are removed;
`SrOutcome` is now a Rust-only enum.

### Differences

- The World enters imagespace inside the Rust selection step, before the C++ scope guard is
  constructed instead of just after it. Nothing between them can throw: the Rust export cannot
  unwind and every leaf is noexcept.
- Every new leaf is noexcept, so no C++ exception can unwind into a Rust frame. The manager read
  leaf calls only `ReadProcessMemory`, like the owner read leaf, so it has no `catch`: under
  `/EHsc` that handler would be provably unreachable. The other new leaves set World flags or
  call functions that are already noexcept and guarded; host callbacks, logs, Runtime calls and
  the COM lease keep their own catches.
- The TAA receipt is built by `format!` instead of a 400-byte `snprintf` buffer. The longest
  possible text is under 300 bytes, so the original never truncated it.
- The TAA receipt bits are one process-wide atomic, as the function-local static was.

### Traces

Source comparison walked every path of the three original functions. Imagespace: an unselected
call (wrong caller, already inside, FRAME closed, not live without a token, or a changed source
without a token) runs only the original; a selected call keeps the exit order above on return and
on unwind, and the LastError the engine left is restored after the exit. Effects: the SR seam
still runs only for the live or token-owning call whose manager matches the native cell, after the
`pre_sr` receipt; the after step still runs for the exact call even when the before step was not
selected, and keeps receipt, RETIRE, drain order. TAA: the original runs first, the predicate reads
the same World fields and leaves, and the answer is the original and not excluded.

A scripted comparison against `fab9fe5a` found all thirteen receipt texts of the original
`render_hooks.cpp` in the new sources, with format specifiers normalized.
`render_hooks/imagespace/tests.rs` adds unrun fake-leaf checks of the three imagespace callers and
a single entry, a closed FRAME and a changed source, the exit order on the input and FRAME passes
with an output-end refusal, the Effects manager and argument selection with SR emission and its
failure diagnostic, the Effects-return receipt and drain, and the TAA exclusion by source and by
raster snapshot with one receipt per combination. Pinned Rust 1.94.1 formatting, locked offline
Cargo metadata and whitespace checks passed. Build, link, Clippy, tests, the shared host fixture,
deployment and game checks remain with the user; this is a source port, not a runtime result.
Open game checks: SR published at native and reduced quality with native TAA excluded only inside
the resolved imagespace, the `ui_return` and `ui_restored` receipts each frame, a forced RETIRE
refusal at the Effects return that drains at the imagespace exit or after Swap, VATS and Pip-Boy
contents after the Effects-return drain, and an imagespace call from an unselected caller.

## Viewport, dynamic-resolution and prepass seams in Rust

This source port starts at `59783098`, which merges `c1e16179` (the imagespace port above), and
moves the first-party choices of `observe_viewport`, `observe_dynamic_resolution` and
`observe_prepass` from `render_hooks.cpp` into `render_hooks/producer.rs` without new research.
The viewport selector store `0x1839ec0`, the HUDless selection return `0x21fa2ca`, the DR writer
return `0x21f9f34` and the prepass return `0x21f9f78` are the ones in the [native callback analysis](fallout4-render-integration-20261116.md)
and are still checked by `verify` (`WORLD_CALLS` and `PRODUCER_CALLS`). The native targets
`0x2f4d750` (the DR and viewport manager argument) and the configuration after the DR writer are
the ones in the [reduced output scope notes](fallout4-native-reduced-output-scope-20261116.md); the
prepass capture after `0x21f9f78` within the same-source render lease is the one in the
[depth-domain notes](fallout4-depth-domains-20261116.md). No option, GPU work, shader, address,
Runtime export, GameSDK field or vendor and producer policy changes, and nothing visual is meant to
change.

| Owner | Rust (`render_hooks/producer.rs`) | C++ (`render_hooks.cpp`) |
| --- | --- | --- |
| Viewport | After the original store: on the native targets the owned full-output selector follows the native intent (`rsf_fallout4_resolution_native_viewport`), and a failed write refuses the World with its diagnostic. Then, when live on the native targets from the HUDless return, the `hudless` receipt on the input or FRAME pass with `RSF_OK`, then the HUDless seam (`publication.rs`). | The return address before the original, the original first, then the LastError save and restore around the Rust step. |
| DR before | An open FRAME drops its depth snapshot (the raster snapshot is kept). | The return address and LastError save, the LastError restore before the original. |
| DR after | A call on the native targets while resolution ownership is pending refuses the World and its raster, then transfers the native write (`native_resolution_write`), with its diagnostic on failure. The live call from the DR return on the native targets, not overlapping, on an open FRAME and SUBMISSION before inputs and on the current source asks the screen; only a refusal set of exactly `WORLD_LEASE_INVALID`, then a native owner whose frame, view and window match the FRAME pass and whose Context and chain match the World, configures native resolution with the recovery reserve. A failed configuration refuses the raster only while ownership is pending or the change was refused. Any other call on an open FRAME with pending ownership refuses the raster. | The native LastError captured after the original and restored after the Rust step; the typed screen classification on an empty pass in the ordinary World, the owner read and `configure_native_resolution`. |
| Prepass before | Records the World source, then selects the live or recovery-owned call at nested depth 1 from the prepass return on an open FRAME and SUBMISSION before inputs. On an open FRAME every call drops the depth and raster snapshots, a nested call interrupts depth and a call after inputs refuses the World. The selected call clears the interruption. | The nested depth counter and the guard that restores it when the original returns or unwinds, the LastError save and restore, and the recorded call (`NativePrepassCall`) on the detour frame across the original. |
| Prepass after | Only the selected call, still live or recovery-owned on an open FRAME and SUBMISSION before inputs and not interrupted: the raster capture into the World (result ignored), then a depth capture into a stack snapshot, adopted only when the recorded source is the current render source and the World's source; otherwise the World is refused with its diagnostic. | The typed raster and depth captures with the receipt logger, and the copy into the World depth snapshot. |

`NativeProducerLeaves` has 13 entries: the World read (80 bytes), the snapshot invalidation, the
depth interruption, the raster refusal, `native_resolution_write`, the screen refusals, the scope
leaves' owner read, the configuration, the raster and depth captures, the depth adoption, and the
same resolution receipt and shared diagnostic functions the other tables bind. `NativePrepassCall`
(48 bytes) carries the recorded source and the selection. All three are size-asserted on both
sides. Rust uses the recovery leaves' refuse and same_source, calls the HUDless seam, the recovery
token, the Rust resolution helpers and `rsf_fallout4_cpu_render_source_is_current` directly. The
`rsf_fallout4_render_emit_hudless` export lost its only caller and is removed.

### Differences

- The captures call `rsf_fallout4_depth_capture_raster` and `rsf_fallout4_depth_capture` with the
  ABI identity Rust recorded, instead of the inline wrappers that converted the C++ identity to
  the same value before the same calls.
- The candidate depth snapshot is a Rust stack value starting from the C++ value-initialized
  snapshot (`EMPTY`), and the capture resets its output first, as before.
- The native targets address is `resolution.rs`'s `TARGETS` instead of a second literal, and the
  scope `Owner` fields and `EMPTY` are visible within the crate. The public DLL exports are
  unchanged in source.
- Every new leaf is noexcept. The screen classification keeps its catch; configuration, the
  native DR transfer and the captures were already noexcept, and the owner read is the scope leaf.
- The merge removes the four unused resolution wrappers and their private C entry points.
  Their Rust callers now use ordinary Rust functions, including the producer's callable
  arguments. It also removes the unused C++ raster and depth capture wrappers.

### Traces

Source comparison walked every path of the three original functions. Viewport: the original runs
before anything, a call off the native targets does nothing else, the selector write precedes the
HUDless receipt and seam, and a failed write still allows the seam. DR: the depth snapshot drops
before the original; after it the overlap refusal and transfer come first and block configuration,
the selected path keeps screen, owner, frame, view, window, Context, chain, configuration, then
busy and refused in short-circuit order, and the pending-ownership refusal remains the `else` of
the selected condition. Prepass: the source is recorded before invalidation, selection is computed
before the World changes, interruption is set by a nested call and cleared only by the selected
one, and the capture after the original rechecks live or token, FRAME, SUBMISSION, inputs and
interruption before the raster capture, then depth capture, current render source and World source
in that order. LastError is restored before each original and after each Rust step as before.

A scripted comparison against `c1e16179` found all six receipt texts of the original
`render_hooks.cpp` in the new sources. `render_hooks/producer/tests.rs` adds unrun fake-leaf checks
of the selector write and its refusal on the native targets, the HUDless receipt then seam on the
FRAME and input pass, the DR depth drop, the overlap refusal and transfer, the pending-ownership
refusal, the selected configuration and its refusal, exact screen and owner matching, the prepass
selection, invalidation, interruption and post-inputs refusal, and the same-source capture and
its refusals. Pinned Rust 1.94.1 formatting, locked offline Cargo metadata and whitespace checks
passed. Build, link, Clippy, tests, the shared host fixture, deployment and game checks remain with
the user; this is a source port, not a runtime result. Open game checks: a reduced quality frame
configured at the DR return with the `hudless` receipt and HUDless WINDOW each frame, a nested or
foreign prepass refusing that frame's depth, an overlapping DR writer while ownership is pending,
a Pip-Boy or menu screen leaving native resolution alone, and the owned selector following a
native full-viewport intent.


## Render transaction contracts

F03 starts at `70e01825` and uses the callback and executable evidence above.
The rel32 guard descriptors are the source of the caller return RVAs and the
SR pass key. The CPU and render guards share an exact bounded read with
their existing 16-byte and 32-byte limits. No new executable or shader
evidence was collected.

- Effects entry checks the verified caller, range, manager and open
  imagespace FRAME. Its stack receipt carries CPU identity through the native
  call. An owned recovery token or auxiliary lease can still use this
  boundary after quiesce or source invalidation if the World matches.
  Imagespace and post-Swap cleanup still cover retained obligations.
- Runtime OK releases a recovery token only with NEXT_REDUCTION_ALLOWED.
  The retirement caller observes whether ownership ended. Resolution
  reporting records NOT_READY when output restoration fails.
- The viewport shell holds one typed framebuffer lease on its stack.
  Acquisition returns identity and extent from that lease even on a partial
  read, and WINDOW retains the colour from that same lease. The FINAL_SCENE
  begin and end precede WINDOW begin.
- The normal and reduced SR branches select their evaluation from Rust.
  Reduced depth uses the integer render rectangle. Motion and optional
  colour surfaces use the fractional viewport and camera jitter. Optional
  surfaces are filtered by format and COM identity, and a sealed recovery
  transaction still requires all five surfaces. The native leaf holds the
  input COM references and guards the Runtime evaluation and restoration.
- The callback seams share one native World snapshot and the recovery
  fixtures share an idle table. Resolution and recovery use the same
  output-owner record. The screen reader and publisher share their screen
  record. Bindgen derives the receipt stage IDs and the Runtime/adapter
  request, surface and constants from their C headers.

Source validation used the previous guard tables and callback traces, bindgen
0.72.1, pinned Rust 1.94.1 formatting and Git whitespace checks. No builds,
tests, lint or devices were run. The updated fake leaves cover a foreign
Effects manager and replaced source, an owned return after source
invalidation, withheld retirement, failed output restoration, fractional
auxiliary mapping and a partially read framebuffer. Native run and capture
requirements from the previous sections remain open.

## Typed input and Present boundaries

The baseline was `84a8b9eb`. The follow-up removes residual callback-table and COM
observation work while preserving the same Fallout 4 AE 1.11.240 hook entry and
caller guards. The source comparison followed each replaced native leaf to the
corresponding Rust caller. No new hook sites, layouts or engine behavior were
inferred from that comparison.

- `render_inputs/native.rs` holds the candidate Context, chain and six texture
  references as typed Rust values. Each accepted reader transfers its references
  once. A refusal drops the candidate after the same receipt point. The
  framebuffer lease still crosses as one acquisition, and the resource and
  native-owner rereads still precede publication.
- `d3d11.rs` uses the SDK's typed interfaces for canonical `IUnknown` comparisons,
  immediate Context and chain checks, auxiliary format support and framebuffer
  extent. Reference transfer and the World stores still use native RAII.
- `screen.rs` keeps the guarded entry addresses and calls the menu-name leaf
  first for each menu. The expected name object still gates the menu-open call.
  Freeze and camera probes follow in their prior order. The four native call
  signatures retain their exception guards. The depth, input and resolution
  receipts also retain their guarded callbacks and text.
- `render_hooks.rs` reads the exact outer chain through the renderer's window
  cell and compares frame, view, window, Context and chain against the leased
  values. The source check is repeated after the screen callback. The native
  Present detour closes SUBMISSION before the original and uses the admitted
  CPU identity and matched frame/window to open its CPU phase. Its destructor
  retires that phase on return or unwind and preserves the current LastError.
  The post-Swap WINDOW/FRAME, recovery, auxiliary and resolution retirement
  order remains in `render_hooks/scope.rs`.
- Render MinHook queue and apply calls now link directly to the same MinHook
  instance. Hook creation and checked rollback still use native typed trampoline
  storage. The F01 direct Steam chain, guarded cold creation and CPU ABI are
  the same baseline paths.

The remaining shells contain native engine and host calls, the typed
trampolines, TLS World storage and COM transfer or callback lifetime guards.
Their private headers still describe those boundaries. This pass removes the
depth platform table, the separate input-state header and the native padding
fixture. The padding test now calls the production comparators directly from
Rust. The layout, formatting, generated bindings and locked offline dependency
metadata are source checks. Workspace-wide rustfmt also flags the baseline import order
in the generated Fallout 4 platform bindings. Native build, linkage, tests and game evidence
remain open.


## Typed framebuffer and linked render boundaries

This source pass starts at `0251a81a` and compares the current adapter against
the recorded Fallout 4 AE 1.11.240.0 executable in
[engine metadata](../../games/fallout4/engine.json). The exact-build entry
bytes, call descriptors, owner cells and native render order remain the
evidence for the port. The comparison used the existing adapter and recorded
native observations. It adds no executable or active shader observations.

- `render_inputs.rs` and `render_inputs/native.rs` own the accepted
  FrameBuffer0 Context, outer chain and RTV texture as typed references.
  [The input audit](fallout4-native-inputs-20261116.md#hudless-refusal-attribution-16-november-2026)
  identifies logical slot 0 through the map at `0x2f4d750 + 0xdf4` and the
  physical record at RendererData `+ 0xa58 + index * 0x30`. The renderer,
  map and target rereads still precede acquisition. The input receipts keep
  their native formatting and guarded log callbacks. The temporary lease
  drops texture, chain and Context in the previous native order.
- `render_hooks/publication.rs` takes that acquisition after the guarded
  viewport return at `0x21fa2ca`. It checks frame, window, Context, chain and
  output extent, holds the same texture through FINAL_SCENE begin/end and
  WINDOW begin, and passes it to the native WINDOW owner for retention through
  Swap. The viewport detour restores the original LastError after the Rust
  lease drops. The publication fixtures record the final texture, the
  submission of each pass, and release on acceptance and refusal. A
  partial acquisition receipt still comes from the acquisition that was
  refused. See the [render evidence](fallout4-render-integration-20261116.md).
- `render_hooks.rs` uses the same acquisition for committed-recovery capture
  at the matched outer Present return `0xc335ec`. It checks the request and
  raster snapshot before acquiring, compares the fresh FRAME and Context,
  checks canonical chain identity, then reads the live FRAME and CPU identity
  for the diagnostic request. The native export guards contain exceptions from
  the capture request and submission. The Present detour still closes
  SUBMISSION before the original and retires its CPU phase from the C++ scope
  guard. The post-Swap finish order remains in `render_hooks/scope.rs`.
- `render_hooks.cpp` and `render_hooks_platform.h` link hook create/remove,
  busy and binding operations directly. The guards and reverse rollback remain
  in Rust. Native trampoline slots and originals remain in C++, where the
  World, prepass, imagespace and Present guards still run during real unwind.
  The native LastError saves around original calls retain their positions.
- `render_records.h` defines the screen result, renderer input slot order,
  depth math receipt, resolution receipt and output-owner record used by both
  languages. `render_records.rs` is generated with bindgen 0.72.1. The depth
  snapshot and resolution entry points are called directly where no additional
  native handling is needed. `resolution.rs` saves and restores LastError around
  the complete receipt operation, including refusal paths. Native resolution
  planning and publication planning share the guarded reconfigure leaf.
  Pending ratio, selector and shadow ownership still follow the existing
  same-frame and advanced-frame retirement rules.

The native World still retains the temporal and raster input references,
WINDOW ownership, native trampolines and guards around throwing calls. Pinned
Rust 1.94.1 formatting, targeted bindgen regeneration, locked offline Cargo
metadata and Git whitespace checks cover this pass. Build, link, fixture,
device and game validation remain open.


## Screen and viewport instruction replays

This follow-up starts at `fb27ab2f`, after the F01 typed admission work. It
uses the exact Fallout4.exe 1.11.240.0 fingerprint recorded in
[engine metadata](../../games/fallout4/engine.json). The source comparison
used the recorded instruction windows and the checked adapter on that base.

- `screen.rs` replays `0x1a80480` and `0x1a804a0` as unsigned nonzero
  reads at UI `+0x1ec` and `+0x1f0`. For each index 0 through 12, the
  replay of `0x102b5c0` freshly reads PlayerCamera `+0x28` and compares
  the nonnull state with the pointer at `+0xe0 + 8 * index`. Null skips
  the indexed read. Each read is guarded. An unreadable probe gives the
  existing unavailable classification. The whole-function byte guards,
  all menu name and object checks, the second observation, CPU source
  checks, class sentinels and the LastError guard remain in use. The
  name getter and menu-open routine still execute through the two C++
  exception guards in `screen.cpp`. [Screen query evidence](fallout4-input-ownership-20261116.md#camera-state-and-correction-to-the-frozen-update-interpretation)
- `resolution.rs` replays the explicit-size path of `0x1817ef0`, which
  jumps from `0x1817f32` to `0x1817f97`. That path calls no helpers and
  uses the unit scale from `0x2929458`. Guards cover
  `0x1817ef0..0x1817f33`, `0x1817f97..0x181806d` and the unit literal.
  The replay checks the live TLS/fallback Context against the owned
  shadow, reads its normalized camera port, writes the four float
  components at shadow `+0x90..+0x9c` with the original scalar operation
  order, copies MinDepth as bits to `+0xa0`, sets dirty bit 2 and copies
  MaxDepth as bits to `+0xa4`. Read or write failure follows the same
  output-scope cleanup with a post-attempt shadow observation. LastError
  is restored after the checked replay, before the existing next read.
  Native no-explicit-size calls, selector handling, Context/renderer/HWND
  matching, geometry checks and source retirement stay on the existing
  paths. The retained instruction listing is
  `reduced-native-1817ef0-20261116.txt`. [Viewport arithmetic and native consumer](fallout4-reduced-consumers-20261116.md#native-viewport-builder-does-not-round-the-ratio-product)
- `render_inputs.rs` compares canonical COM identity in the same order
  as the former owner: optional Context and chain, then colour, depth,
  motion, Pip-Boy, VATS and refraction normals. It borrows each current
  pair from the native World at the point of comparison. The generated
  `NativeInputReferences` record describes just the accepted reader's
  reference transfer. C++ still attaches the references to the World
  and releases unused slots. Context/window checks, resource identities
  and COM reference lifetimes retain their current contracts.
- `render_hooks.rs` owns the common 16-message diagnostic budget.
  `render_hooks.cpp` still supplies the bound native log callback and
  retains its exception catch. Resolution passes resolved typed
  reconfigure and config callbacks into the native guards and shares
  the config guard with render publication. It samples config after
  reconfigure and calls the private noexcept recovery reserve after
  preflight. Native format strings and receipt budgets keep their
  prior meanings. Typed original trampolines, return addresses, native
  unwind guards and the Present/Swap LastError precedence stay in the
  C++ detour frames.

Source checks compared both viewport guard spans against the retained
disassembly. Pinned Rust 1.94.1 formatting, bindgen 0.72.1, locked offline
Windows Cargo metadata and MSVC x64 syntax-only checks covered this pass.
Build, link, fixture, lint, device and game verification remain open.


## C render ABI headers and generated records

This source pass starts at `818a5ede`, which already removed the
`render_hooks.h` forwarding step. The remaining data and call boundaries
in `render_records.h`, `render_inputs_platform.h`, `depth_domains.h`,
`screen.h`, `resolution_platform.h` and `render_hooks_platform.h` are
C11 headers with C++ linkage and exception specifications where applicable.
`render_records.rs` is generated from the C declarations in this private ABI. It supplies the camera, input receipts, depth snapshot, World
facts and hook argument records. The Rust seam callback tables still model
required callbacks and supply the existing fake leaves to the Rust fixtures.

A Windows x64 Clang AST comparison of the previous C++ declarations and
current C declarations matched all 30 existing records in size, alignment,
field width and offset. It mapped the renamed input state back to the
original name. The 1496-byte depth snapshot retains maps at byte 1304,
with three 64-byte row-vector matrices in near, world and projection order.
The input reader, history and framebuffer receipts retain 912, 96 and 344
bytes respectively. The temporary baseline copy used the C offsetof header
and Clang builtin because the installed libclang predates the MSVC STL.

The ordinary-World refusal-only call now constructs its empty SDK pass in
one Rust helper used by both producer and resolution admission. The
source check precedes the native menu probes, and the present result ordinals,
no-camera sentinel, argument order, and log callbacks retain their original
values. Source comparison found the existing freeze reads
(`0x1a80480`, `0x1a804a0`), camera equality replay (`0x102b5c0`) and
explicit viewport replay (`0x1817ef0..0x1817f33` and
`0x1817f97..0x181806d`) still run in the prior order. The original-call
detour bodies match after accounting for the C spellings of the Present
constants. The executable fingerprint and earlier guard evidence remain in
[engine metadata](../../games/fallout4/engine.json).

`render_inputs.h` remains a C++ ownership header for WRL references in the
World. C++ still hosts the original trampolines, native unwind guards,
throwing menu and host boundaries, and the receipt formatter calls. Rust
sees the generated receipt or borrowed facts at those boundaries.

The layout comparison, individual and combined MSVC C11 syntax checks,
MSVC C++20 syntax checks, pinned Rust 1.94.1 formatting, byte-identical
bindgen 0.72.1 regeneration and locked offline Windows Cargo metadata all
passed. Build, link, fixture and runtime checks remain open.
