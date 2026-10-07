# Graphics helpers

Shared D3D11 helpers for the carriers and runtime. Game-specific resource identity and conventions belong in `games/<id>`; vendor evaluation belongs in `runtime/backends`; cross-API presentation belongs in `runtime/presentation`.

## Source map

Public C headers live in `include/rescaleframe/`; implementations live in `src/`. Each module name below names both its header and implementation unless noted.

| Module | Responsibility |
| --- | --- |
| `d3d11_observer` | Observe resource creation, source Present events, and queued diagnostic dumps |
| `frame_tap` | Shadow one immediate context, report selected draws, substitute resources, and watch constant uploads |
| `resource_roles` | Classify descriptor-based candidates using AC7 capture-derived heuristics |
| `resource_ref` | COM retain/release and caller-owned back-buffer/RTV references for C callers |
| `constant_buffer_read` | Synchronous staging readback with resource type/device checks |
| `constant_twin` | Fixed-capacity replacement-buffer cache keyed by borrowed original identity |
| `motion_decode` | Remove plugin-defined stored bias/scale and preserve unwritten pixels as a sentinel |
| `motion_resolve` | Fill sentinel pixels with camera reprojection and emit dense pixel motion/device depth |
| `colour_transport` | Encode bounded HDR input and decode it before engine grading |
| `colour_fidelity` | Apply same-frame spatial colour residual correction with depth-edge suppression |
| `scene_promote` | Build output-resolution substitutions for caller-identified scene/composite/UI targets |
| `depth_replay` | Immediately replay selected live geometry into a scene-depth copy |
| `d3d11_state` | Paired full-context or narrow depth-binding snapshots with owned getter references |
| `texture_dump` | Blocking diagnostic TGA/JSON readback; `texture_dump_raw.cpp` writes exact subresource-zero bytes |
| `present_blit` | Legacy reconstructed-scene debug view before Present |
| `fullscreen_pass` | Fullscreen copy, diagnostic tonemap, alpha view, and premultiplied composites; the blend formula alone does not validate a game FG UI guide |
| `ui_layer` | Two-slot transparent RGBA8 layer ring at output extent |
| `ui_identify` | Bounded pointer-membership sets and CRC32C shader names |
| `overlay_renderer` | Upload overlay atlas/meshes and draw into the caller's current target |
| `overlay_input` | Capture window input and provide optional cursor/raw-mouse detours |

The frame tap is retained compatibility/diagnostic machinery using format and binding heuristics. The default AC7 native plugin uses engine-owned views, graph roles and queued identity. Current implementation and acceptance limits are recorded in [current status](../../docs/current-status.md).

## Ownership and call order

Initialize each size-checked C structure's `struct_size` and any declared `abi_version`. A returned result reports CPU validation/setup/submission, not GPU completion or a verified picture. Logging callbacks are synchronous and borrow their message text.

Use the owning render thread for immediate-context work and serialize setup/destruction with callbacks. Texture/view accessors are borrowed unless the header explicitly transfers a COM reference. Retaining a texture preserves its allocation, not its contents; consume or copy it at a defined point in its source frame. Release back-buffer references/views before resizing.

The observer's creation callbacks hold its mutex and only record facts. Present callbacks run outside it. The frame tap suppresses nested injected work so it cannot enter the game-state shadows. Plan/candidate/divert storage requires render-thread coordination: publication flags do not retire a reader already inside a hook. Keep callbacks and named resources alive until those readers finish.

Wrap injected backend work in a paired `d3d11_state` save/restore when it can change arbitrary bindings. The small fullscreen/debug helpers have narrower snapshots; their headers name the inherited effects and unsupported binding state. Snapshots preserve bindings, not resource contents or query execution.

## Historical compatibility mechanisms

The observation, extraction and promotion mechanisms below describe the earlier D3D11 route and its limitations. The accepted native AC7 integration sizes engine UI targets and routes bloom/exposure through its SR graph; these heuristics are not its default renderer. A widget target is not inherently a premultiplied vendor FG UI guide. See [the native refactor](../../docs/research/ac7-native-renderer-refactor-20261001.md) and the current status above.

### Resource observation and substitution

Frame-tap reconstruction inputs use format, size, and binding heuristics. They do not establish a verified AC7 shader/view/frame identity. Motion, depth, and selected floating scene colour qualify the input set; exposure is optional. A qualifying callback sequence counts passes, not presented frames.

`rsf_frame_tap_watch_target` reports bounded draws into a caller-named target, including effective viewport and pixel inputs. A watch borrows texture identity; an owner retaining the back buffer for the watch must clear it and release that reference before `ResizeBuffers`.

A promotion plan opens scene reads at the first single-target composite/recombine binding, after scene rendering. Earlier reads retain native scene colour to avoid feeding unfinished lighting with a reconstruction. The tap redirects selected shader/target/clear/copy bindings and scales requested viewports/scissors while promoted targets are active. The owner supplies target identities, depth policy, and the gate callback after `rsf_promote_fill_plan`, which resets the other plan fields.

On the recombine route, the gate seeds output-size scene colour; the game's recombine composites translucency; later native TAA writes go to scratch; a second gate copies the recombined result to the tonemap input. Clear/quiesce the old plan before rebuilding replacements.

This compatibility promotion preserves the game's grading/compositing passes. Bloom and unpatched texel-size constants can still describe render-resolution inputs on this route, and dropping mismatched depth removes scene occlusion. Correct output requires rendered evidence with the game's shaders/constants. Route measurements and remaining limitations are in [AC7 UI composition](../../docs/research/ac7-ui-composition.md) and [UI extraction](../../docs/research/ac7-ui-extraction.md).

Direct draw diversion remains available, but the AC7 measurement on 7 September 2026 showed those quads also composite scene/blur/glow content. Diverting and recompositing at Present skipped engine processing and produced flat/discoloured output. The compatibility correction promotes the engine's interface layers and keeps its own composite. Reusing a promoted layer as an FG guide still requires validation of its alpha/effects conventions; the extraction note records that correction.

## Hook coexistence

Present uses a MinHook detour on the first module-owned function reached through the existing entry jump chain. A separate trampoline built from original module-file bytes supplies the nested-call escape route. This avoids entry patches that Steam/RTSS reassert; see [Windows Present coexistence](../../docs/research/windows-present-hook-coexistence.md).

The stock Windows D3D11 runtime rewrites its immediate-context submission vtable around flush/work calls. Hooks check a Draw sentinel and reapply the family, adopting the current runtime functions as originals. Work calls refresh after forwarding; the Present owner calls `rsf_frame_tap_refresh`. DXVK's table is static. `vtable_refreshes` counts recovery; see [the measurement](../../docs/research/d3d11-runtime-vtable-rewrite.md).

Install/rollback/teardown must account for producers already inside callbacks. Current gaps are recorded in [the review](../../docs/review.md).

## Recorded validation

Earlier compatibility records cover synthetic decode/texture checks, observer and DLSS debug game runs, and cross-build/Wine/DXVK tests of the render-target watch, substitution plan, state restoration and former `scene_reinsert` plan. Those tests queried actual bindings; they did not establish image quality for the heuristic substitution route. Input recognition and picture correctness require evidence from the game's shaders, constants and frame.

The live runtime now connects the overlay, and the native AC7 renderer has acceptance for specific deployed SR/UI/cloud and DLSS-G paths. Earlier tracker entries describing unconnected overlay pieces or pending compatibility rendering apply to their dated increments. Current limits are in [current status](../../docs/current-status.md); the dated work remains in [the tracker](../../docs/implementation.md). This documentation reconciliation adds no new runtime validation.
