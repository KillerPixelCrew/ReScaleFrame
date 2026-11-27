# FidelityFX 3 Vulkan WSI route in Rust

Baseline `cf29c84`. This moves the process-wide WSI callback ownership of the
FidelityFX 3 Vulkan frame-generation owner (`rsf_ffxgvk`, FG backend 3) from
the native `std::atomic<rsf_ffxgvk_native*> owner` in `native_fg_vulkan.cpp`
into the existing Rust owner in `vulkan_ffx_fg.rs`. It follows the earlier
[cutover](rust-vulkan-ffx-fg-cutover.md), which left this route native. The
stable `rsf_fgvk_*` and `rsf_ffxgvk_*` entry points, their results, the
disabled-build stubs and the private WSI dispatch ABI are unchanged. Evidence is
source inspection of the pinned SDK and bridge, Rust tests, Windows MSVC builds,
CPU contracts and export comparison. No device, window or game ran.

Sources read: `runtime/vulkan/fsr3/wsi-dispatch.h` (the bridge ABI and its
implementation compiled into the corrected SDK), `sdk-fixes/swapchain-lifetime.json`
(which SDK calls it replaces), and the pinned FidelityFX Vulkan checkout
`references/godot/fidelityfx-vulkan` at `c6efa6bf7f2027b3ec94f28578bb5965eabb9e55`.
SDK line numbers below are from that checkout before the RSF corrections are
applied; the corrections replace the named Vulkan calls in place.

## Uses of the native owner pointer at the baseline

| Use | Caller and thread in the pinned call graph | Baseline behaviour |
| --- | --- | --- |
| `open` | Rust `initialize`, host thread | Compare-exchange claims the pointer before loading anything. That is both the one-owner admission and the WSI route. A failure frees the partial leaf, whose destructor clears the pointer. |
| `device_proc` | `open` itself (barrier, copy, idle entry points), and the SDK backend as `vkDeviceProcAddr` during `create_effect` | Loads the pointer, checks the device, resolves with KHR to core fallbacks. |
| `graphics_submit` | SDK `gameQueue.submit`: `presentInterpolated` (lines 2169, 2232 via `submitCompositionOnGameQueue`), `queuePresentNonInterpolated` and the other `queuePresent` paths (2292, 2335, 2434, 2563, 2687). All run inside `queuePresent` on the presenting thread. | Loads the pointer; a null pointer reports `VK_ERROR_DEVICE_LOST`. |
| `serialized_create` | `rsf_ffx_wsi::create` in `FrameInterpolationSwapChainVK::init` (line 1598), inside `create_swapchain` | Device check, Streamline DLSS-G suppression. |
| `serialized_images` | `init` (lines 1627, 1630), inside `create_swapchain` | Device check, DLSS-G suppression, latency WSI scope. |
| `serialized_destroy` | `ffxReplaceSwapchainForFrameinterpolationVK` (733, the game swapchain) and its failure cleanup (726), and `destroySwapchain` (1764) from `destroy_chain`, the swapchain-context destroy and failed creation | Device check, DLSS-G suppression. |
| `serialized_acquire` | `acquireNextRealImage` (2573): on the presenting thread from `queuePresent` (2653), and on the SDK presenter thread (977, 1146) | Device check, DLSS-G suppression, latency WSI scope, timing. |
| `serialized_present` | `presentToSwapChain` (776): from `queuePresentNonInterpolated` (2555) on the presenting thread, and from the SDK presenter thread | No device (queue only), DLSS-G suppression, latency queue present, timing. |
| Destructor | Native `close`, the last step of Rust retirement | Unregisters (ignoring the result), clears the pointer, unloads the module. |

The SDK generation callback is not on this list. It receives the leaf as its
user context from `configure` and runs synchronously inside `queuePresent`
(the [cutover](rust-vulkan-ffx-fg-cutover.md) records the call path).

## Callback retirement audit

The bridge's `get` copies the table entry under its mutex and calls the hook
after releasing it (`wsi-dispatch.h` lines 22 to 46). Unregistering therefore
never waits for, or proves the end of, a callback that already copied its
entry, and clearing the raw pointer after unregistering would not either. The
proof has to come from the SDK's threads:

- `destroySwapchain` (line 1719) first waits for presents and then calls
  `killPresenterThread` (2731), which sets `shutdown`, signals and waits on the
  interpolation thread handle. That thread waits on its presenter thread before
  it exits (1280 to 1283). Both threads that can call the acquire and present
  hooks are joined before the real swapchain is destroyed.
- Disabling generation (`configure` with generation off) also calls
  `killPresenterThread` (1899). Rust disables before draining when the owner was
  ever available.
- `destroy_chain` calls `vkDestroySwapchainFFXAPI`, which reaches that
  `destroySwapchain`; the swapchain-context destroy (provider `DestroyContext`)
  calls the same replacement destroy when the chain is still present. Failed
  creation cleans up synchronously inside `create_swapchain` (lines 526, 726).
  Presenter threads start only when generation is enabled (1895), which needs an
  available owner.
- The other hooks run on the presenting or creating thread, inside a leaf call
  the Rust owner makes. The host must serialize the `rsf_ffxgvk_*` calls
  (`vulkan_fg.rs`), so they cannot overlap `rsf_ffxgvk_destroy`.

Rust reaches the native close only after the effect, chain and swapchain
context were released. So in the pinned call graph no hook can overlap the
destructor, and no use after free was found. The safety came from those facts
and the host contract, not from the pointer.

A refused unregister cannot leave the hooks registered either. The bridge
refuses with `-1` for a null, short or partial setup and `-2` for another ABI.
The leaf builds neither (Rust refuses a null device). It returns `-3` only when
another device holds the table. That table holds one device, and only a clear
naming this device can remove this registration, so `-3` means these hooks were
already gone. The destructor keeps ignoring the result, as before.

No defect in the baseline was proven, so there is no fix for one. The move still
replaces implicit facts with an enforced guard, below.

## Ownership after the move

| Concern | Owner |
| --- | --- |
| Singleton admission | Rust. A process-wide `Route` in `vulkan_ffx_fg.rs` is claimed in `initialize` before `open`, so a second owner gets `NOT_SUPPORTED` before anything is loaded, as before. |
| Publication | Rust publishes the opened leaf immediately after `open`, before `create_swapchain`, because the SDK calls the create, image and destroy hooks synchronously inside creation and its failure cleanup. |
| Hook borrows | Each hook takes a scoped `Borrow` from `rsf_core_ffxgvk_wsi_borrow` for the whole callback, including the forwarded Vulkan, Streamline and latency calls, and returns it with `rsf_core_ffxgvk_wsi_return`. Device hooks and `device_proc` ask Rust to match the admitted device; the queue hooks (`present`, `graphics_submit`) do not, as before. |
| Closing | After the SDK contexts are destroyed, Rust closes the route: new borrows get null, and while a borrow is active retirement returns `NOT_READY` and keeps the leaf, its registration and the admission. The retry repeats no SDK release. |
| Native close | Native. It unregisters the hooks, frees the leaf and unloads the module. |
| Admission release | Rust, after the native close returns, so a successor cannot register before this owner unregisters. A failed `open` (which registered nothing and freed its own leaf) releases admission through the same path. |
| `open`'s own lookups | The typed leaf `device_proc_for` takes the leaf directly, since the route is not yet published. |

The route lock covers only its fields. It is never held across a native,
Vulkan, Streamline or SDK call, and no hook waits on it beyond that update. A
close attempt does not wait for borrows; it refuses, so a hook that is itself
waiting inside a vendor call cannot deadlock retirement. The borrow is a count,
not a lock.

Unchanged: queue, Streamline suppression and latency call order inside each
hook; every Vulkan result, including `VK_ERROR_INITIALIZATION_FAILED` for an
unrouted device hook and `VK_ERROR_DEVICE_LOST` for an unrouted submit; SDK
completion, capture and timing; and the original `rsf_ffxgvk_ops` layout (23 pointers).
Behaviour differs only when a hook is still borrowing the leaf at retirement,
which the audit found unreachable in the pinned graph: that retirement is now
refused instead of freeing the leaf under the hook.

## Verification

Pinned Rust 1.94.1 from `D:/Coding/ReScaleFrame/.local/rust-1.94.1-verification`,
with a worktree-local Cargo target and CMake tree. yyjson was copied from the
main checkout after checking all three SHA-256 values in `yyjson.lock.json`.

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked
  -- -D warnings` and `cargo test --workspace --locked`: 373 tests passed, 230
  in the orchestrator crate. The `vulkan_ffx_fg` tests now run one at a time
  because owners share the route. Two are new:
  `one_owner_holds_the_wsi_route_and_a_borrow_retains_its_leaf` (second-owner
  refusal before open, foreign-device refusal, a queue borrow held across
  `rsf_ffxgvk_destroy`, closing refusal, held admission, single close on retry,
  and a successor) and `failed_open_releases_the_wsi_route`. The fake `open`
  asserts admission precedes it with nothing routed, and the fake `close`
  asserts the route refuses borrows while admission is still held.
- Mutation: making the route close ignore active borrows failed the borrow test
  (`rsf_ffxgvk_destroy` returned `OK` instead of `NOT_READY`). Restored.

- MSVC Release (Visual Studio 18 2026, MSVC 14.52) with the main checkout's
  vendor and reference paths, including corrected Streamline, built the Runtime,
  the AC7 and shared carriers, Minecraft, `rsf_native_vulkan` and the CPU and
  device-capable Vulkan fixtures without warnings. Only CPU entry points ran.
- New `native_ffx_vulkan_wsi_route_contract` drives the production hooks
  (`serialized_create`, `_destroy`, `_acquire`, `_images`, `_present`,
  `device_proc`, `graphics_submit`) through the real Rust route and the
  production native close. It checks foreign-device refusal, forwarded Vulkan
  results and the KHR to core lookup fallback. It destroys the owner from inside
  a forwarded present: the SDK contexts are released, the close is refused with
  `NOT_READY`, the hooks stay registered and admission is held. Later hooks are
  refused, the retry unregisters exactly once while the route lends nothing and
  admission is still held, a late hook touches no freed leaf, and a successor is
  admitted.
- Twelve CPU contracts passed in Release: `native_ffx_vulkan_wsi_route_contract`,
  `native_ffx_vulkan_submission_contract`, `native_ffx_vulkan_reset_contract`,
  `ffx_vulkan_wsi_dispatch`, `native_fg_vulkan_dispatch_contract` and its FSR4
  and XeSS variants, `native_vulkan_dll_exports`, `native_fsr4_fg_state_contract`,
  `native_fsr4_fg_contract`, `native_fsr4_fg_bridge_contract` and
  `native_dx12_vulkan_interop_contract`. The submission and reset fixtures now
  check the route through a borrow probe instead of the removed native pointer.
- Release export names and ordinals matched
  `build/rust-migration/baseline-exports` exactly: Runtime 381, AC7 carrier
  256, shared loader 273, Minecraft 267. The two new Rust symbols are internal
  to the static archive.
- A Release tree with `RSF_FFX_INCLUDE=OFF` and `RSF_FFX_VK_INCLUDE=OFF`
  (no `RSF_HAVE_FFX_VK` in `rsf_native_vulkan`) built the Runtime and passed
  `native_vulkan_dll_exports`, `native_vulkan_contract` and
  `native_fg_vulkan_dispatch_contract`.

## Generation callback source

The pinned SDK calls the generation callback synchronously from
`queuePresent` on the presenting thread. The Rust owner serializes configure,
Present and teardown. The route publishes a copy of the prepared frame record
only after configure succeeds and clears it on disable or effect destruction.
The callback reads the record for its native context and exact frame ID.
It releases the route lock before the native layer applies the reset flag,
dispatches the SDK and records any generated image. A successful dispatch
increments the status counter for that owner. Route release clears the count.

The MSVC Release reset fixture invokes the production native callback from its
fake SDK Present. It checks wrong-frame refusal, reset and exact-source
behavior, and the status count after an SDK dispatch failure. It passed
alongside six focused WSI, dispatch and export contracts. All 379 workspace
tests passed with pinned Rust 1.94.1, formatting and Clippy. Runtime, AC7,
Loader, Minecraft and Godot export names and ordinals matched the recorded
baselines. This check used no device or game.

## Validation gaps

No device, window, game or full `eng/verify.ps1` run, and no Debug MSVC build.
The hook bodies ran only in the CPU fixture against fake Vulkan entry points;
the production `open` and SDK creation steps still ran in no test. The audit's
thread-join argument is from the pinned SDK source, not an observed trace.

## Explicit registration and retirement

The Rust owner now registers the corrected SDK's WSI table after opening
the native leaf and before publishing it to callbacks. At teardown it
retires the SDK contexts, closes the route to new borrows and waits for
existing borrows by returning `NOT_READY` until they clear. It then
unregisters the table through the same typed native call. The pinned
bridge returns `-3` only if the table belongs to a different device.
Rust accepts that result as proof that this owner's table is gone.
Other refusals keep the module and the process-wide admission for retry.
The native destructor only unloads the module.

The private operations table adds one callback. The route fixture now
drives that callback for registration, a refused unregister, and a
later successful retry. The public headers have no changes. Build,
fixture and device results for this change remain open.
