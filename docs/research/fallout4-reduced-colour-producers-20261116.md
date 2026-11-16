# Fallout 4 reduced Main-colour producers, 2026-11-16

## Question and evidence boundary

Why does the Quality 1 capture already contain scaled duplicate rectangles in native pre-SR Main, while the reduced GBuffer and depth footprint is clean? This offline audit traces native writers and sampling contracts rather than changing crop admission or attributing the image to a vendor.

Target: Fallout4.exe 1.11.240.0, SHA256 `fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`. All addresses below are RVAs in that image. Reference: Community Shaders `e0e711594291c1a8e447ac758099492514810fbb`, CommonLibF4 `7f8deca16dc7dd245c9c52151ec9ef27edd9ad60`. Reference implementation and replacement HLSL are study material, not native execution evidence or copied code.

Root's actual report `.local/fallout4-research/native-raster-20261116-155136-917`, PID64716, Runtime `8570CCC2...`, source5737/native6211, has a complete matched-Present Quality 1 capture. The copied viewport is 1707x1067 and assembled SR reset is zero. Root observed duplicate rectangles. Independent numeric analysis supplied by root establishes:

- The native Main top-left 1707x1067 RGB crop equals SR source exactly across 5,464,107 components. The first bad captured colour is already `native_color.png` before this source's SR call.
- Replayed depth normalization differs by at most 7.32e-8 on world/nonclear samples. All five auxiliary expansions match the specified jitter-aware nearest operation.
- Native GBufferAmbient (logical30, previously PipBoy-labelled) and depth have a clean reduced geometry footprint, unlike Main.

These facts exclude the crop or this same-source SR call as the first introduction visible in these artifacts. They do not exclude earlier-frame SR feedback, later native consumers of full-allocation output, or ENB interception. Root is preparing a same-quality ENB-isolated control; no result from that control is assumed here. No game, GPU, build, attachment or deployment was performed for this audit.

## Native route into the seam

| Stage | Exact native evidence | Contract and remaining limit |
| --- | --- | --- |
| Frame setup | WorldBegin `21F0AC0`; `21F0AE6` calls `1839EC0` to set RTM+FE4=1 | Native dynamic/default viewport selection is enabled. This alone does not identify a subsequent colour draw's viewport. |
| Reduced setup | RenderPreUI `21F9F2F` calls UpdateDynamicResolution `1839ED0`, then `21F9F3B` calls geometry updater `2186110` | Adapter ratio planning runs after the former call and before the latter. Native dynamic UV geometry is updated from the ratios. |
| GBuffer | PreUI calls `21F30B0`, `21F33F0`, then DeferredPrePass `21F38B0`; return `21F9F78` is the depth snapshot | `21F33F0` requests clear-mode bindings for logical26,27,29,30 and optionally31, then32. These are distinct from Main1. |
| Deferred composition | `21FA031` calls `21F82A0` | First output selection at `21F834E` is MainCopy2, mode3. `21F8495` draws through `221FB60`. Later Main1 is selected at `21F8979` or alternative `21F89D7`, mode3. Main composite call is `21F8BB5` (+915); an optional later draw is `21F8C7E`. |
| SSR inside composite | `21F868E` (+3EE) selects logical4 mode3; `21F86A5` selects logical8 mode0. Pooled acquire `21F88AE` (+60E), release `21F8963` (+6C3) | Native target selection and scoped reference anchors match. Actual selected shader, SRVs and raster geometry on the bad frame are not captured by this static audit. |
| Forward | `21FA036` calls `21F9100`; Main1 mode3 at `21F9412`, then again `21F9788` | Main has more writers after deferred composition. `21F9772` calls RenderEffect `2185610`; its contribution must not be silently attributed to the earlier composite. |
| Pre-SR | RenderUI `21FA283` calls RenderEffectRange `2185420` | Current reader observes Main before that call. It proves texture/owner identity and current viewport bounds, not the mapping of every prior Main writer. |

`1839B70` is SetCurrentRenderTarget: RCX manager, EDX slot, R8D logical target, R9D mode. CommonLib's mode3 is restore/no-clear, mode0 clear. Logical-to-platform mapping is manager+DF4; a logical number is not a platform array index. The composite's Main selections retain existing contents. This does not by itself prove stale contents survive the draw: full draw coverage, blend state and shader discard still matter.

`1839E00`, called at `21F838F`, `21F89BA`/`21F8A18` and forward sites, builds viewport state through `1817EF0`. It passes a full/default selection derived from FE4; the builder combines native target dimensions, normalized viewport and dynamic ratios. The previously chosen analyzer label “ApplyNativeTargets” is only a label, not an OM binding or clear operation. Likewise `18231C0` is a verified `ret 0` stub in this build, not proof of a GPU bind.

## Clears: what is actually proved

The exact native flush family `1823CF0` contains real D3D11 ClearRenderTargetView calls at `1823D9A` and `1823DEC` (context vtable displacement190, slot50). It resolves the queued platform RTV, tests its pending mode for zero, clears using shadow clear colour+8E0, then changes the mode to3. Thus a mode0 request can become an actual full-view RTV clear when this dirty state is flushed. A viewport-sized geometry draw is not equivalent to that clear.

A separate offline scan retained 197 verified direct calls to `1839B70`. It found conditional slot0 Main1 clear requests at `1A82154` and `1A82273`: R9D=0, EDX=0, R8D selects Main1 or framebuffer0 according to a pointer+68 test, only when `1839910` returns -1. These are concrete possible Main clears, not a demonstrated per-world-frame clear. The scan does not cover indirect calls, inlined writes or prove branch execution. Neither those requests nor the native flush has been correlated to source5737's actual Main identity.

Consequently the last actual Main clear before the captured colour remains unknown. Prior-frame in-place SR feedback is retained as a hypothesis, not established as the cause and not dismissed by the same-source crop equality.

## Stock composite bytecode and constants

The native shader archive SHA256 is `13569a497f9ba091a2be293859bcf5585ad8faa8fc1d3ad74991b8512eea125b`. Existing inventory/join matches native raw bytecode to the pinned reference archive. This audit used system D3DDisassemble on all 203 native composite shaders (23 VS, 180 PS), not compiled replacement shaders. 164 PS variants read CB2[0] screen-coordinate constants; 16 do not. Live variant selection is still required.

For example native PS ordinal3401, descriptor68, consumes SV_POSITION.xy, emits SV_Target0 and declares CB2[3], CB12[47], 2D resources t0..t7 and cube-array t8. Its literal coordinate operation is `mul r0.xy, v0.xyxx, cb2[0].xyxx`, followed by texture sampling at those coordinates. Later operations also consume CB2[0].zw. Pixel coordinates therefore depend on the supplied constants and resource allocation, not merely on a normalized fullscreen triangle UV assumption. Resource slots alone do not establish logical engine target names.

RTTI identifies primary BSDFCompositeShader vtable `29161A8`, SetupGeometry slot7 `22174F0`. At `2217576`/`2217598`, that function obtains current target width/height from `1839830`/`18398A0`, writes their reciprocals into a reflected constant slot selected via shader metadata+58, and writes reciprocal ratio values from RTM+FB8/FBC into its next components (`22175BA`..`22175D0`). The dimension getters read current native target metadata, without multiplying by FB8/FBC. This establishes a native allocation-size plus ratio contract. The reflected slot index and actual bound constant buffer on the bad draw still need correlation before equating these writes with a particular disassembled variant's CB2[0].

For full-allocation GBuffer textures, pixelXY/full-allocation-size can correctly address a top-left reduced footprint. Changing just that divisor to reduced dimensions could instead sample the wrong region. Conversely a physically reduced proxy requires matching constants. The static expression alone is not a demonstrated native sampling defect.

## Reference proxy path versus current adapter

`src/Render/TemporalRenderHooks.cpp` scopes the reference deferred composite around BeginProxyPass with normals, AO, environment, ambient, emissive, diffuse/specular, MainCopy, tiled and half-AO inputs, then EndProxyPass Main. SSR similarly proxies its inputs and uses a unit triangle, with separate pooled-target handling. `features/Upscaling/src/DynamicResolution.cpp` physically allocates render-sized proxies, copies the active top-left source, swaps platform records, scales target metadata widths/heights, repoints bound PS SRVs, sets unit ratios and restores/copies designated outputs afterward. This is a coordinated resource/metadata/geometry transaction, not only a viewport patch.

Current `resolution.cpp` keeps full native allocations and writes native ratios before triangle/prepass work. Its output scope starts after the reader/depth snapshot at the pre-SR seam, switches output viewport/ratios and reinserts SR into the same full Main allocation. `render_hooks.cpp` does not apply the reference pre-SR proxy transaction. Replacement composite HLSL is also not evidence that vanilla bytecode requires the same transaction. Native dynamic-resolution support and replacement-shader assumptions must remain separate until actual bindings identify a mismatch.

## Smallest discriminating facts

1. Compare root's matched ENB-isolated control before attributing the first pre-SR defect to stock code. ENB may perform work inside native context calls before this seam.
2. For the last actual Main clear and first Main-writing composite draw, correlate exact source/native ID, canonical Main resource/RTV, queued clear mode and actual ClearRenderTargetView or draw execution. Determine whether this frame overwrites the full relevant region before any sampling of previous Main/MainCopy.
3. At the first bad producer boundary, retain actual viewport/scissor, VS/PS identity, selected draw geometry, CB2 screen constants/reflected slot, SRV resource/view identities and samplers. Compare input images before the MainCopy draw, SSR and final Main draw. Existing pre-SR snapshots cannot distinguish those writers.
4. Only if the matched native producer is clean on entry and wrong on exit should a geometry/constant/proxy correction be proposed. If the input is already wrong, follow that resource's prior writer or prior-frame use instead.

No production repair is justified by this static audit alone. It does establish exact callable boundaries and numeric contracts for a small subsequent observation. Current crop/depth/owner checks should remain unchanged. Successful SR evaluation and reset0 do not establish whole-image quality, sustained FG, scanout or latency acceptance.

## Retained offline receipts

Under `.local/fallout4-research/`:

- `trace-colour-producers-offline-20261116.py`, `colour-producer-native-20261116.json/.txt`: exact-SHA Capstone call windows and chained-unwind family ownership.
- `colour-clear-and-composite-constants-20261116.txt`, `colour-native-dimension-getters-20261116.txt`: exact native clear and constant producer instructions.
- `find-native-main-clear-20261116.py`, `colour-all-native-rt-bindings-20261116.json`: bounded direct binding inventory. Call windows are not dynamic argument proofs.
- `colour-composite-rtti-20261116.json`: primary RTTI/vtable only; secondary data beyond its known entry was excluded rather than treated as function pointers.
- `disassemble-colour-producer-shaders-20261116.py`, `colour-native-shader-contracts-20261116.json` and selected `colour-native-composite-*-20261116.asm`: stock bytecode interface/coordinate summaries. No replacement compilation or repeated whole-container dump.

Checks for this note: receipt JSON parse, offline analyzer Python syntax and scoped diff whitespace. These are source/artifact analysis checks, not runtime validation.

## Same-quality ENB-isolated control and next scalar observation

Root completed report `.local/fallout4-research/native-raster-20261116-161308-012`, PID71412, with unchanged Runtime `8570CCC20A05FCDE6C533E8BD3748C827CF6D1ED842062C1E90596E5BF2375B3` and Entry `463B1EED47090204DBB301B0332CA5CB9E5938907A67274994D59980E5AED0C7`. Only the local ENB proxy was temporarily quarantined under root's backup/hash guards. Root observed the stationary shower viewpoint clean at native Quality0 and Quality1 (1707x1067 to2560x1600), unlike the ENB-on nested rectangles. Capture `scene-20261116-161912-234-104267921-71412-1` is complete/matched Present, source19050/native19505. Root's numeric comparison also finds the active Main crop clean; stale pixels outside that crop are expected and are not this defect.

This is useful coexistence discrimination, not a byte-identical save comparison: the menu named Sanctuary Hills level1 8min, versus the prior ExitSave6min. The control strengthens the case for investigating ENB interaction with native reduced production. It does not yet identify a particular ENB writer or establish that native production is universally correct without it. Root closed by ordinary AltF4; the retained baseline `4CB273` null+50 fault/C0000005 recurred. Controller restored20 entries and root restored ENB SHA256 `AC194BD525BB1C3CF363448F1D2B9F96DD859424E6FEAC00695B152B4B951388`. This is not normal-exit, FG, latency or coexistence acceptance. ENB remains required for delivery.

Root's additional pinned-reference review found ENBRenderInfo exposes wrapped device/context/chain and screen dimensions, with no supported per-pass dynamic-resolution contract established. Reference Rules.cpp disables features under ENB. Its proxy framework therefore cannot be assumed ENB-compatible, and that disable policy is not adopted here.

### Frozen ignored scalar observer

File: `.local/fallout4-research/trace-colour-producers-20261116.js`, SHA256 `EA5F116CF17C9FDE536517C8DFF957A3C302309277946AEAC6F55B7C80E23B1D`.

The coordinator must prefix `globalThis.RSF_COLOUR_TRACE_CONFIG = <JSON>;` and pass the resulting source to its existing owned-PID Frida attachment. This audit did not attach or execute the observer. Required JSON fields are:

| Field | Required fact |
| --- | --- |
| `expected_pid` | Fresh owned live PID, never a prior report PID |
| `expected_executable_sha256` | Full executable SHA above |
| `expected_entry_sha256`, `expected_runtime_sha256` | Actual matching loaded artifact file hashes, not assumed current build outputs |
| `expected_main_texture` | Fresh exact nonzero hexadecimal native Main texture address from verified current-run reader/creation evidence |
| `minimum_native_frame` | Fresh world native-frame floor from current-run receipt |
| `output_width`, `output_height` |2560,1600 for this proposed observation |
| `render_width`, `render_height` |1707,1067 for settled Quality1 |
| `warmup_ms` |1000; accepted range1000..10000 |
| `output_path` | Fresh absolute `.jsonl` within this repository's `.local/fallout4-research/` report tree; exclusively created |

The script requires exact executable path/hash, one matching Entry and Runtime, and the exact ENB proxy path/hash. A separate system `d3d11.dll` is allowed by exact path selection rather than confused with the proxy. Ten native sites are preflighted against exact PE bytes before attaching. Only the existing pre-SR Entry detour is admitted as an alternative, and only an expected native suffix plus an E9/direct-or-FF25 route into the hash-matched Entry is accepted. Unknown ENB-modified native entry bytes refuse; no bypass is attempted.

It selects one Composite entry after warmup when output dimensions, reduced ratios, FE4/FE5, minimum native frame and Main identity match. Native-frame and Main identity changes subsequently terminate the receipt as incomplete. Source IDs are not invented from native frame; adapter receipts must correlate them. The pre-SR boundary is observed before its original continuation executes, and instrumentation detaches there. It preserves arguments, returns and LastError. There are no D3D getters, game calls, graphics allocations, readbacks, extra processing passes or exception-handler changes.

| Receipt | Exact observation site |
| --- | --- |
| Native composite and forward phase entry/exit | `21F82A0`, `21F9100` |
| MainCopy/Main/optional draw wrapper entry/exit | `221FB60`, return filters `21F849A`, `21F8BBA`, `21F8C83` |
| Reflected screen constant after write | `22175D6`, four floats at RBX+RDI*4; labelled reflected slot rather than presumed CB2 index |
| Native target request | `1839B70`, manager identity and actual slot/logical/mode arguments |
| Native-issued RTV clear calls | `1823D9A`, `1823DEC`: actual receiver, RTV, clear colour, vtable/target module/RVA/entry bytes before the context target |
| Native-issued OM/viewport calls | `1823F17`/`182431C`: actual pointer/count arguments and target route before the context target |
| Native pre-SR seam | `2185420`, return filter `21FA288` |

It additionally observes selected setters/draw method entries from the verified application's current context vtable. This records Main bindings, shader/SRV/sampler/constant-buffer identities, IA geometry inputs and raster arguments where those calls are actually seen. An unchanged inherited binding is explicitly unknown, not absent. Native-issued calls and application-context entries are separate receipt levels and may describe the same call. A module/RVA identifies the target entry, not the final GPU execution or its complete forwarding chain.

The duration is20seconds, fixed total384 rows, per-phase24 context rows (at most12 draw rows),16 native-state rows and8 native-clear rows. Constant receipts are also bounded. Drops are counted and make sequence completeness unavailable. Startup cannot consume the selected-frame budgets. A four-entry rolling record of matching native-issued Main clears after warmup is included at selection, with its observation start time; an empty ring never proves Main was never cleared. Receipt writes flush synchronously and never replace prior evidence.

No verified ENB-inner context pointer/layout is currently available to this observer. It therefore does not follow guessed wrapper offsets or claim to see every inner draw, viewport change or clear introduced by ENB. If the route terminates at an ENB entry and scalar results are insufficient, the next prerequisite is a verified native inner-context identity from creation evidence or exact-build forwarding disassembly. That separately verified pointer could permit bounded inner-entry observation, with explicit nesting/duplicate labels. It is not supplied by guessing the application context's ownership from a method name.

There is no existing generic intermediate checkpoint service in the current HostABI. If scalar observations cannot establish a concrete mapping mismatch, first-bad-pixel attribution still needs a separately reviewed reusable checkpoint mechanism owned by the existing dump/GPU capture path. Candidate checkpoints remain after MainCopy `21F849A`, Main composite `21F8BBA`/optional `21F8C83`, and Forward return `21FA03B`. Counterfeit `process_upscale` passes or a Frida GPU-copy framework are not alternatives.

Static checks: `node --check` passed. `.local/fallout4-research/colour-producer-observer-mock-20261116.cjs` passed14 in-memory cases covering configuration refusal, exact-byte guard refusal before any hook, owned detour admission, coexistence of proxy/system module names, warmup and reduced/Main selection, frame change refusal, LastError preservation and entry-only hooks at interior instruction sites. The mock uses no process or graphics API and is not an attachment/graphics validation. Final static review also guards the reflected-constant producer instructions, its absent-slot FF branch and the actual write destination before reading RBX/RDI. The absent, valid and mismatched slot paths have dedicated mock cases. Exact native bytes are retained in `colour-observer-pe-guards-20261116.json`.

## Actual ENB-on scalar result and verified inner discriminator

Root's next unchanged-binary report is `.local/fallout4-research/native-raster-20261116-162951-354`, PID74428, ENB present at the exact hash above. Root again observed nested rectangles, now looking down in the shower. Matched capture `scene-20261116-163231-244-105066921-74428-1` is source7570/native8002. The corrected scalar trace is **native9814**, not that capture's frame. Neither trace may be described as same-source intermediate pixel evidence.

Root initially ran the earlier observer SHA `10A6D0BF...` on native7397 before receiving the constant-guard correction. Its file remains retained as `colour-producers.jsonl`; constants from that run are not trusted. Corrected SHA `EA5F116C...` ran afterward as `colour-producers-r2.jsonl`, reached selected-frame-complete, and retained166 numbered rows plus the detached receipt. It observed449 context events and dropped361 under the explicit budgets. It did not observe pixel contents.

| Corrected native9814 fact | Interpretation |
| --- | --- |
| Application context `0x227fbeb2110`, vtable `0x180114558`; Main physical4, texture `0x2295a14ca60`, RTV `0x2295a0e43a0` | Context targets lie in the exact ENB module at base180000000, not system D3D11. Same basename alone would conceal this distinction. |
| Three guarded reflected slot0 constants: approximately `[1/2560,1/1600,1.4997071,1.4995314]` | The native allocation-size/inverse-ratio producer contract is actually exercised in MainCopy, Main composite and optional Main passes. |
| MainCopy and Main native-issued viewport1707x1067 | No wrong Main XY extent is demonstrated at those boundaries. |
| Forward temporarily issues2560x1600, but immediately after binding logical13/RTV `0x2295a0e4220`; Main is then rebound with1707x1067 | The full-size transient is not evidence that the observed draw writes full-size into Main. |
| Retained clear targets RTV `0x2295a0ac160`, logical8 | This is the SSR blurred target, not Main. The empty recent Main-clear ring is only bounded negative evidence. |
| MainCopy and optional Main DrawIndexed3, IB format57/offset41216 and VB stride12/offset40960 on buffer `0x2295a2b79e0` | Geometry identities and counts are observed; actual vertex contents and complete inherited state are not. |
| Main-composite PSSetConstantBuffers start2/count1 at native `2217BCC`, shader/resource setters and correct Main binding were seen | No live bytecode hash or all inner modifications are established from these pointers alone. |
| Main-composite accumulated35 context events, but no Draw receipt | The old24-event aggregate setter budget was spent before the draw. Absence is instrumentation loss, not a skipped native draw. |

Thus no concrete wrong Main viewport, clear or geometry is proved by r2. The next observer specifically repairs the diagnostic loss and follows only a newly proved inner owner. Root reports ordinary AltF4 and the fresh baseline4CB273/null50/C0000005 fault; this does not change rendering attribution or establish normal teardown.

### Exact forwarding proof

The application's vtable RVA114558 was checked offline against all115 entries in the exact ENB PE. Each of the24 selected slots matches the r2 entry. The selected wrapper methods contain loads of receiver+6C20; several trivial forwarders establish its meaning directly:

- ClearRenderTargetView at `2C2E0`: load RCX from `[RCX+6C20]`, load its vtable, tail-jump slot50 (`+190`). The complete instruction sequence is17bytes, beyond the earlier16-byte entry receipt.
- RSSetViewports `2C130` first caches the supplied count and all six viewport scalars per entry at wrapper+A98/AA0. At `2C19D` it loads `[RCX+6C20]`, restores the original viewport-array argument from R10, then tail-jumps slot44 (`+160`) at `2C1AA`. This wrapper's ordinary forwarding code does not rescale that array.
- IASetInputLayout loads the same field at `2BB47` before its slot17 continuation. OMSetRenderTargets loads it at `36204` and calls slot33 at `36217`. DrawIndexed includes same-field loads at `6F876` and `6FC62`, with native DrawIndexed dispatch among its branches. Complex branches may also perform ENB work; these facts do not declare them transparent.

Receipt: `enb-colour-context-forwarding-20261116.json/.txt`, generated by `analyze-enb-context-forwarding-20261116.py`. Initial first-unwind-fragment inspection truncated several small functions. The retained analysis corrects this using exact vtable method boundaries or chained unwind families, and labels raw intervals rather than claiming every intervening byte is one function. The quoted forwarding instructions and slot ABIs, not proximity alone, justify reading the one inner pointer.

### Separate frozen inner observer

`.local/fallout4-research/trace-colour-producers-inner-20261116.js`, SHA256 `ED9D56A98B2A5009B98E7871A9462206F4A400AFF6174E9191F1C05FCB3424C7`, is a separate file. Earlier helpers and actual traces are unchanged. Root uses the same `RSF_COLOUR_TRACE_CONFIG` prefix contract with fresh PID, Main pointer, native floor, matching artifacts and fresh output path. No additional graphics call is needed.

Additional guards require exact ENB vtable RVA114558 and all24 method addresses/entry bytes, plus the complete forwarding sequences above. Only then does it read context+6C20, reject null/self before dereferencing, and require its vtable and selected method targets inside the hash-matched system module `C:/Windows/System32/d3d11.dll`, SHA256 `9253355CD78FBC6AACB72E91BE264D04B145EF06D64CBF496C8DE3E5F0E44EDB`. A different system build or third wrapper refuses. It does not continue walking pointers. The exact inner identity/vtable must remain stable through the selected frame.

Outer and inner entries are recorded separately with module path/base and invocation nesting. The same forwarded call is expected to appear twice, so event counts are not draw counts. ENB-internal calls to the verified native-inner methods can now be distinguished from an outer forwarded call. Off-thread work, unhooked methods, inherited bindings and downstream method-code detours remain limitations; no receipt claims GPU completion or complete pixel provenance.

Draw admission has its own12-row budget per phase and level **before** setter budgeting. OM/viewport/scissor/clear each have6 rows per phase/level; other methods have2. Bounded scalar setter state is updated even after its setter row is suppressed, and each retained draw includes the latest observed shader/CB/SRV/IA/raster facts for that receiver. Untouched inherited state remains explicitly incomplete. The global limit is768 rows/20seconds, still only one selected native frame. No shader/buffer contents are inferred from handles. No resource pointer is retained as an owned COM lease and no target is touched.

`node --check` passed and `colour-producer-inner-observer-mock-20261116.cjs` passed18 in-memory cases. Added cases cover null inner field before dereference, wrong system SHA, method-address alias refusal, and a draw surviving30 suppressed setters with correct separate outer/inner nesting and retained bounded state. The actual root attachment of this new observer is pending; these are mock/static checks only. Root will combine it with a subsequent coherent batch, not infer a production fix from the current r2 trace.
