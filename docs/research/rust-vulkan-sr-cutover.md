# Vulkan super-resolution Rust cutover

The baseline is `c83715f`. This source port moves the `rsf_srvk` owner for FSR2/3 into the existing Rust runtime. It preserves SRVK ABI 2 and the seven exports used by DLL and static archive callers. DLSS and XeSS still use their existing provider implementations. FSR4 still uses its split owner. Ordinary and split SR share `vulkan_sr_split::normalize_frame`, including support for the older ABI 2 frame prefix without the bias mask.

## Ownership and execution

| Boundary | Owner |
| --- | --- |
| Public entry points | `native_sr_vulkan.cpp` forwards to Rust and supplies a private native function table. The existing CMake feature gates select the table. Without Vulkan FidelityFX headers, valid create requests return `NOT_COMPILED`, and valid spatial recovery requests return `NOT_SUPPORTED`. |
| Setup and lifetime | `vulkan_sr.rs` owns the handle, provider selection and sizing, staging allocations, the device dispatch lease, partial initialization cleanup and destroy retry. For FSR2/3 it selects the newest parsed minor/patch for the requested major, checks the effective opaque ID, and preserves FSR2 native AA's output-sized plan. |
| Frame execution | Rust validates camera and image leases, initializes the zero mask and private output once, computes temporal reset and capture ordering, and publishes output after accepted dispatch. A failed dispatch invalidates history and leaves the engine output for the caller's existing spatial recovery. One logged sharpening failure keeps vendor output usable. |
| Native work | The C++ leaves resolve Windows DLL and Vulkan entry points, marshal FFX descriptors, allocate Vulkan resources, issue barriers, clears and copies, invoke SDKs, run the existing Vulkan sharpening pass and make Vulkan readbacks. The GPL sharpening option/pass implementation remains behind the leaf. |
| Recovery | Rust admits only distinct same-device RGBA16F images with usable dimensions, the specified layouts and source transfer usage. It checks the driver's blit-source, blit-destination and linear-filter capability bits before the native leaf records its barriers and linear blit. The source returns to shader read and the output stays transfer-destination. |

The runtime records onto caller-supplied command buffers in the existing render graph. The caller submits in recording order, including a failed SDK recording, or retires the owner. Provider code reuses the private output only under this ordering contract. The alternate DLSS and XeSS staging and sharpening leases remain with their respective providers.

## Render inputs and downstream consumers

Source review used the [Godot dependency lock](../../integrations/godot/dependencies.json), the `ed1daf0bf001b61586d9930840f2f1394092c079` engine checkout with the RSF patch applied, and the Vulkan FidelityFX API headers at `c6efa6bf7f2027b3ec94f28578bb5965eabb9e55`. The [engine integration evidence](godot-engine-integration.md) records the real graph and captures, including the earlier storage-image fixes and moving-geometry failures.

| Input or pass | Producer and existing binding | Cutover |
| --- | --- | --- |
| Scene colour | Godot Forward+ internal colour. The graph leases single-view render-size RGBA16F in shader-read layout. FFX reads HDR linear scene colour, and its automatic exposure remains enabled. | Same image, format, dispatch colour input and transfer-source check for capture or fallback. |
| Depth and motion | Godot's `resolve_motion` supplies render-size R32F depth and dense RG16F UV motion. The adapter supplies signed motion-to-pixel scale, render-pixel jitter, inverted depth, finite near/far and vertical FOV in radians. Generic FFX callers can also lease D32F. | Same format, scale, depth flags and SDK descriptors. Frame, view, session and generation discontinuities reset history. |
| Reactive, composition and bias | Optional render-size R8 masks from the caller. Godot's current packet has colour, depth, motion and output. | FFX retains separate reactive/composition slots and an initialized zero mask for an omitted slot. DLSS alone consumes bias. |
| Sharpening | The existing Vulkan compute pass can sample motion, depth and optional sampled 1x1 floating exposure. | Same constants, SPIR-V, optional inputs, scratch image and per-command descriptor/UBO retirement. FFX vendor sharpness is supplied separately. |
| Reinsertion and later rendering | FFX writes private RGBA16F in GENERAL. The successful SR result reaches Godot's upscaled graph texture through a transfer copy. The failed path uses spatial reconstruction with the leased source. Later passes and FG/Present use the completed image through their existing engine and presentation owners. | Same barriers and output ownership. No shaders, variants, graph bindings, motion producers or presentation callbacks changed. |

This verifies the service-side bindings by source review. The previously recorded broader transparent, particle, skinned and occlusion-heavy coverage gaps, motion-image failures and render-path limits still apply.

## Capture retirement

The shared `FrameCaptureVk` copy and the Rust frame dump owner still handle Vulkan staging, reservation and encoding. The FSR2/3 mode tags each readback with its actual command buffer and records a Vulkan event after the image-to-buffer copy and host barrier. On `rsf_srvk_retire_commands` it only considers readbacks on that command buffer. In the patched engine, `RenderingDevice::_begin_frame` calls `_stall_for_frame` before reset and waits there for the frame fence. `RenderingDeviceDriverVulkan::command_pool_reset` forwards each buffer after a successful `vkResetCommandPool` through `RSFGodot::vk_recording_begin` and `rsfg_vk_recording_begin` to the runtime. The direct fixture follows its fence wait and reset with the same retirement call. Godot drains prior device work before destroying the context.

The fence or drain authorizes freeing the readback. An event in `VK_EVENT_SET` confirms that the copy actually ran, including when an unsubmitted recording is abandoned at reset or destruction. Only an executed copy is mapped. The path does not credit other captures on the device or use a CPU frame ID as GPU completion. Legacy Vulkan capture row pitch is derived from the actual packed byte count, including R8 masks. Capture paths outside this owner retain the shared submission mode.

## Verification

- Scoped VS2026 x64 Release built the Vulkan archive, Runtime DLL, Godot native DLL, AC7 carrier, shared loader, Minecraft DLL and linked/DLL ABI contracts. Device fixtures were built only, including the FSR2/3 and DLSS modes of `rsf_native_vulkan_test`, `rsf_native_xess_vulkan_test`, `rsf_input_dump_vulkan` and the D3D11/WARP `rsf_input_dump_test`. The build used the staged pinned headers. The corrected Streamline Vulkan package receipt was absent, so the DLSS Vulkan provider used the build-time stub.
- Debug and Release each passed seven CPU cases: `native_vulkan_contract`, `native_vulkan_dll_exports`, `native_sr_split_vulkan_contract`, `native_sr_split_vulkan_disabled`, `native_sr_split_vulkan_stub`, `native_xess_vulkan_contract` and `native_xess_input_range_contract`. Release with `RSF_FFX_VK_INCLUDE` empty passed the linked SR contract and DLL export check. The linked SR contract simulates Vulkan events, checks exact command-buffer retirement and verifies that an abandoned copy is never mapped.
- Release `dumpbin /exports` matched every name and ordinal in `build/rust-migration/baseline-exports` for Runtime (381), AC7 (256), Loader (273) and Minecraft (267). The Runtime built without ordinary Vulkan SR matched all 381 pairs as well. Godot has 398 names including all seven linked SRVK exports. The baseline has no Godot export inventory. `dumpbin /imports` found the Runtime import on the split DLL fixture and confirmed static linkage for the ordinary SR fixture and Godot.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` passed. The orchestrator has 105 passing tests, four new. The new tests exercise FFX provider selection, partial creation cleanup, native AA sizing, failed dispatch reset and output refusal, destroy retry, delegated provider routing and spatial fallback admission.
- The first isolated configure received a yyjson file whose hash differed from the repository lock. Using the already staged checkout's hash-matched yyjson files resolved it.

Validation covers source, builds, ABI and CPU fixtures. GPU validation and current game image acceptance remain open.
