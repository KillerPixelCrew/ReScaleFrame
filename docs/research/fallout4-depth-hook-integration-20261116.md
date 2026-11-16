# Fallout 4 prepass depth handoff, 16 November 2026

This adapter change connects the independently researched native depth domains
to the existing temporal callbacks. Initial validation was source and static PE
verification only; the later coordinator build/device results are recorded below.
They do not establish live game acceptance. The executable is
Fallout4.exe1.11.240.0, SHA256
`fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`.
Native interval, matrix, TLS-context and camera semantics are documented in
[the depth-domain research](fallout4-depth-domains-20261116.md); this hook does
not invent those facts or modify the native depth resource.

## Producer ABI and guarded boundary

The installed native caller at21F9F73 is `e8 38 99 ff ff`, targeting21F38B0
and returning21F9F78 inside RenderPreUI. Its entry bytes are
`48 89 5c 24 08 48 89 6c 24 10`. The existing whole FRAME was admitted only
at ordinary WorldBegin parentC382A5. That frame fact remains valid after
WorldBegin returns; requiring its short-lived entry TLS flag during prepass
would incorrectly reject this later producer.

The native ABI is `void()`. Pinned CommonLibF4 revision
`7f8deca16dc7dd245c9c52151ec9ef27edd9ad60`, DrawWorld.h89, declares that
signature, as does RenderHooks.cpp251 in community-shaders revision
`e0e711594291c1a8e447ac758099492514810fbb`. The alternate
TemporalRenderHooks wrapper declares an opaque `void*`; that declaration alone
does not establish a native receiver. Exact entry disassembly derives state
from globals and TLS, initializes R8D/R9D at21F38D1/21F38D8 and sets RCX/RDX
at21F3964/21F3976 before its first call21F3983. It consumes no incoming
argument there. The native caller does not use a return value. The wrapper
therefore forwards the unchanged argument-free routine once, preserving its
LastError across copied-state capture. Foreign calls still execute the same
original routine and are not admitted as producers.

## Lifetime and publication

The existing World holds the header-defined `NativeDepthSnapshot` by value.
Each prepass call clears the old copy before the native producer. Nested calls
invalidate outer capture; any producer after temporal inputs also makes that
source refused. Only the exact return21F9F78 within an open ordinary
FRAME/SUBMISSION, before temporal inputs and under the unchanged CPU source,
can capture after the original returns. The CPU identity is rechecked against
the saved identity on both sides of capture. Source teardown resets the whole
World and its snapshot. No native matrix pointer or second source ledger is
retained.

After the leased reader pass matches the pre-world frame/view/window/context/
outer-chain owner, `make_native_depth_domains` must succeed before planning,
explicit FG_INPUTS and actual SR. The same copied descriptor accompanies each
pass. Missing, stale or unsupported facts refuse temporal publication before
the host sees a raw mixed-depth substitute. The generic ABI23 conversion owns
normalization; native source depth and downstream game consumers stay intact.
This is scalar depth conversion at existing composed raster samples, not
spatial reprojection of first-person geometry. Native motion coverage and
first-person visual quality remain separate evidence requirements.

`verify_native_depth_domains()` joins cold preparation and pre-enable guards.
The producer hook uses the existing queued enable/disable mechanism. A cold
CreateHook failure removes only hooks created in that preparation attempt;
if removal fails, their ownership remains recorded and startup refuses.
The new module is linked by Fallout4 CMake. Declared capabilities remain0 and
the native-only quality choice remains unchanged.

## Checks and limits

Read-only PE comparison passed all7 hook-entry guards,10 direct-call targets
and7 existing Present continuation guards, including the new prepass entry
and caller. Source checks confirm clear-before-producer, capture-after-return,
and descriptor-before-planning/FG/SR order. Receipts are retained under
`.local/fallout4-research/depth-hook-guard-check-20261116.json` and
`prepass-hook-entry-proof-20261116.txt`. Scoped UTF-8 and whitespace checks
were performed. No worker build, game, device test, deployment, capability
enable or deferred-restoration invocation occurred in that source freeze.

The coordinator subsequently passed the coherent ABI23 Windows Release VS2026
gate `verify-depth-domains-r2-20261116.log`: exit0,220 passed/15 skipped,
107.24seconds, Cargo fmt/Clippy clean. WARP auxiliary_sr_device152 passed.
The first gate attempt failed on a Windows near-macro collision in new fixture
locals; renaming them preceded the complete r2 pass. The foreground-denied
anomaly_fg_dlss_sroff_device skip does not establish activity. The separately
rebuilt `depth-scoped-fixture-20261116.log` passed55 sources,41 captures,
39 SR evaluations and34 tags/generated callbacks. These generic device tests
exercise the shared path, not native Fallout4 source provenance, first-person
quality, physical scanout or measured latency. Exact built hashes are recorded
in [engine metadata](../../games/fallout4/engine.json).

The built batch has not replaced current PID37012
(`native-raster-20261116-112938-741`), which remains the older ABI22 deployment.
Its scene/native-Quit handoffs timed out with focus denied and no keys sent;
result.restored remains false and guarded recovery is unexecuted. Native
snapshot/normalization/SR quality acceptance, full reduced scales, FG, latency
and the shader/motion audit remain mandatory pending. No successful game close
or restoration is inferred from the generic fixture or the new build.
