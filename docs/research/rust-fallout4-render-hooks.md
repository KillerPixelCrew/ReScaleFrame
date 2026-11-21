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