# Fallout 4 native reduced output scope, 2026-11-16

## Question and corrected evidence

The current question is why a native-only Quality1 frame shows a sharp reduced scene rectangle over a full-size scene. The authoritative user image is `.local/fallout4-research/native-raster-20261116-172137-067/user-native-quality1-failure.png`, with `user-quality1-failure.json`. Root's PID66836 run used commitc32d8f7, Runtime `8570CCC20A05FCDE6C533E8BD3748C827CF6D1ED842062C1E90596E5BF2375B3` and Entry `72B274B92AF5F5AFAC334F92E89EBC091812CD3A7FD05CF8B8A43713A393A449`. Its module evidence proves F4SE and ENB absent. The user reported failure at1707x1067 to2560x1600.

This refutes the earlier idea that ENB is necessary to trigger the symptom. The earlier clean no-ENB control had a nonidentical save and does not establish a generally correct native reduced pipeline. ENB compatibility work remains deferred until native acceptance. No wrapper internals were investigated for this task.

The partial capture source16183/native16705 was recorded after reload, with full render_rect and sr_result-3. It does not match the failure image, so it cannot locate the first bad colour writer or prove this image's SR call failed. Likewise an immediate configure-only public status may show preparation/-3 between evaluations. This is distinct from an actual source's evaluated result. Root reports native Quit/Desktop endedC0000005 at retained native4CB273 and restored all21 entries; this is not normal-exit acceptance. No live game was queried or operated by this worker.

Target native image: Fallout4.exe1.11.240.0, SHA256 `fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`. Addresses below are RVAs. Reference source is pinned Community Shaders `e0e711594291c1a8e447ac758099492514810fbb`, studied without copying code. Native scalar/layout guards remain in `resolution.cpp` and related existing adapter verification. Shared REA/IDA sessions were not touched.

## Current source contract

| Point | Current behavior | What this proves or leaves open |
| --- | --- | --- |
| After native UpdateDynamicResolution at return21F9F34 | `configure_native_resolution` plans through the existing host, obtains requested dimensions, verifies source/native ownership, then writes reducedFB8/FBC andFE5 before native dynamic triangle/cache/prepass work | Native input commitment occurs before later camera/resource/screen and actual evaluation admission. Planning alone does not guarantee a later full-output resolve. |
| Pre-imagespace call21FA283 to2185420 | `emit_sr` reads/validates current inputs and same-source depth snapshot, publishes FG input facts, then begins output state and calls reconstruction | Several later refusals return before output scope begins. A committed reduced input does not itself establish successful SR. |
| Successful output scope | Saves native/input ratios and activation, FE4, jitter, shadow viewport and geometry identities; writes ratios1/FE5false/FE4false and uses explicit-size native1817EF0 | Output starts with verified full shadowXYWH. It is not proof of every later submitted viewport, actual geometry choice or final texture contents. |
| Native RenderEffect2185610 | Partial manager+40 wins when manager+48 is nonzero; otherwise effect+A8 andFE4 choose manager+38, with manager+28 on the unit branch | ForcingFE4false selects existing unit geometry on the nonpartial path. Blindly replacing all geometry would also change a legitimate partial path and is not justified by this image. |
| Failed reconstruction | Ends output scope before unchanged native imagespace; reduced input scope remains until source retirement | No equivalent explicitly preflighted spatial recovery transaction is implemented here. Whether this frame's native path fully resolves the reduced input is not demonstrated by source inspection. |
| Whole RenderUI return | Drains auxiliary transaction, restores borrowed output state, preserves genuine nativeFE4false intent, then clears publication state | Output ratios normally restore to the source's reduced input ratios until Swap. This is a concrete state transition, not proof of a late sampling defect. |
| After Swap/source retirement | `finish_resolution_source` restores still-owned original native ratios/activation; ownership changes or failed reads retain/refuse rather than writing a new owner | A correlated receipt is needed to distinguish normal restoration from a failed/changed owner. No ownership guard was relaxed. |

The auxiliary transaction and generic SR crop/reinsertion are unchanged in this task. No provider, pacing, CPU, depth or camera behavior was edited. The unit/dynamic/partial pointers and their native reference ownership remain untouched.

## Late-consumer hypothesis audit

Root identified native callsC383F6 to21FA3B0 andC383FB to21FA930 after RenderUI as possible remaining consumers. Exact native inspection does not establish a late final-copy draw at those points:

- `21FA3B0..21FA42B` calls21D6DE0 twice to clear accumulator bookkeeping, then21D6C10. That latter tail-calls221EE00 withEDX9 andR9zero. The inspected221EE00 path clears pass-list entries; the optional pass-release loop is conditional onR9 and is skipped for this call. It is not a render-batch draw.
- The optional220A700 calls2253580. The complete2253580 body resets descriptor fields and returns; it contains no graphics call. It should not be described as an established texture resolve or final copy.
- Tail21AE190 resets pointers/counters and calls21AFEE0 for populated storage. The inspected21AFEE0 portion iterates storage blocks and calls the allocator cleanup helper165BA00. No direct RTM ratio, viewport or imagespace-geometry consumer was identified there.
- `21FA930..21FA988` clears association flags/pointers and returns, with no direct RTM, viewport or triangle read. This source walk does not purport to prove every possible virtual/destructor side effect elsewhere in the engine.

The old `RenderUI-capstone.txt` stopped at the first unwind fragment21FA315. Its chained continuation to21FA3A5 includes UI work through21C3830 and21C3E50 before the actual function return. Current output cleanup already occurs after those calls. The initial first-fragment assumption is corrected here; no lifetime extension was made from the truncated listing.

Exact retained receipt: `.local/fallout4-research/native-late-output-consumers-20261116.txt`. It contains bounded function/call windows rather than a repeated whole-image dump. No final-copy producer is proved from these calls, so extending output state through lateRenderEnd or Swap would be speculative and could affect unrelated native consumers.

## Reference differences that remain material

`TemporalRenderer::ConfigureUpscaling` prepares render size, verifies resources and `PreflightExternalResolve` before committing requested dimensions. `PublishDynamicResolution` prepares its proxy resources before publishing ratios. `PerformUpscaling` in `TemporalResolve.cpp` distinguishes provider success from safely skipped/failed reduced input. For recoverable cases it runs a spatial recovery and expands chain inputs; unsafe or unrecoverable cases are distinguished and native fallback is scheduled. Its successful RenderUI scope changes ratios and swaps dynamic geometry to the existing unit geometry; EndRenderUI resets ratios to1.

RSF currently commits native reduced raster before equivalent later admission/recovery is guaranteed. This is a concrete source-contract gap worth testing, not proof that the user's screenshot is a fallback frame. The reference also relies on coordinated proxy resources/replacement shader contracts, so copying only its geometry swap or proxy list would not establish correctness. Reusable GPU recovery belongs to the generic host mechanism with verified adapter facts, not new vendor or GPU-processing policy in this adapter. No such mechanism was invented in this change.

## Prepared bounded discriminator, source only

The changed adapter files add existing-report receipts, without changing any ratio write, geometry pointer, TAA decision, lifetime or native continuation. No new option, exported API, hook site or framework is added. Private `ResolutionStage` is adapter-local.

| Receipt stage | Correlation purpose |
| --- | --- |
| `pre-sr` | The exact pre-imagespace hook ran while this source had a reduced input plan. It is not a successful input/evaluation claim. |
| `output-ready` | Records successful or refused full-output borrowing, with current native globals and shadow. |
| `evaluated` | The only row whose `result_is_evaluation=1` contains the actual reconstruction/host result. |
| `effects-return` | State after unchanged native effects0..13, before auxiliary retirement. |
| `hudless` | State after nativeFE4false at return21FA2CA, before the existing HUDless callback. |
| `ui-return` / `ui-restored` | Whole RenderUI state immediately before/after borrowed output-state restoration. |
| `source-retired` | State after the existing post-Swap resolution retirement, including its result and remaining scope obligation. |

Rows carry session/source/native identity, input/output scope and changed flag, current and requested ratios, FE4/FE5, manager partial flag and all three geometry pointer identities, shadow viewport/dirty bit, camera dimensions and actual submitted RS viewport. Pointer identities are not actual vertex contents, shader selection or proof that a partial geometry branch executed.

Graphics metadata queries occur only on the synchronous native callback, when the existing current-owner check verifies exact native TLS/fallback Context, frame/view/window/device-context/chain and supplied leased context. Unknown ownership leaves `owner=0`, `rs_read=0`; zeros are not interpreted as a full viewport. There is no graphics mutation, resource substitution, allocation, readback, extra render pass or off-thread query. LastError is preserved around the receipt.

The fixed process budget is32 ordinary plus32 sampled rows. Native-quality startup is excluded. The first four reduced sources can produce eight stages each; later candidates use native_frame modulo128 and a separate budget. Stage deduplication includes session/source, and a source known to be reduced stays eligible for its retirement receipt after state clears. Native scalar snapshots and RS queries occur only after budget admission. These are bounded snapshots, not a complete draw/state trace.

The next root run should pair the generic configure-only capture fix with a matched actual-evaluation capture while the failure is visible. Interpretation is deliberately narrow:

1. Reduced `pre-sr` with no `output-ready`/`evaluated` row for the same sampled source demonstrates a later admission skip, not vendor failure. It motivates closing the precommit/recovery contract before attributing a failed image to SR.
2. Actual evaluation failure plus reduced state after cleanup makes native recovery/output coverage the next producer question. It does not authorize a guessed full-depth/colour fallback.
3. Actual evaluation success with verified full state through effects/HUDless, but a wrong matched image, shifts investigation toward real colour/geometry/shader producer checkpoints. These scalar receipts alone cannot identify first bad pixels.
4. An owned viewport or ratio that diverges during the successful output scope provides a concrete next native writer to trace. A restored reduced ratio after whole RenderUI, by itself, is expected current behavior and not proof of a late consumer.

## Checks and limits

Source-only checks passed: scoped `git diff --check`, matching44 printf fields/arguments, and exact-byte checks of all11 existing native resolution guard sites against the installed SHA. `.local/fallout4-research/native-reduced-output-static-20261116.json` records source hashes, guards and receipt bounds. `render_hooks.h` is unchanged and included only in the ownership hash manifest.

No build, device fixture, game launch, process query, attachment, deployment or commit was performed. The new receipts are unbuilt and unobserved until root's coherent gate/run. They are a diagnostic handoff, not a claimed visual repair, spatial recovery, sustained SR, FG, latency or normal teardown acceptance.

## Coordinator capture and diagnostic batch validation

The coherent Windows Release VS2026 gate
`verify-inventory-capture-diagnostics-20261116.log` passed:224 passed,14 skipped,
238 total in118.80seconds, with clean Rust fmt/Clippy. The explicitly rebuilt
hidden shared-SR device fixture passed in `capture-plan-device-20261116.log`: a
capture request survived plan/plan calls and recorded the actual evaluation
source/raster; unchanged plans preserved actual success/refusal status, while
a genuine quality change cleared stale output status. The existing actual
backend reset sequence remained1,0,1,0,1,0,1,0, with exact cold and missing-depth
refusals. FG remained Off. This fixture does not claim complete final-image
capture or a native game result.

Built Runtime SHA256 `22830370ac079440177912e1b77680b37d55b7b1cd43915b6e2a68a4f70ce772`
and Fallout4 Entry SHA256 `be2e0bbff6f030a2e69eade3bd3ea57f0d2ac54f996b00d6820b32d48034109f`
identify this batch. CPU conjunction and reduced-scope receipts are built but
not observed in a new game run; no image, source-binding, FG or latency repair
is claimed from these diagnostics.
