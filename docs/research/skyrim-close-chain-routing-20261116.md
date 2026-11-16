# Skyrim close-time swapchain routing, 16 November 2026

Question: identify the close-time AV after the Present body separation, then
cover the exact overlay installer's typed method set without suppressing native
teardown or genuine fullscreen transitions. Evidence follows
[scoped creation](scoped-d3d11-creation-20261116.md) and
[the original Steam continuation audit](skyrim-steam-present-continuation-20261116.md).
This worker performed static/source checks only, not a build, game/GPU run,
deployment or commit.

## Captured mechanism and corrected receiver inference

Coordinator PID49592 receipt is
`.local/skyrim-research/input-acceptance-20261116-014431/close-trace.jsonl`,
REA `ev_67b1b108d364e07f70d4067fb4f7676b9e02a484aefb37e0079b96818eaa03b8`.
The run used Runtime SHA256
`c53c8f5361b7034cf83c8ea2e5956585d3493f26979469de29a4a6530ad84841`.
The observer retained an AV at system DXGI21640 reading2b4 with RCX0,
RDI1e1e4c8bc50 and RAX/RBX887a0022. Accurate stack includes DXGI38b5f,
Skyrim1008a2b/6553a3/6514b3/15a843e. This followed completed observation and
the controller's CloseMainWindow. All four files were restored and the
observer detached cleanly, per coordinator receipts.

Exact Skyrim1.7.104 SHA256 is
`846efccf0c1374d71f892907f46549560f2fcb0a75cb87a3eed438baa0f1402f`.
Teardown10085a0 calls immediate-context ClearState/Flush (vtable370/378),
releases renderer+50 twice and global3331f18, then iterates32 window records
at this+70, stride50. At1008a28, slot50 calls
SetFullscreenState(FALSE,null), returning to1008a2b before chain Release.
This is not the previously repaired Present call.

Exact system DXGI SHA256 is
`84bc502f36687bb6c4dd9763d109c6ed7339767ed50a8a70707761563dd3e816`.
Its SetFullscreenState385d0 saves the supplied receiver in RDI. At38b53
it loads receiver+110 into RCX and calls21640 at38b5a. The latter reads
`[RCX+2b4]`, exactly matching the captured null read. Ghidra identifies21640
as CDXGIBaseAdapter::InternalGetAdapterDesc; this is rejection telemetry
preparation after887a0022 (NOT_CURRENTLY_AVAILABLE), not successful native
fullscreen teardown. REA IDs:
`ev_45076ca680241f9065ca1cdb14b11af58729c83181ea241bf2161085564273b2`,
`ev_c520af00ba5d707da4ddc5c2711a7243897d63fa1b9bb61c3ca19cc6ca18ae18`.
The analysis session was closed after inspection.

An initial overlay-pointer inference incorrectly called RDI physical. The
complete identity log corrects it: line349 says facade1e1e4c8bc50 and
old_physical1e2b4e04fe0; line358 says new_physical1e2b4f22600. Therefore
DXGI received the **facade**, not either physical chain. The overlay log's
same pointer was not proof of native identity. A native windowed/fullscreen
failure or lifetime workaround is not justified by this receipt.

## Complete exact Steam registration set

Steam SHA256 remains
`21c0c1ddb4259c1156356bdfe8432dff504d52c7b96d19718688cff2d5a6e52f`.
Installer9c9d0 registers these seven methods through helper95dd0, each with
jump-follow budget1. The same helper follows EB/E9/FF25 at95297..952d4.
The existing compiled real-call wrapper/barrier requirement applies to every
internal body reached by typed dispatch, not only Present.

| Method/slot | Handler | Shared slot | Forward-load RVA and exact bytes | Destination |
| --- | --- | --- | --- | --- |
| Release/2 | 9c320 | 1834b0 | 9c32f `48 8b 05 7a 71 0e 00`;9c375 `48 8b 05 34 71 0e 00` | RAX |
| Present/8 | 9c130 | 1834b8 | 9c154 `48 8b 05 5d 73 0e 00`;9c23e `48 8b 05 73 72 0e 00` | RAX |
| SetFullscreenState/10 | 9c650 | 1834c8 | 9c6cf `48 8b 05 f2 6d 0e 00` | RAX |
| ResizeBuffers/13 | 9c520 | 1834c0 | 9c5bc `4c 8b 15 fd 6e 0e 00` | R10 |
| Present1/22 | 9bf50 | 1834d0 | 9bf72 `48 8b 05 57 75 0e 00`;9c05d `48 8b 05 6c 74 0e 00` | RAX |
| SetColorSpace1/38 | 9c5e0 | 1834d8 | 9c630 `48 8b 05 a1 6e 0e 00` | RAX |
| ResizeBuffers1/39 | 9c3d0 | 1834e0 | 9c4df `4c 8b 15 fa 6f 0e 00` | R10 |

Present1 registration uses QI for SwapChain2 IID
a8be2ac4-199f-4946-b331-79599fb98de7, then its inherited slot22. The last
two use SwapChain3 IID94d99bdb-f1f8-4ab0-b236-7da0170edab1. No additional
swapchain method registration was found within this complete installer.
Ignored full receipts: `steam-all-swapchain-installer-20261116.txt`,
`steam-all-swapchain-handlers-20261116.txt` and
`steam-all-method-loads-20261116.json` under `.local/skyrim-research`.

Fullscreen handler9c650 performs its pre-work, reads1834c8 at9c6cf,
restores receiver/BOOL/output to RCX/EDX/R8, then tail-jumps RAX at9c6eb.
Its missing stack frame is expected. Resize handlers likewise restore their
original arguments, including stack arguments, and tail-jump R10. Release
calls its slot twice on distinct branches, preserving ULONG results and doing
its own AddRef/cleanup in between. Present/Present1 retain both TEST and normal
paths, with the latter applying Steam's own interval/flag adjustments.

## ABI8 adapter wiring and bounded producer receipts

The approved repair extends the existing game-local read-site mechanism to
all ten loads. Each full seven-byte MOV and its following16 native bytes are
guarded. Ten21-byte relays use the original RAX or R10 load encoding and
register-preserving FF25 jumps to site+7; seven immutable typed pointer cells
start at page+512. Existing cold admission, module pinning, queued MinHook
publication/rollback, process-lived allocations and no-stop-unpatch rules stay
unchanged. Installer/helper/global slots are never rewritten. All ten hooks
must publish before a new facade is admitted; any partial failure refuses
startup even if rollback itself fails, retaining safe backing memory.

Host ABI8 `rsf_d3d11_host_dispatch_chain` is authoritative for seven typed
requests. The adapter supplies exact receiver and method arguments; an error
never falls through to native. Handled HRESULTs return unchanged; Release
returns unsigned references and adds no lease or post-release object access.
Unowned receivers read that method's current Steam continuation and forward
unchanged arguments. Present1 keeps its borrowed parameter pointer; resize
keeps count/extent/format/flags and ResizeBuffers1 keeps both borrowed arrays.
The generic core owns real-call/nonvirtual body separation, ordinary temporary
owner leases and Release's special destruction path. No forced-windowed mode,
blanket fullscreen success or suppression of native teardown was added.

A separate narrow diagnostic now logs at actual FG_INPUTS and WINDOW emission:
source/native/scope and original flags, matching CPU source/scope, active state,
World-entry count, screens, exact eligibility, simulation/render state and
reservation owned phase/bound/refusal/poll/job facts. It uses first8 sources,
then source%128, with64 records maximum to match the shared same-source receipt.
It reads facts only and changes no screen, flags, ownership or vendor policy.

Validation here: exact-file hash/instruction checks, source/diff review and
synthetic relocation assertions in
`.local/skyrim-research/steam-all-method-relays-check-20261116.json`.
Those verify ten destinations/registers and seven distinct cells, not execution
or actual allocation. The subsequent bounded validation is recorded below.
Competing external hook writers and late overlay injection retain the existing
explicit limits.

## ABI8 bounded routing and close validation

The coordinator's Windows Release gate passed219 tests with15 skipped and
Cargo fmt/Clippy clean. The scoped creation fixture and plain bridge final-Release
check also passed. These checks cover the coherent ABI8 batch, not every external
wrapper or every installed hook branch.

`.local/skyrim-research/input-acceptance-20261116-020919` records PID20280,
baseline0ecfee57e8d57e411948810c22c6fee1ebf63a73 plus uncommitted changes,
Runtime SHA256 `98566e259e58d1bd18a683a2298c01e699327a0e60b6f5335404a2029cc6cf8f`
and Entry SHA256 `a2a3bc9b92138c0ec76d0d32e54539daf48241fba8d8927102e343f1954373be`.
The controller completed SR/provider-switch observation and CloseMainWindow;
`exit.json` records observation_completed=true and exit_code=0.
`close-trace.jsonl` records ExitProcess0 with no observed AV. REA receipt
`ev_a03626511fc9698905fa4ed90edaca1baded123631523b76ea6e1c17372503dd`
reports detached cleanup with null cleanup error. All four replaced files passed
restoration hash checks. This corrects the earlier failing close experiment above:
typed routing now has bounded normal-close evidence, without bypassing teardown.

At sources1152,1280,1408 and1536, both FG_INPUTS and WINDOW report original
flags0x6, eligible1, bound1 and refusal0 with matching source/scope. The shared
prepare receipt instead retains decimal flags22, including NO_FG, while the
matching input/CPU/reservation admission conjunctions are true. This localizes
the remaining flag discrepancy beyond adapter emission; the core owner is
repairing the shared flag writer. No generated FG, visual correctness, physical
cadence or complete ENB/Steam/RTSS coexistence acceptance follows from this run.
The shared repair still needs its own gate and bounded validation.

## Explicit-provenance follow-up

The subsequent production gate
`.local/skyrim-research/verify-explicit-fg-provenance-20261116.log` exited0:
219 passed,15 skipped, fmt/Clippy clean. The focused
`explicit-fg-provenance-device-20261116.log` passed53 sources,39 captures,
37 SR evaluations and33 tags/generated callbacks, including a genuine sticky
refusal case. Device callbacks do not measure physical scanout or latency.

PID52400 (`input-acceptance-20261116-021712`) corrects the retained-flag
discrepancy above: matching-source receipts retain flags6, continuous1 and
valid1, with317 tags. PID47748 (`input-acceptance-20261116-022508`) records316
tags. Both completed observation, exited0 and hash-verified restoration of four
files. Generated output remained inactive; these runs establish resumed input
admission/tagging and normal close, not generated-frame or visual acceptance.
The latter foreground experiment left WindowsTerminal PID35804 foreground,
so it establishes no focus-dependent cause or behavior. Full mod coexistence
and full FG acceptance remain unverified.
