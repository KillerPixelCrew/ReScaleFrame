# Standalone FSR3 Vulkan bridge: binding, SR, FG and WSI in Rust

The baseline is `c509a09b`. The standalone GNU bridge `VulkanStoryFsr3.dll`
(`runtime/vulkan/fsr3/bridge.cpp`) keeps its SDK runtime binding, its
super-resolution context (`076ca75c`) and now its frame-generation and SDK
swapchain owners in `runtime/vulkan/fsr3/owner.rs`. All 21 exports keep their
names and signatures. The native side keeps the typed FidelityFX effects and
descriptors, the replacement swapchain function table, the Vulkan loader lookup,
the resource mapping and the presentation callback shell.

This bridge is separate from the integrated FidelityFX Vulkan owners in the
Runtime ([FG cutover](rust-vulkan-ffx-fg-cutover.md),
[WSI owner](rust-vulkan-ffx-wsi-owner.md)). Those use
`native_fg_vulkan.cpp` and the corrected SDK's `wsi-dispatch.h`; this bridge
includes neither, and neither file nor any `sdk-fixes` receipt changed.

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

`Fsr3FrameGeneration.Create` calls `FgCreate(logical, physical, width, height,
Fsr3ProxyFormat, &handle)` and treats a nonzero result or a zero handle as a
failure. `Disable(Fsr3ProxyContext)` resets history only on 0, and `Dispose`
throws on a nonzero `FgDestroy` while keeping the handle. Evaluation goes through
`IFrameGenerationDevice.EvaluateFsr3FrameGeneration`, whose implementation and
the `Fsr3ProxyContext` property live in the host outside this repository; the
bridge sees only the 248-byte `Fsr3FgFrame` and its opaque `SwapchainContext`.

`Fsr3SwapchainRuntime.TryCreate` calls `SwapchainCreate` with four queues of one
family and requires 0 and a nonzero owner. It exposes the owner as the opaque
`NativeContext`, routes `Handle`, `Recreate`, `GetImages`, `Acquire` (infinite
timeout), `Present`, `PrepareDestroy`, `DestroyChainChecked` and
`LastPresentCount` through it, and its `CheckRelease` makes every nonzero
preparation, chain release or `SwapchainDestroy` terminal with ownership
retained. No managed code mirrors the owner layouts, so moving both owners
behind opaque Rust pointers changes nothing they observe.

`runtime/vulkan/fsr3/build.ps1` is the only build recipe for this DLL. No
CMake target, `eng/` script or packager invokes it. `build-sdk.ps1`, the
`sdk-fixes` manifests and the Godot build receipts hash other files, not
`bridge.cpp` or `build.ps1`, and are unchanged.

## Ownership after the move

| Concern | Owner |
| --- | --- |
| Runtime binding | Rust. `VulkanStoryFsr3Open` still returns -1 for a null module, 0 for the bound module and -2 for another module or any missing `ffxCreateContext`, `ffxDestroyContext`, `ffxQuery`, `ffxDispatch` or `ffxConfigure` export. Missing exports leave the bridge unbound. The module stays caller-owned, with no reference acquired or released. The table is a set-once `OnceLock`. The native `resolve` leaf is `GetProcAddress`. |
| Shared SDK availability | The bound table, or an all-null table before Open (`rsf_core_fsr3_sdk()` exposes it to the unit tests). Rust reads it for every admission and passes each leaf the entry it calls. Only the native presentation callback reads it itself, through `Sdk()`. |
| SR Plan | Rust refuses with -1 without a bound query or either output slot. The native leaf builds `ffxQueryDescUpscaleGetRenderResolutionFromQualityMode` with a null context and returns the SDK result. |
| SR Create | Rust checks prerequisites (-1: unbound create, null result, device or physical device, any zero extent), allocation (-2) and Vulkan readiness (-3), then calls the native create leaf. It releases every partial owner on failure and writes `result` only on success. |
| Vulkan lookup | Native. `BindVulkanLoader` rebinds the shared `vkGetDeviceProcAddr` from the loaded `vulkan-1.dll`. `Fsr3GetDeviceProcAddr` keeps the fallback from `vkGetBufferMemoryRequirements2KHR` to the promoted core entry. SR create reports both facts as bits and Rust requires both. FG create has its own `fg_bind_vulkan` leaf that only rebinds the loader, as before, and Rust maps its failure to -3. |
| SR descriptors and Evaluate | Native descriptors unchanged. Rust returns -1 for a null context, null frame or unbound dispatch, -2 unless color and output are RGBA16F, depth D32F, motion RG16F and both masks R8_UNORM, and picks the frame time (`deltaMs` when greater than zero, otherwise 16.667 ms, NaN included). |
| SR checked retirement | Rust. Null returns 0. A live effect without a destroy export returns -1. A nonzero SDK release returns its code and keeps the context and capsule attached. |
| Swapchain create | Rust. -1 without bound create, query or destroy, any null device, queue, info or result, or a non-null `oldSwapchain`, with `result` untouched. Otherwise `result` is cleared, then -2 for either allocation, the SDK creation result, or after releasing the SDK context (result ignored, as before) the query result or -3 for an incomplete replacement table. The native leaf builds `ffxCreateContextDescFrameGenerationSwapChainVK` with the four queues, the copied create info with `oldSwapchain` cleared, and the Rust owner's chain slot as `desc.swapchain`. |
| Swapchain pass-throughs | Rust admits, the native leaf calls the replacement entry. `Handle` returns the current chain or null. `Recreate`, `GetImages`, `Acquire` and `Present` return `VK_ERROR_INITIALIZATION_FAILED` (-3) for a null owner or their required pointer, otherwise the proxy result; `Recreate` updates the current chain only on `VK_SUCCESS`. `LastPresentCount` returns 0 without an owner or chain, and the native leaf returns 0 without the optional SDK entry. |
| FG create | Rust. -1 without bound create, a null result, device or physical device, or a zero extent; -2 for either allocation; -3 when the loader does not rebind; or the SDK result. `result` is written only on success. The native leaf fills the interpolation, HUD-less RGBA8 and Vulkan backend descriptors (B8G8R8A8 backbuffer maps to its FFX format, everything else to R8G8B8A8) and creates the effect. |
| FG evaluate | Rust. -1 for a null context or frame or without bound dispatch and configure. -2 unless color and UI are RGBA8, depth D32F, motion RG16F and commands are present, then for direct frames an RGBA8 output image matching the context extent, or for proxy frames a swapchain owner with a live context and chain. The owner pointer is read only after the format and command checks pass, as the native `\|\|` chain did. A context linked to a different owner (or to one while a direct frame arrives) is disabled first. Then configure, link the pair on proxy success, prepare with the frame-time default, and either generate one direct output or register the frame's UI. `EvaluateDirect` returns -1 for a null frame or a non-null `swapchainContext`, then evaluates. |
| FG pairing | Rust. `FgContext.chain` and `FgContext.swapchain` are set with `Swapchain.active_fg` after a successful proxy configure and cleared together only after a successful disable. The callback context the SDK receives is the native capsule, which lives exactly as long as the Rust FG owner. |
| Checked disable | Rust. 0 for a null or unlinked context. -1 without a live effect or bound configure, or when the stored swapchain is missing, has no context or now holds a different chain. Then configure off, re-check the pairing, wait for presents (-1 without dispatch), clear the UI registration, and only then unlink. The swapchain argument is ignored in favour of the stored association, as before. |
| Swapchain prepare and chain release | Rust. `PrepareDestroy` returns 0 for a null chain, -1 without owner, context or dispatch, disables the active FG only when it is linked to that chain, then waits. `DestroyChainChecked` prepares, returns 0 for a null chain, -1 without the replacement destroy entry, destroys the chain and clears the current chain only when it matches. The legacy void `DestroyChain` stays a native shell over the same Rust function and keeps its `stderr` line. |
| Swapchain and FG retirement | Rust. `SwapchainDestroy` disables any active FG, prepares the current chain, returns -1 for a live context without a destroy export, then releases the SDK context and frees the capsule and owner only on success. `FgDestroy` disables, releases a live effect when the destroy export is bound, and frees on success. Every nonzero step returns its code and keeps the owner. |

The Rust owner no longer has these tables: it calls the leaves in `platform.rs`
directly, and its unit tests compile same-named fakes. In the C++ bridge, two
private tables in `bridge_platform.h` carried the leaves. `rsf_fsr3sr_platform`
is unchanged (ABI 1, 80 bytes, nine entries). `rsf_fsr3fg_platform` is new (ABI 1,
208 bytes, 25 entries: 15 swapchain and 10 interpolation). The header now also
holds `VulkanStoryFsr3FgFrame` and the two request records. Rust and C++ assert the
same sizes and offsets: FG frame 248 with `deltaMs` at 136, `frameId` 152, `ui`
216 and `swapchainContext` 240; swapchain request 64 with `info` at 48 and the
queue family at 56; FG request 32 with the format at 24; table offsets 8, 80,
128 and 200. C++ also asserts `VkSwapchainCreateInfoKHR` is 104 bytes with
`oldSwapchain` at 96, the offset Rust reads for the creation refusal.

## Checked-lifetime traces

Each trace lists the old native sequence and the new Rust sequence; they match
step for step. "Kept" means the owner and every link stay attached.

| Path | Sequence (old and new) | On failure |
| --- | --- | --- |
| Swapchain create | prerequisites, clear `result`, allocate, SDK create, query, completeness check, write `result` | create failure frees the owner (no context exists); query failure or an incomplete table releases the SDK context, ignores that result and frees the owner |
| FG create | prerequisites, allocate, rebind loader, SDK create, write `result` | every failure frees the owner and leaves `result` unwritten |
| Proxy evaluate | admit, disable a different link, configure (proxy callback), link, prepare, register UI | configure failure leaves the context unlinked from the new owner; a later failure keeps the new link |
| Direct evaluate | admit, disable any link, configure (no swapchain notify), prepare, generate | a failed disable returns before configure |
| FG disable | configure off, wait for presents, clear UI, unlink | each failure returns its code and keeps the link, so tagged inputs and the effect stay live |
| Prepare destroy | disable the FG linked to this chain, wait for presents | a failed disable or wait returns its code; links kept |
| Chain release | prepare, destroy chain, clear the current chain if it matches | a failed preparation keeps the chain and links |
| Swapchain destroy | disable the active FG, prepare the current chain, release the SDK context, free | each failure returns its code; owner kept |
| FG destroy | disable, release the effect, free | each failure returns its code; owner kept |

Unchanged edge cases, kept on purpose: `FgDestroy` with a live effect and no
destroy export frees the owner without releasing the effect (unreachable after a
successful Open, which binds that export); a swapchain whose chain was recreated
under a linked FG makes `FgDisable` and `FgDestroy` refuse with -1 until a proxy
evaluate relinks to the new chain; and if a second FG context were evaluated on
an owner, `active_fg` would move to it while the first keeps its pointer. The
managed session holds one FG context at a time, so that last case is not reached.

## Linking

`build.ps1` follows the standalone XeSS-FG recipe. From the repository root,
which selects the pinned toolchain, it compiles `owner.rs` alone with
`rustc --edition=2024 --crate-type staticlib --target x86_64-pc-windows-gnu
-C panic=abort -D unsafe_op_in_unsafe_fn` into a GUID-named sibling archive.
It then links `bridge.cpp` (still C++17, `-static -Wall -Wextra`) with that
archive and the Rust std system libraries into a GUID-named temporary DLL. The
target moves into place only after both steps succeed. A `finally` block removes
the temporary archive and DLL on every path, so a failed compile or link leaves
the previous DLL untouched. This slice changed only the recipe's description.

The `rsf_core_fsr3_*` symbols come from the Rust archive, as the
`rsf_core_xess_*` symbols do in the XeSS bridge. Whether MinGW also exports them
next to the 21 explicit `dllexport` entries was not checked. Managed code
resolves only the named exports.

`runtime/backends/fsr/compat/src/lib.rs` includes `owner.rs` under
`#[cfg(test)]` only. Its CPU fixture then runs under `cargo test` and Clippy
`--all-targets`, and the `rsf-fsr4-compat` archive does not change. The fixture
is one sequence because the SDK binding is process-wide. For SR it covers unbound
refusals, a missing export, a null private table, same and other modules, plan
outputs, every create refusal with partial-owner release and an unwritten slot,
format refusal, the frame-time default including NaN, and a failed and then
successful checked destroy. For FG and WSI, fake leaves log every SDK-facing call
and can fail one call by name. It covers unbound and null-owner results with no
leaf call; swapchain refusals that leave `result` untouched (`oldSwapchain`, a
null queue, a null table); create, query and incomplete-table failures with the
context release and partial-owner release; recreate success and failure; the
pass-through refusals; FG create refusals including loader failure; direct and
proxy admission, the direct-only export and the frame-time default; the exact
call order for proxy evaluate, disable, prepare, chain release and retirement;
link retention after a failed wait or UI clear; a direct frame disabling a proxy
link first; the stale-chain refusal after recreate; and retained owners after a
failed SDK release followed by a successful retry.

## Deliberate differences

- SR, FG and swapchain creation now allocate twice: the native capsule and the
  Rust owner. Either failure returns -2, as the single allocation did. The
  exported pointers now address the Rust owners, which managed code treats as
  opaque.
- The presentation callback's user context is the FG native capsule rather than
  the whole FG owner. It dispatches the same effect and lives as long.
- FG create now rebinds the Vulkan loader before filling descriptors instead of
  after. Filling descriptors has no external effect, so the SDK sees the same
  call.
- Open publishes the table once through `OnceLock`. Concurrent opens are now
  well defined; the loser reports against the winning module.
- An invalid private table returns -1 from Open, Plan, Create, SwapchainCreate
  (before `result` is cleared) and FgCreate. The shipped tables are static, so
  this path cannot be reached.
- Resolution stops at the first missing export instead of resolving all five
  first. `GetProcAddress` has no other effects, and the result is still -2.

## Coverage scope

This moves ownership without changing GPU work. The SR and FG resources,
formats, SDK resource states, descriptor values, dispatch constants, the
upright-motion vertical negation, the sRGB transfer function, the UI composition
flags and the four-queue proxy descriptor are unchanged. The producers and
composition masks keep the coverage and limits recorded in
[the Godot integration record](godot-engine-integration.md) and the
[line-width record](fidelityfx-vulkan-line-width.md). No shader, buffer or
producer changed. Active-permutation and moving-scene validation for this bridge
remain open.

## Verification

Source review only for the FG/WSI slice. A normalized comparison of the 21
`dllexport` definitions in `bridge.cpp` against both `HEAD` (`a5867f2f`) and
`c509a09b` found them identical; the old forward declaration of `FgDisable` is
gone. The native leaves were compared line by line with the removed FG and WSI
bodies for descriptor fields, constants and SDK call targets, and each Rust
path above was traced against the removed admission, ordering and retention
logic, including short-circuit order. The 15 new `rsf_core_fsr3_*` prototypes
and 25 leaf types were checked against the Rust signatures. Pinned Rust 1.94.1
`rustfmt --check` for `owner.rs` and the compat `lib.rs`, the PowerShell parser
for `build.ps1`, `cargo metadata --locked --offline` and `git diff --check`
passed. Nothing was compiled, linked or run. The rustc archive, the g++ link,
Clippy, `cargo test` (including the extended fixture), the DLL's export list
and device or game behavior await user verification.

The earlier note that FG and WSI were interlinked through `activeFg` and the
swapchain pointer and should move together remains correct; this slice moved
them together as one ownership boundary.

## Later change (2026-11-24)

`exports.rs` is gone. The 17 `rsf_core_fsr3_*` functions that its exports only
forwarded now carry the exported `rsf_vk_fsr3_*` names and signatures in
`owner.rs` (the `fsr3.def` names are unchanged). The three create entries keep
a separate exported function that packs its scalar arguments into the request
record for `create_sr`, `create_swapchain` and `create_fg`, and the void
`rsf_vk_fsr3_swapchain_destroy_chain` still logs a refusal from the checked
variant. The platform probes (`has_effect`, `wsi_complete`, `wsi_has_context`,
`fg_has_effect`, `fg_bind_vulkan`, `bind_vulkan`) return bool; a missing loader
or buffer-requirements entry still refuses SR creation with -3. All of
`platform.rs` is Rust, so "native leaf" above means that module.
