# Shared D3D11 translucency owner Rust cutover

The baseline is `ed0c6b3` on `rust/orchestrator-anomaly`. The question was whether the shared translucency and cloud-depth owner behind `native_translucency.h` could move to Rust without changing its callers or its three exported entries, and whether its rectangle-only cloud association and failure-state races could be closed with facts the adapter already reports. This was a source port with CPU contracts, ABI fixtures and built device fixtures. No device fixture, window or game ran.

## Callers and surface

- `loader/proxy/src/dlss_bridge.c` forwards `RSF_GAME_RENDER_TRANSLUCENCY`, `CLOUD_DEPTH` and `MATERIALS` passes to `rsf_native_translucency_pass` on its immediate context while the bridge is started.
- `native_sr.cpp` reaches `rsf_native_translucency_take` through the SR service table and calls `rsf_native_translucency_release` from `rsf_native_sr_release_resources`. `native_sr.rs` copies the masks into `rsf_dlss_pipeline_frame` (`color_before_transparency`, `transparency_layer`, `reactive_mask`, `transparency_mask`, `bias_mask`, `motion_depth_layer`).
- The only producer is AC7. `games/ac7/src/native_renderer.cpp` builds the translucency and materials scopes in `primary_scope` (session, family, view, native frame from family +0x68, render and output rectangles, mip bias) and the cloud pass in `native_cloud_composite_draw` (session, depth, viewport rectangle; no family, view or frame).
- `tests/native_translucency.cpp` is the WARP device fixture.

## Ownership after the move

| Concern | Owner |
| --- | --- |
| Frame identity and generation | `native_translucency.rs` `Ledger`. A first scene-colour begin opens a new generation keyed by family, view, native frame and render rectangle. End-of-pass masks, layer merges and the SR handover are admitted only for the open frame's key, in that order. |
| Producer identity | The opaque snapshot comes from the opening scene colour, the layer from the leased `color_output` of a layer pass of the same key, and cloud depth from a composite tagged with the adapter's session and viewport rectangle. |
| Resource leases | Owned COM references (`com_ref`) for the kernel, the snapshot and three R32 masks, the layer copy and two cloud slots. Game textures are borrowed for the call; their SRVs are cached with a retained texture through `native_regions::SourceView`. Masks handed to SR stay borrowed until the next scope or release, as the header states. |
| Device ownership | Every scene, layer and cloud texture must belong to the device of the caller's immediate context, compared by `IUnknown` identity. A different device retires all owned resources. |
| Exclusive use | `Exclusive<Owner>` takes the state out of its mutex for one call. Reentry or a concurrent call is refused; a release during a call is applied when that call ends. No lock is held across driver calls. |
| Material mip bias | Rust decides the value and when (on after the opening snapshot and for the base pass, off before the end-of-pass masks). The frame tap remains the sampler bias owner: `rsf_frame_tap_set_sampler_bias` observes `PSSetSamplers` on the hooked context. `sampler_bias.rs` needs the caller's bound sampler baselines, which this path does not have. |
| Native leaves | `native_translucency.cpp` keeps the HLSL literal, its compile (`compile_compute`, no strictness), the shared `rsf_d3d11_state` scope and the frame tap call, passed to Rust as a four-entry table. `native_translucency_com.cpp` creates the sampler, clears predication and binds and dispatches. Texture, SRV, UAV, constant buffer, crop, copy and identity calls reuse `d3d11_output_platform.cpp` and `native_regions_com.cpp`. |

No game or engine policy entered the runtime. The cloud association uses only the session and rectangle the adapter reports.

## Coverage of the handed-over inputs

| Input | Producer | Consumer | Change |
| --- | --- | --- | --- |
| Colour before transparency | Region copy of scene colour at the first scene-colour begin | DLSS `ColorBeforeTransparency` | none |
| Transparency layer | Region copy of a scene-sized layer after the layer merge | DLSS `TransparencyLayer` | none |
| Reactive, coverage, bias | The unchanged compute kernel, mode 0 at the scene pass end, mode 1 at the layer end | FSR reactive and T&C, XeSS responsive mask, DLSS transparency and bias hints | Refused after the view's SR pass |
| Cloud depth | Copy of the R32 composite depth | Motion resolve of pixels without object velocity | Session match, late refusal, double buffer |

## Changed behavior

The cloud handover was associated by rectangle alone, and a composite could be adopted by a frame it did not belong to. The research in [ac7-motion-depth-20261004.md](ac7-motion-depth-20261004.md) measured TrueSky's composite in the post-opaque extension, before the first translucency pass of the same frame. These rules follow from that order and from the facts each pass carries:

- A cloud composite is adopted only when its session equals the translucency scope's session and its rectangle equals the scope's render rectangle. AC7 fills both sessions from the same host session id.
- A composite that arrives after a frame's first scene-colour begin and before an SR pass of that view asks for the frame is refused as late. Before, the next frame adopted it when the rectangle matched. After a frame whose SR pass never asks, the next composite is therefore refused and that frame resolves cloud pixels at scene depth.
- Any SR request for the same family and view closes the open frame, whether or not its masks match. Later scene or layer ends for that frame are refused, so masks already borrowed by SR are not rewritten.
- A second scene-colour begin with the same identity before the SR pass keeps the first snapshot. Before, it re-snapshotted an image that already held the first pass's translucency. AC7 calls pass 0 or pass 2, never both, so its measured path is unchanged. After the SR pass, the same identity opens a new generation, so an adapter that does not advance its frame number still gets fresh masks each frame.
- Cloud depth is double-buffered. A new composite is stored in the slot the current frame did not borrow.
- A cloud texture must also have one mip, one layer and one sample, as `CopyResource` into the owned R32 texture requires. Its extent comes from a checked rectangle instead of an unchecked signed subtraction. The cloud pass also requires an immediate context.
- `rsf_native_translucency_take` refuses a short SR pass instead of reading past it.
- Device ownership is compared by `IUnknown` identity instead of the `ID3D11Device` pointer.
- The old catch handler cleared the ready flag without the lock. There is no such path now: leaves contain their own exceptions and no Rust panic crosses the C boundary.
- Concurrent or reentrant calls are refused instead of blocking. AC7 issues all of them on its render thread.

Unchanged: the HLSL, its compile options, the constant layout, the linear clamp sampler, mask and snapshot formats, dispatch size, the full state save and restore with cleared predication and render targets, the compile failure latch per device, the layer format and size rules, the mip-bias policy and the release at module unload.

## Verification

Commands ran from the worktree with Visual Studio 2026 in `build/translucency`, configured with the vendor and reference paths of the coordinator's `build/rust-migration` cache. The pinned yyjson files were copied from the coordinator checkout after the download failed its TLS connection.

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` passed. `rsf-orchestrator` ran 125 tests, 8 of them new translucency contracts: identity and order admission, the repeated begin, refusal after SR and after a failed layer merge, the cloud session and rectangle rules, late composites, the material bias policy, deferred release and the constant layout.
- Debug CTest `native_translucency_abi` (Runtime DLL) and the new `native_translucency_abi_embedded` (the same fixture linked against `ReScaleFrame::OrchestratorCore`) check null and short refusal, ignored context-free passes and release, without a device. With `orchestrator_domain_contract` and `game_plugin_contract` they passed 4 of 4.
- The HLSL literal is byte-identical to `ed0c6b3` (1599 bytes, SHA-256 `2EBD54152B71DC38FF9641B594F8ABED53069AC25BAD4CC35CBCCB2892BCF4D0`), compiled as before with `cs_5_0`, entry `main`, name `translucency_masks` and flags 0.
- `dumpbin /exports` on Release `ReScaleFrame.Runtime.dll` (381), the AC7 `dinput8.dll` (256), the shared `dinput8.dll` loader (273) and `ReScaleFrame.Minecraft.dll` (267) matched the coordinator's `build/rust-migration/baseline-exports` in every ordinal and name, including the three translucency entries (Runtime 287 to 289). The Release `native_translucency_abi` and `native_translucency_abi_embedded` fixtures also passed.
- Built only, not run: `rsf_native_translucency_test`, extended with session mismatch, late composite and borrowed double-buffer cases, and `rsf_sr_pipeline_test`, in Debug and Release.

Unverified: the WARP fixture, AC7 in a game, the effect on FSR and XeSS images, cloud motion in flight, and the full `eng/verify.ps1` suite.
