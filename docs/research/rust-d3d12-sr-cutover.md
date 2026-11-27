# Generic D3D12 SR Rust cutover

The baseline is `d8f4e6a` on `rust/orchestrator-anomaly`. The question was whether the generic D3D12 SR owner behind `native_sr_d3d12.h` could move to Rust without changing its callers, its kernel or the frame generation handoff recorded in the [D3D12 FG cutover](rust-d3d12-fg-cutover.md). This was a source port with synthetic contracts. No device, window or game ran.

## Ownership after the move

| Concern | Owner and evidence |
| --- | --- |
| Admission and vendor choice | `sr12.rs` copies the declared setup prefix and keeps the ABI 1/2 rules: short structures, unknown backends, quality above 5, missing devices and invalid engine extents are refused before any platform call. DLSS uses the registered, shared or plain context in that order. FSR and XeSS open an `rsf_sr_session` with the same HDR and auto-exposure inputs. |
| Resources | Rust holds the device, root signature, pipeline, three descriptor heaps, the normalized colour, depth and motion textures, the staging output, the exposure textures and the diagnostic readbacks through `ComRef`. Vendor contexts are destroyed before those resources, as in the C++ owner. |
| Validation and history | The extent, format and device checks and their per-generation cache are unchanged. A consecutive native frame with the same history key, session and generation continues history; anything else, a refused evaluation or a reset flag resets it. |
| Normalization | `sr12_platform.cpp` keeps the `cs_5_0` kernel and root layout byte for byte. Rust writes the same descriptors (inputs at `t0..t3`, outputs at `u0..u3`) and the same six root constants, including `-render_extent * motion_to_uv`, and dispatches 8x8 groups. |
| Frame generation handoff | After an accepted evaluation Rust binds the FG12 platform table and calls `fg12::handoff_inputs` with the copied frame record and the normalized depth and motion in `NON_PIXEL_SHADER_RESOURCE`. The copies are recorded on the evaluation list after the vendor read and before sharpening or reinsertion. FG12 holds them until the producer's submission confirmation and queue retirement, unchanged from the FG cutover. A refused evaluation hands nothing over. |
| Reinsertion and capture | The private output reaches the engine target only on accepted evaluation. Capture roles, slots, previews, the per-slot flush at entry and the full flush at destroy are unchanged. `capture_scene` still requires the accepted slot's session, frame and command list. |
| Platform leaves | Each leaf in `sr12_platform.cpp` performs one D3D12, vendor, capture, sharpening or dump call. The nine public `rsf_sr12_*` entries remain native forwarders so the Runtime DLL and the embedded consumers keep the same exported names, ordinals and decorations. |

## Changed behavior

- The motion diagnostic, active only with a log callback, copied 8x8 boxes at the quarter points for render extents of 16 to 31 pixels, where the far boxes crossed the texture edge. It now requires 32 pixels on each axis. `format_and_extent_rules` checks the boxes for accepted extents.
- A vendor that reports a successful create without a context now fails with `RSF_BACKEND_ERROR_INIT_FAILED`. The C++ owner would have evaluated through a null context. No current vendor does this.
- The per-evaluation state is held under a mutex, so `capture_scene` no longer reads it while another thread evaluates. Sharpening options keep their own lock and `present` still takes neither.

## Verification

Commands ran from the worktree with Visual Studio 2026 Debug. The build directory was configured with `cmake -S . -B build/sr12 -G "Visual Studio 18 2026" -A x64 -DBUILD_TESTING=ON` and the vendor and reference include paths of the existing local checkout.

- Baseline at `d8f4e6a`: built `rsf_orchestrator`, `rsf_proxy_ac7`, `rsf_proxy_dinput8`, `rsf_unity_package`, `rsf_godot`, `rsf_minecraft`, `rsf_game_unity_mono` and the fixtures below. `ctest -C Debug -R '^(fg12_abi|fg12_abi_embedded|gpu_sr_policy|gpu_sr_policy_embedded)$'` passed 4 of 4. `cargo test --locked -p rsf-orchestrator` passed 43.
- After the change: the same targets plus `rsf_sr12_abi_test` and `rsf_sr12_abi_embedded_test` built. The same CTest selection with `sr12_abi` and `sr12_abi_embedded` passed 6 of 6. The new fixture checks setup refusals and null-handle refusals for the DLL and embedded linkage without creating a device.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` passed. `rsf-orchestrator` ran 54 tests, 11 of them new SR12 contracts with a recording platform and reference-counted fake COM objects. They cover admission, dispatch constants, the order of normalization, vendor evaluation, FG handoff, sharpening and reinsertion, history resets, the validation cache, external and unit exposure, inputs-only owners, the diagnostic readback and teardown order.
- `dumpbin /exports` on `ReScaleFrame.Runtime.dll`, both `dinput8.dll` carriers, `ReScaleFrame.Loader.dll`, `ReScaleFrame.Unity.Native.dll`, `ReScaleFrame.Godot.Native.dll`, `ReScaleFrame.Minecraft.dll` and `ReScaleFrame.Game.UnityMono.dll` matched the baseline in every ordinal and name. Each binary except the Unity Mono adapter exports the nine `rsf_sr12_*` entries.
- Built only, not run: `rsf_unity_sr_d3d12_test`, `rsf_exposure_normalization_test` and `rsf_fg_present_bridge_test` need a D3D12 device.

Unverified: real vendor evaluation, descriptor and barrier validity under the D3D12 debug layer, sharpening on a device, the handoff into live frame generation, the GL, Unity and Godot callers on hardware, and any game. The full `eng/verify.ps1` suite was not run.
