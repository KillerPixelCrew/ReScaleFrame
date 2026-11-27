# Native FidelityFX Vulkan FG HUD-less copy in Rust

Baseline `119892c2351731239c706956480820aa9551ac5c`. The Runtime's FidelityFX 3 Vulkan FG
owner (`rsf_ffxgvk`, backend 3) already admitted the HUD-less frame in `vulkan_ffx_fg.rs`.
The native `copy` and `transition` functions still decided how to transfer it into the slot's
private image. `Ops::record_hudless_copy` now records the same sequence through two typed
Vulkan leaves. It follows the [Vulkan-to-D3D12 recording split](rust-vulkan-dx12-copy-plan.md).
All barriers here use ignored queue families since both images stay with the graphics queue.
The Runtime's [WSI route](rust-vulkan-ffx-wsi-owner.md), the shared promoted device-proc alias
and the standalone `runtime/vulkan/fsr3` worker retain their existing paths.

## Recording and admission

`Owner::hudless` validates the frame record, then honors the capture-only path when FG is
disabled. It checks that the source has the owner's device, a live image handle, copy-source
usage, RGBA8 format, the swapchain extent and shader-read layout. The shared frame ledger
joins it to prepared guides with matching frame ID, session, view and resource generation.
The existing image helper allocates or reuses the slot image after that join, and an allocation
failure still returns `INIT_FAILED` before recording. The copy takes the producer image and
that slot image, using the same swapchain width and height passed to native `open` at baseline.

| Order | Image and layout | Source and destination access |
| --- | --- | --- |
| 1 | Producer, `SHADER_READ_ONLY_OPTIMAL` → `TRANSFER_SRC_OPTIMAL` | Both `MEMORY_READ \| MEMORY_WRITE` |
| 2 | Slot, `UNDEFINED` on first use or `SHADER_READ_ONLY_OPTIMAL` on reuse → `TRANSFER_DST_OPTIMAL` | Source zero from `UNDEFINED`, otherwise `MEMORY_READ \| MEMORY_WRITE`. Destination `MEMORY_READ \| MEMORY_WRITE`. |
| 3 | `vkCmdCopyImage` from producer to slot in their transfer layouts | One region at zero offsets, extent `{width, height, 1}` |
| 4 | Producer, `TRANSFER_SRC_OPTIMAL` → `SHADER_READ_ONLY_OPTIMAL` | Both `MEMORY_READ \| MEMORY_WRITE` |
| 5 | Slot, `TRANSFER_DST_OPTIMAL` → `SHADER_READ_ONLY_OPTIMAL` | Both `MEMORY_READ \| MEMORY_WRITE` |

Each barrier uses `ALL_COMMANDS` (`0x10000`) for both stages, `QUEUE_FAMILY_IGNORED`
(`0xffffffff`) for both families, dependency flags zero and one image barrier. The access
mask is `0x18000` wherever both memory bits are set. The native leaves assemble
`VkImageMemoryBarrier` and `VkImageCopy` for color, mip 0, layer 0 and one level and layer.
Raw layouts 0, 5, 6 and 7 and the masks were checked against local Vulkan SDK 1.4.357.0
`vulkan_core.h` and are paired with native header assertions.

## Ordering and consumers

- Prepare still checks for an available slot and waits for its pending source fence and then
  consumer fence before reusing it. SDK prepare still consumes R32F depth and RG16F motion
  with the existing camera and frame record, followed by the guide captures. This study
  covers the transfer call and its resource handoffs. It supplies no new shader producer or
  active binding evidence.
- `recorded_hudless` is set before the barriers. The slot becomes initialized and eligible
  as a recorded HUD-less input after the two restore barriers, followed by the existing
  `fg-hudless-color` capture of the slot. Submission still marks the source fence and clears
  the initialized flag for discarded HUD-less recordings.
- Present still requires both recorded roles to be submitted, a matching source identity and
  a slot with no unknown completion flag. It configures the SDK with the admitted slot and
  frame before forwarding the original `VkPresentInfoKHR`. With `allowAsyncWorkloads = false`,
  Rust attempts the graphics-queue consumer-fence submission after SDK Present even when
  Present reports an error.
  In FidelityFX Vulkan `c6efa6bf7f2027b3ec94f28578bb5965eabb9e55`,
  `FrameInterpolationSwapchainVK.cpp` calls `presentInterpolated` at line 2627, dispatches
  interpolation at 2178, invokes the generation callback at 2046 and reads the asynchronous
  Present result at 2242.
- The generation callback still matches the configured frame lease, propagates its reset
  flag, and captures generated outputs before UI after successful dispatch. The WSI hook
  borrows still span each forwarded call. Drain still waits for SDK presents and device idle
  under the queue lock before flushing capture.

## Private ABI and checks

The old private copy slot is replaced by `barrier` and `copy` between `prepare` and
`configure`. The table grows from 26 to 27 pointer-sized slots, counting the two-pointer
loader. `Barrier` is 40 bytes and `ImageCopy` is 32 bytes, with sizes asserted on both sides.
Source review checked ordered fields and widths, callback signatures, the native initializer
and the Rust fixture. The C++ fixtures take a copy of the production table and keep its two
recording leaves. The disabled feature branch still passes null ops to Rust, which validates
setup before returning `NOT_COMPILED`. Both that branch and the Rust validation path match
the baseline. The public FG interfaces keep their existing signatures.

Source comparison, Rust 1.94.1 formatting, locked offline Cargo metadata and Git whitespace
checks passed. Build, compile and link of both feature paths, Clippy, fixture execution,
device and game behavior, deployment and push remain user-owned.

## FidelityFX Vulkan FG startup

Compared with `cc8f17ff`, Rust now owns the unpublished graphics
capsule while it resolves the required FidelityFX and Vulkan entries.
It preserves the order of the five FidelityFX exports and the three
device lookups that use the existing alias policy. Memory properties,
resource setup and capture initialization follow. The corrected WSI
registration export is then required, and all five WSI functions are
queried through the original device resolver without alias promotion.

Typed native leaves bind those addresses and populate the same
descriptor storage. A failed stage closes the unpublished capsule
before returning its existing result. The outer owner still holds the
module and reserves the same WSI route before startup. Registration
and callback publication follow a successful open as before. The
27-slot provider table, existing WSI fixtures and disabled path
keep their source interfaces.

The new private startup table has ten callbacks. Source comparison
matched their order and the 15 required names across all lookup
groups. The new fixture checks that the full WSI group is queried
and partial startup closes on a miss. Pinned formatting, locked
offline metadata and whitespace passed. Builds, fixtures, and
device and game verification remain open.

## Shared FG admission, submission and WSI follow-up

PORT.md V02 builds on V04's common source and image enumeration helpers and
V05's external input planner at `a3ea6940`. FidelityFX retains its finite
world-to-view transform and setup-specific depth and motion checks. Both image
routes query the count before a bounded fill and support a write-only count
query. The shared Present predicate now checks the prepared record for
`NO_FG` and `AMBIGUOUS_ID`, including legacy implicit Present. An explicit
matching identity can also veto interpolation.

FidelityFX, DLSS-G and Vulkan-to-D3D12 FG now give the Rust submission planner
six distinct resource slots and the actual submitted command list. This
preserves the per-slot masks and the different native fence and capture
actions. The native compatibility adapter remains available. FidelityFX
releases its route through the existing owner-ID check after closing its leaf
and unloading the module. The main FG dispatcher uses the common atomic
claim, and Rust supplies the default shader-read layout and one-frame limit
when the provider table omits a query callback.

The copy ABI generates the same barrier for same-queue transitions and
external acquire and release. The two transfer records still express their
own layout or extent facts. Vulkan command recording stays in a shared native
leaf. Rust now supplies the existing 15-second timeout for waits on private
FG resources. FidelityFX capture uses per-readback Vulkan events and still
flushes after the existing SDK and Vulkan drain. This prevents an idle device
from certifying an abandoned recording at a reused command address.

The patched SDK's dispatch-table implementation and its fixture stay on the
SDK side of the DLL boundary. The public Vulkan headers, SDK descriptor
marshalling, exception guards and native WSI hook fixtures remain in C++.
The shared Rust fixtures cover count, submission and veto decisions. The
FidelityFX native fixtures add stored-veto coverage for both Present routes
and check the provider-specific matrix and guide refusals.

Source review, bindgen 0.72.1 plus Rust 1.94.1 formatting, and
`git diff --check` were used for verification. Builds, fixture execution
and device validation remain open.
