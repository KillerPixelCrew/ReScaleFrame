# Vulkan FidelityFX 3 frame generation Rust cutover

The baseline is `5accc0e`. This ports the first-party owner of the FidelityFX 3
Vulkan frame-interpolation swapchain (`rsf_ffxgvk`, FG backend 3) from
`native_fg_vulkan.cpp` to `vulkan_ffx_fg.rs`. The stable `rsf_fgvk_*` and
`rsf_ffxgvk_*` entry points, their results and the disabled-build stubs are
unchanged. SDK calls, Vulkan commands, the WSI hooks, capture and timing stay in
the native leaf. Evidence is source inspection of the pinned SDK, Windows MSVC
builds and CPU contracts. No device, window or game ran.

The SDK source read for this port is the corrected FidelityFX Vulkan checkout at
`references/godot/fidelityfx-vulkan`, revision `c6efa6bf7f2027b3ec94f28578bb5965eabb9e55`.
The FSR4 port (a removed note)
was the reference for shared ordering.

## Ownership

| Concern | Owner |
| --- | --- |
| Setup and swapchain admission | Rust checks setup size and ABI, the embedded graphics setup, required callbacks, depth and motion flags, and four distinct queues of one family. The native wrapper reads only the swapchain extent, format, colour space, layers, `sType` and `oldSwapchain`; Rust refuses an old chain, layered or non-SRGB swapchains and formats other than RGBA8/BGRA8 with `NOT_SUPPORTED` before anything is loaded. |
| Process-wide admission and WSI routing | Native at this cutover: one atomic routed the hooks and was the one-owner admission, claimed in `open` and released in the leaf destructor after WSI unregistration. Moved to Rust later, with scoped hook borrows; see [rust-vulkan-ffx-wsi-owner.md](rust-vulkan-ffx-wsi-owner.md). |
| SDK module and contexts | Native steps (`open`, `create_swapchain`, `query_swapchain`, `create_effect`, `query_provider`, `destroy_effect`, `destroy_chain`, `destroy_swapchain`, `close`). Rust sequences them and records which ones succeeded. |
| Recording ledger, slot fences, private HUD-less images | Rust, through the shared `vulkan_fg_frames` ledger and the shared `vulkan_fg_resources` image and fence helpers, with the native `rsf::vkfg::Device` table as their platform. |
| Presentation admission | Rust selects the source (implicit latest or the explicit identity prefix), asks the shared ledger for a candidate, and configures the presenter. Configure passes the admitted slot record to the leaf as a lease. |
| Generation callback | Native. `FrameInterpolationSwapChainVK::queuePresent` calls `dispatchInterpolationCommands`, which calls the callback synchronously on the presenting thread (`FrameInterpolationSwapchainVK.cpp` lines 2627, 2178 and 2046). The callback reads only the lease, so it never aliases the Rust owner, which is inside its present call at that time. |
| Retirement | Rust. Each step records completion, so a refused teardown can be retried without repeating a release. |
| Diagnostics | Native capture and the opt-in timing file. Rust opens the `ffx_prepare_total` and `ffx_present_total` scopes; the leaf keeps `ffx_prepare_sdk`, `ffx_configure_*`, `ffx_queue_present`, `ffx_acquire`, `ffx_slot_fence_wait` and the WSI hook scopes, so nesting is unchanged. |

## Verified invariants

- Preparation and HUD-less validation, the order of their checks and every
  returned result match the baseline. A disabled owner with no capture request
  still returns OK without recording.
- A slot is reused only after `vulkan_fg_resources::prepare_fences` (then `rsf_core_vkfg_prepare_fences`) has waited for
  both its source and consumer fences. Unknown completion keeps the slot
  unavailable. Superseded command-buffer recordings are discarded by the shared
  ledger, and a discarded HUD-less copy loses its initialized layout.
- Presentation admits a source only once, only when both roles were recorded
  and submitted, and never for `NO_FG`, `AMBIGUOUS_ID`, a foreign session,
  view or resource generation, or a short or foreign-ABI identity. An explicit
  source that is not admitted reports source frame 0.
- A camera reset reaches SDK dispatch through the leased record's flags without
  changing presenter activation. The ledger adds the reset flag for a producer or
  frame discontinuity.
- The presenter is configured on the graphics queue with
  `allowAsyncWorkloads = false`, so interpolation runs on the game queue and the
  consumer fence submitted after Present on that queue covers it
  (`FrameInterpolationSwapchainVK.cpp` line 1832).
- Teardown checks that every recording was submitted, disables generation,
  waits for SDK presents and device idle under the host queue lock, then
  destroys the effect, the chain, the swapchain context, the slot images and
  fences, unregisters the WSI hooks, releases admission and unloads the module.
  The leaf keeps the callback context alive until the effect is destroyed.
- When Present cannot be configured, the image is not presented and the result
  is `VK_ERROR_INITIALIZATION_FAILED`, as before. A Present without an admitted
  source configures generation off and presents the real image.

## Findings and behavior changes

1. **Missing consumer fence after an error Present (fixed).** The baseline
   fenced the HUD-less slot only when `pOutQueuePresentKHR` returned success or
   suboptimal. `presentInterpolated` submits the interpolation that reads the
   HUD-less copy and then returns `presentInfo.lastPresentResult`, the result of
   an earlier asynchronous present (`FrameInterpolationSwapchainVK.cpp`
   lines 2114 to 2242). After, for example, `VK_ERROR_OUT_OF_DATE_KHR`, the slot
   could be reused and its copy overwritten while the GPU still read it. Every
   admitted and configured source now receives its consumer fence whatever the
   result. A failed fence submission marks unknown completion.
2. **Use after partial teardown (fixed).** After a refused destroy, the
   baseline still let Present, image queries and acquire reach SDK functions of
   a destroyed effect or chain. Once retirement has started, those calls and new
   recordings are refused; status, enable, submission notifications, drain and
   destroy retry remain available.
3. **Failed creation cleanup (fixed).** The baseline destructor ignored SDK
   destroy results after a failed creation and freed the owner anyway. Creation
   cleanup now uses the checked teardown. If it refuses, the owner is returned
   with the error for the documented destroy retry in `native_fg_vulkan.h`, as
   the FSR4 owner already does. A never-available owner skips disable and drain
   because no frame or Present could have reached it.
4. **Generation lease.** The baseline callback looked up any slot by frame ID
   and checked its submission flags. The SDK calls back only for the frame of
   the configuration applied in that `queuePresent` (it applies the pending
   configuration at line 2597 and takes `currentFrameID` from it at line 1863),
   so the leaf now accepts
   exactly the configured lease. This is narrower, never broader, than before.
5. **Stale fixtures.** `native_ffx_vulkan_reset.cpp` no longer compiled at the
   baseline: it named removed members such as `rsf_ffxgvk::latest` and
   `Slot::consumer_completion`. Both FFX fixtures now share
   `tests/native_ffx_vulkan_harness.h`, which drives the retained
   `rsf_ffxgvk_*` entry points through the real Rust owner and the real native
   leaf. Only module loading, SDK context creation and Vulkan entry points are
   fakes.

The duplicated present-identity reader in `vulkan_ffx_fg.rs` and
`vulkan_fsr4_fg.rs` is identical. Moving it into the shared ledger would change
the FSR4 owner, which was out of scope for this dispatch.

The coordinator merge moved that reader and the DLSS equivalent into the
shared Rust FG ledger. The dispatch facade now validates the same prefix
through Rust before calling the provider. The combined fixtures check
short and foreign prefixes, explicit null sources, and frame, session,
view and generation matching.

The creation leaves now report which SDK contexts exist separately from
the SDK result. The pinned `ffxCreateContext` clears its output and the
frame generation providers write their context outputs on success. The
Vulkan swapchain provider writes its replacement chain before querying
replacement functions, and that query returns success in the pinned
source. For defensive SDK results, Rust sends returned contexts through
the existing retirement path. It retains a refused destroy for retry.
It also retains an unexpected replacement chain with no context because
`vkDestroySwapchainFFXAPI` requires its context. These are source findings.
The updated creation path has no build or runtime results recorded.

## Verification

The Windows MSVC build used Visual Studio 18 2026 (MSVC 14.52) with the
coordinator's vendor and reference paths, including the FidelityFX Vulkan API
headers above and Godot's Vulkan headers. No SDK binary was loaded. Logs and
export listings are in
`D:/Coding/ReScaleFrame/build/rust-migration/worker-evidence/rust-vulkan-ffx-fg`.

- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked
  -- -D warnings` and `cargo test --workspace --locked` passed: 210 tests, 135 in
  the orchestrator crate. The four new `vulkan_ffx_fg` cases cover admission
  refusal before the native leaf opens (including the disabled-build
  `NOT_COMPILED`), owner retention after a refused cleanup of a failed creation,
  the consumer fence after an error Present, and teardown that waits for
  submission and drain and retries without repeating released steps.
- Mutation check: restoring the baseline's success-only consumer fence made the
  error-Present case fail with one fence instead of two. The change was reverted
  and the case passed again.
- Debug and Release built the Runtime DLL, the AC7 and shared carriers,
  Minecraft, `rsf_native_vulkan`, the CPU contracts and the device-capable
  fixtures `rsf_native_vulkan_test`, `rsf_native_dlss_vulkan_test`,
  `rsf_native_xess_vulkan_test`, `rsf_native_fg_xess_vulkan_test`,
  `rsf_native_fsr4_fg_bridge_test` and `rsf_input_dump_vulkan`. Only CPU entry
  points ran.
- Eleven CPU contracts passed in Debug and in Release:
  `native_ffx_vulkan_submission_contract`, `native_ffx_vulkan_reset_contract`,
  `native_fg_vulkan_dispatch_contract`, `native_fg_vulkan_dispatch_fsr4_contract`,
  `native_fg_vulkan_dispatch_xess_contract`, `ffx_vulkan_wsi_dispatch`,
  `native_vulkan_dll_exports`, `native_fsr4_fg_state_contract`,
  `native_fsr4_fg_contract`, `native_fsr4_fg_bridge_contract` and
  `native_dx12_vulkan_interop_contract`.
- The submission fixture covers one fence per joint notification, idempotent
  notifications, reused command addresses, abandoned recordings, SDK prepare
  failure retention, split roles, unknown completion, session, view and
  generation identity at HUD-less join, explicit older and newer sources,
  short and foreign identities, `NO_FG`, unassociated output, the error-Present
  consumer fence and two-fence slot reuse, the oversized image query, and
  teardown retention after a refused drain, effect
  destruction and swapchain-context destruction. The reset fixture covers four
  frames with producer and engine resets delivered to SDK dispatch with one
  activation, explicit disable, unknown completion, a view-change reset on an
  older explicit source, and a refused view identity followed by the matching one.
- A Release tree with `RSF_FFX_INCLUDE=OFF` and `RSF_FFX_VK_INCLUDE=OFF`
  compiled the disabled stubs and passed `native_vulkan_contract`,
  `native_fg_vulkan_dispatch_contract` and `native_vulkan_dll_exports`.
- Release export names and ordinals matched
  `build/rust-migration/baseline-exports` exactly for Runtime (381), AC7 carrier
  (256), shared loader (273) and Minecraft (267).
- The fresh worktree's yyjson download failed its pinned hash. The pinned copy
  from the main checkout (matching SHA-256 `40a9d8b3…`) was used; it is
  untracked under `vendor/`.

## Validation gaps

No AMD or other device, window, game or full `eng/verify.ps1` run was part of
this cutover. Interpolated image quality, frame pacing, resize and
`VK_ERROR_OUT_OF_DATE_KHR` recovery, HDR, and SDK presenter-thread behavior under
real load remain unvalidated on hardware. The consumer-fence fix is proven
against the SDK source and the CPU contracts, not by a device capture.

The CPU harness replaces the native creation steps (`open`,
`create_swapchain`, `query_swapchain`, `create_effect`, `query_provider`).
Their production code, which loads the module, claims the process-wide
admission, registers the WSI hooks and creates the SDK contexts, moved from
the baseline with only its failure cleanup relocated, and ran in no test. The harness's fake `open`
applies the same admission, so the second-owner refusal it observes is not the
production path.
