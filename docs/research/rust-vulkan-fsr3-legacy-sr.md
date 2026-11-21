# Standalone FSR3 Vulkan bridge: binding and SR in Rust

The baseline is `c509a09b`. The standalone GNU bridge `VulkanStoryFsr3.dll`
(`runtime/vulkan/fsr3/bridge.cpp`) now keeps its SDK runtime binding and its
super-resolution context in `runtime/vulkan/fsr3/owner.rs`. All 21 exports keep
their names and signatures. Frame generation and the SDK swapchain proxy stay
native in the same DLL and read the binding that Rust retains.

This bridge is separate from the integrated FidelityFX Vulkan owners in the
Runtime ([FG cutover](rust-vulkan-ffx-fg-cutover.md),
[WSI owner](rust-vulkan-ffx-wsi-owner.md)). Those use
`native_fg_vulkan.cpp` and the corrected SDK's `wsi-dispatch.h`; this bridge
includes neither.

## Consumer trace

Source review only. `runtime/managed/ReScaleFrame.Vulkan/Upscale/Fsr3Native.cs`
loads `amd_fidelityfx_vk.dll`, then `VulkanStoryFsr3.dll`, resolves 20 of the
21 exports (all but the legacy void `SwapchainDestroyChain`) and calls `VulkanStoryFsr3Open(sdk)` under its load lock. Any nonzero
result frees both modules. A success is cached for the process, so both modules
stay loaded for deferred destruction. Three managed consumers share that cache:
`Fsr3Backend` (SR), `Fsr3FrameGeneration` and `Fsr3SwapchainRuntime`.

`Fsr3Backend` treats every nonzero Plan, Create or dispatch result as a logged
failure. It creates a context per plan with `Create(logical, physical, render
size, display size, &handle)` from a zeroed slot. It records dispatch through
`IUpscalerDevice.EvaluateFsr3`, and `Fsr3Context.Dispose` calls `Destroy` after
GPU-safe retirement. A nonzero `Destroy` keeps the handle and makes cleanup
terminal. The managed code never reads through the context pointer.

`runtime/vulkan/fsr3/build.ps1` is the only build recipe for this DLL. No
CMake target, `eng/` script or packager invokes it. `build-sdk.ps1`, the
`sdk-fixes` manifests and the Godot build receipts hash other files, not
`bridge.cpp` or `build.ps1`, and are unchanged.

## Ownership after the move

| Concern | Owner |
| --- | --- |
| Runtime binding | Rust. `VulkanStoryFsr3Open` still returns -1 for a null module, 0 for the bound module and -2 for another module or any missing `ffxCreateContext`, `ffxDestroyContext`, `ffxQuery`, `ffxDispatch` or `ffxConfigure` export. Missing exports leave the bridge unbound. The module stays caller-owned, with no reference acquired or released. The table is a set-once `OnceLock`. The native `resolve` leaf is `GetProcAddress`. |
| Shared SDK availability | `rsf_core_fsr3_sdk()` returns the bound table, or an all-null table before Open. The native FG and WSI code reads it through `Sdk()` everywhere it previously read the file-scope function pointers, so each null check and call site sees the same values. |
| Plan | Rust refuses with -1 without a bound query or either output slot. The native leaf builds `ffxQueryDescUpscaleGetRenderResolutionFromQualityMode` with a null context and returns the SDK result. |
| Create | Rust checks prerequisites (-1: unbound create, null result, device or physical device, any zero extent), allocation (-2) and Vulkan readiness (-3), then calls the native create leaf. It releases every partial owner on failure and writes `result` only on success. |
| Vulkan lookup | Native. `BindVulkanLoader` rebinds the shared `vkGetDeviceProcAddr` from the loaded `vulkan-1.dll`. `Fsr3GetDeviceProcAddr` keeps the fallback from `vkGetBufferMemoryRequirements2KHR` to the promoted core entry. SR create reports both facts as bits, and Rust requires both. FG create uses the same helper and its existing -3. |
| SDK descriptors | Native and unchanged: `FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE` with the `HIGH_DYNAMIC_RANGE` and `AUTO_EXPOSURE` flags, independent max render and upscale sizes, a chained `BACKEND_VK` descriptor with device, physical device and the fallback resolver. The capsule still owns the descriptors for the effect's lifetime. |
| Evaluate | Rust returns -1 for a null context, null frame or unbound dispatch. It returns -2 unless color and output are RGBA16F, depth D32F, motion RG16F and both masks R8_UNORM. It picks the frame time: `deltaMs` when greater than zero, otherwise 16.667 ms, NaN included. The native leaf maps the six images with the shared `Resource` conversion, including `reactive` and `transparencyAndComposition`, then dispatches with motion scale (1, 1), render size from color, upscale size from output, pre-exposure 1, the reset flag, camera near/far/FOV and meters factor 1. |
| Checked retirement | Rust. Null returns 0. A live effect without a destroy export returns -1. A nonzero SDK release returns its code and keeps the context and capsule attached, so the managed terminal path can report it. Only success releases the capsule and the Rust owner. |

The private table is `rsf_fsr3sr_platform` (ABI 1, 80 bytes, nine entries) in
`bridge_platform.h`, which now also holds `VulkanStoryFsr3Image` and
`VulkanStoryFsr3Frame`. Rust and C++ assert the same sizes and offsets: image
24, frame 184 with composition at 128 and jitter at 152, SDK table 48, create
request 32 and the platform entry offsets. The frame-generation record keeps its
248-byte assertion in `bridge.cpp`.

## Remaining native owners

These exports keep their C++ bodies and only read the shared table:

- WSI: `VulkanStoryFsr3SwapchainCreate`, `SwapchainHandle`, `SwapchainRecreate`,
  `SwapchainGetImages`, `SwapchainAcquire`, `SwapchainPresent`,
  `SwapchainPrepareDestroy`, `SwapchainDestroyChainChecked`, the legacy void
  `SwapchainDestroyChain`, `SwapchainLastPresentCount` and `SwapchainDestroy`.
  These cover the four-queue proxy creation, the replacement function table, the
  wait-for-presents drain and checked chain and context release.
- FG: `VulkanStoryFsr3FgCreate`, `FgEvaluate`, `FgEvaluateDirect`, `FgDisable`,
  `FgDestroy` and the `Fsr3GenerateOnPresent` callback. These cover the HUD-less
  RGBA descriptor, direct versus proxy interpolation, UI registration, the
  disable/drain/clear ordering and the cross-links between FG and swapchain
  owners.

FG and WSI are interlinked through `activeFg` and the swapchain pointer. They
should move together in a later slice.

## Linking

`build.ps1` follows the standalone XeSS-FG recipe. From the repository root,
which selects the pinned toolchain, it compiles `owner.rs` alone with
`rustc --edition=2024 --crate-type staticlib --target x86_64-pc-windows-gnu
-C panic=abort -D unsafe_op_in_unsafe_fn` into a GUID-named sibling archive.
It then links `bridge.cpp` (still C++17, `-static -Wall -Wextra`) with that
archive and the Rust std system libraries into a GUID-named temporary DLL. The
target moves into place only after both steps succeed. A `finally` block removes
the temporary archive and DLL on every path, so a failed compile or link leaves
the previous DLL untouched.

The `rsf_core_fsr3_*` symbols come from the Rust archive, as the
`rsf_core_xess_*` symbols do in the XeSS bridge. Whether MinGW also exports them
next to the 21 explicit `dllexport` entries was not checked. Managed code
resolves only the named exports.

`runtime/backends/fsr/compat/src/lib.rs` includes `owner.rs` under
`#[cfg(test)]` only. Its CPU fixture then runs under `cargo test` and Clippy
`--all-targets`, and the `rsf-fsr4-compat` archive does not change. The fixture
covers unbound refusals, a missing export, a null private table, same and other
modules, plan outputs, every create refusal with partial-owner release and an
unwritten slot, format refusal, the frame-time default including NaN, and a
failed and then successful checked destroy.

## Deliberate differences

- SR creation now allocates twice: the native capsule and the Rust owner. Either
  failure returns -2, as the single allocation did. The exported context pointer
  now addresses the Rust owner, which managed code treats as opaque.
- Open publishes the table once through `OnceLock`. Concurrent opens are now
  well defined; the loser reports against the winning module.
- An invalid private table returns -1 from Open, Plan and Create. The shipped
  table is static, so this path cannot be reached.
- Resolution stops at the first missing export instead of resolving all five
  first. `GetProcAddress` has no other effects, and the result is still -2.

## Coverage scope

This moves ownership without changing GPU work. The six SR resources, formats,
SDK resource states, descriptor values and dispatch constants are unchanged. The
producers and composition masks keep the coverage and limits recorded in
[the Godot integration record](godot-engine-integration.md) and the
[line-width record](fidelityfx-vulkan-line-width.md). No shader, buffer or
producer changed. Active-permutation and moving-scene validation for this owner
remain open.

## Verification

Source comparison against `c509a09b` covered all 21 export signatures (a
normalized text comparison found them identical) and return paths, the five SR exports' refusal order, descriptor values and the
FG/WSI call sites now reading `Sdk()`. Pinned Rust 1.94.1 `rustfmt --check`,
the PowerShell parser for `build.ps1` and `git diff --check` passed. Nothing was
compiled, linked or run. The rustc archive, the g++ link, Clippy, `cargo test`,
the DLL's export list and device or game behavior await user verification.
