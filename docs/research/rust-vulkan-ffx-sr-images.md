# FidelityFX Vulkan SR staging images in Rust

This port moves creation, memory-type choice, allocation and binding of the two FidelityFX
Vulkan SR staging images (the RGBA16F output and the R8 zero mask) from the native `allocate`
leaf in `runtime/orchestrator/src/native_sr_vulkan.cpp` into `StagingImage::allocate` in
`runtime/orchestrator/src/vulkan_sr.rs`. It extends the earlier [SR cutover](rust-vulkan-sr-cutover.md).
The baseline is `a5867f2f92f9b883b51f6cf93355005d6e20d210`. This is a source comparison; no
build, test or device run was performed.

## What moved and what stayed

Rust now sequences, per image and in the existing order (output, then mask):

1. `vkCreateImage`. On any non-zero `VkResult` it returns `INIT_FAILED` and leaves the image
   handle at zero. On success it publishes the handle before the next step.
2. `vkGetImageMemoryRequirements`.
3. Memory-type choice: the first type that is in `memoryTypeBits`, below `memoryTypeCount` and
   `DEVICE_LOCAL`, otherwise the last type that is in `memoryTypeBits` and below
   `memoryTypeCount`, otherwise `INIT_FAILED`. This is the native loop's result: it assigned
   every compatible index in ascending order and stopped at the first device-local one.
4. `vkAllocateMemory` with the requirement size and the chosen index. On failure it returns
   `INIT_FAILED` and leaves memory at zero. On success it publishes the memory before binding.
5. `vkBindImageMemory` at offset 0. On failure it returns `INIT_FAILED`.

Every Vulkan failure still maps to `INIT_FAILED`, as the native leaf did. A failure part way
leaves exactly the handles the native leaf had written. `FidelityFx::drop` then releases them
through the unchanged native `release` leaf, which destroys the image before freeing the
memory and skips zero handles. The constructor's rollback destroys the effect first, then
clears sharpening, releases output and mask in that order, and closes the native owner. If SDK
destruction is refused, the constructor retains the native state, module and device dispatch
lease. The later recording review corrected the earlier description and the fixture's stale
rollback expectation to match `FidelityFx::drop`.

The typed Vulkan calls reuse the existing shared `rsf::vkfg::platform` leaves
(`vulkan_fg_resources_native.h`), which the XeSS SR and FG owners already use. They match the
former descriptors field for field:

- `VkImageCreateInfo`: `sType` image create info, no `pNext`, no flags, 2D, extent
  `{width, height, 1}`, one mip and layer, the image format, one sample, optimal tiling, the
  requested usage, `sharingMode` 0 (`EXCLUSIVE`, which the former leaf set explicitly), no queue
  families, `initialLayout` 0 (`UNDEFINED`). A new `static_assert` pins both zero values.
- `VkMemoryAllocateInfo`: `sType`, no `pNext`, `allocationSize` from the requirements,
  `memoryTypeIndex` from Rust.
- `vkBindImageMemory(device, image, memory, 0)`.

The shared `ensure_image` helper was not used. It accepts only device-local types, maps the
no-type case to `-2`, returns raw `VkResult` values, refuses a null handle on success and
releases a partial replacement itself. None of those match the native FFX behaviour.

The entry points are still loaded through `sdk_device_proc` (the Rust dispatch registry), in the
same order, before the barrier, clear and copy entry points, with the same
`MISSING_ENTRY_POINT` result. Only their storage moved into the `rsf::vkfg::Device` member.
The device-local mask comes from `rsf::vkfg::Device::memory_types`, computed from the same
`vkGetPhysicalDeviceMemoryProperties` result that the capture path uses. The memory type count
comes from a new `memory_type_count` leaf.

Unchanged: the program and SDK preparation, version selection, effect creation, plan, the
dispatch registry and `rsf_core_srvk_dispatch_lookup`, frame admission and history, dispatch,
copy, sharpening, capture, the public `rsf_srvk_*` ABI, the `RSF_HAVE_FFX_VK` disabled path
(`platform == nullptr`), and the DLSS and XeSS routes. The staging descriptors keep their
retained layouts (output `GENERAL`, mask `SHADER_READ_ONLY_OPTIMAL`), `copy_source` flags and
usage masks, and the first evaluation still records the mask clear and output transition.

## Private table layout

`FfxOps` replaces the `allocate` slot with three slots in the same position, in both languages:
`image_user`, `images` (pointer to `rsf_vkfg_platform`) and `memory_type_count`, followed by
`release`. `FfxOps` grows from 25 to 27 pointer-sized slots and `Platform` from 256 to 272
bytes. Both sides now assert the `FfxOps` slot count as well as the `Platform` size. I compared
the Rust struct, the C++ struct and the C++ `constexpr` initializer slot by slot.

## Test changes

The Rust owner fake in `vulkan_sr/owner_tests.rs` now supplies the shared image leaves instead
of the removed `allocate` stub. It reports three memory types, types 1, 2 and 4 in
`memoryTypeBits` and types 2 and 3 as device-local, and its allocate leaf asserts the choice
of type 2. It fails the mask allocation with a raw `-2` to check the `INIT_FAILED` mapping. The existing expected call
sequences, including `Release(15, 0)` after a failed mask allocation, are unchanged. A new
table test covers device-local preference, last-compatible fallback, the count bound, a count
of 32 and no compatible type. `rsf_core_srvk_dispatch_lookup` and the `tests` module in
`vulkan_sr.rs` were not touched.

## Checks and gaps

Performed: source comparison of the removed native leaf against the Rust sequence and the
shared leaves, table order and size review, pinned Rust 1.94.1 `cargo fmt --check`, locked
offline `cargo metadata` and `git diff --check`.

Not performed, and owned by the user: C++ compile and link (the new include, the lambdas in the
`constexpr` table and the size assertions), Rust build, Clippy and `cargo test`, the native
`rsf_native_vulkan_test` device test, and an FSR2/FSR3 Vulkan game run. The device test is the
first check that the real driver's chosen memory type and bind still succeed.

## First-use initialization and output publication

The recording port uses `a6f4dedb34f0a23f959964e617def1f5150dab21` as its baseline. I compared
the three compound leaves in `native_sr_vulkan.cpp` against `FidelityFx::evaluate`, the shared
`vulkan_fg_copy` record types and Vulkan marshalling, and the public SRVK recording contract.
The shared barrier and copy records match the required subresources and queue ownership.
The FFX owner supplies the explicit access masks below through `Barrier::with_access`. The
existing XeSS and FG constructor still chooses `MEMORY_READ | MEMORY_WRITE`, with source
access zero from `UNDEFINED`.

| Evaluation point | Image operation | Source and destination access |
| --- | --- | --- |
| After the input dump captures, on first use | Transition the plan-sized R8 mask from `UNDEFINED` to `TRANSFER_DST_OPTIMAL`. | `0` to `TRANSFER_WRITE` |
| Next | Clear colour mip 0, layer 0 of the mask to zero in `TRANSFER_DST_OPTIMAL`. | Clear command |
| Next | Transition the mask from `TRANSFER_DST_OPTIMAL` to `SHADER_READ_ONLY_OPTIMAL`. | `TRANSFER_WRITE` to `SHADER_READ` |
| Next | Transition the private RGBA16F output from `UNDEFINED` to `GENERAL`, then mark initialization recorded. | `0` to `SHADER_WRITE` |
| After accepted dispatch, sharpening and output capture | Transition the private output from `GENERAL` to `TRANSFER_SRC_OPTIMAL`. | `SHADER_WRITE` to `TRANSFER_READ` |
| Next | Copy the private output allocation's exact width and height into the caller's existing `TRANSFER_DST_OPTIMAL` image. | Copy command |
| Next | Restore the private output from `TRANSFER_SRC_OPTIMAL` to `GENERAL`, then commit history. | `TRANSFER_READ` to `SHADER_WRITE` |

Each barrier uses `ALL_COMMANDS` for both stages, ignored queue families, zero dependency
flags and one colour range at mip 0 and layer 0 with one level and one layer. The copy uses
the same colour mip and layer on both images, zero offsets and depth 1. The mask is sampled
and transfer-destination, and supplies each omitted reactive or composition slot. The
private output is sampled, storage, transfer-source and transfer-destination. Only the private
output changes layout during publication. The caller supplies the transfer-destination usage
and layout and retains ownership of that image for later passes.

Input dump capture precedes first-use recording. Parameter capture and optional colour, depth
and motion captures precede the typed FFX dispatch. A refused dispatch clears history and
returns before sharpening, output capture and copy-back. The initialization flag records the
first-use barriers and clear even if that dispatch fails. Reuse requires the caller to submit
the failed recording in order or retire the context. Reset flags and frame, session, view or
resource generation discontinuities still affect history independently of image initialization.

The C++ and Rust tables and the C++ initializer agree slot by slot. `barrier`,
`clear_color_zero`, `dispatch`, `copy` and `sharpen` occupy offsets 112, 120, 128, 136 and
144 on x64. `FfxOps` remains 27 pointer slots and `Platform` remains 272 bytes. The shared
records are 40 and 32 bytes. Typed Vulkan and SDK calls stay behind the native table. The
seven SRVK exports and ABI 2 stay with the existing forwarding functions. With Vulkan FFX
headers disabled, the null platform still returns `NOT_COMPILED` after create admission and
`NOT_SUPPORTED` after spatial fallback admission. Spatial blit, shader and version policy
continue through their existing paths.

The existing Rust owner fixture now describes the primitive recording order around capture,
dispatch and copy-back, including refusal. Checks for this port were source comparison,
pinned Rust 1.94.1 formatting, locked offline Windows Cargo metadata and Git whitespace
review. Enabled and disabled compilation and linking, binary export checks, Clippy, CPU
fixtures, Vulkan execution and visual game acceptance remain with the user. The producer and
shader coverage limits in the [Vulkan SR cutover](rust-vulkan-sr-cutover.md) still apply.

## FidelityFX Vulkan SR startup

Compared with `0681aab6`, the SR owner now resolves the four FidelityFX
context exports in Rust, using the same ordered list as native Vulkan
FG. A missing export closes the unpublished capsule before the module
lease ends. The native resolver method has been removed.

After acquiring the existing device-dispatch lease, Rust resolves the
nine staging functions through the same alias policy and refuses on the
first miss. It then resolves the instance memory query and supplies its
typed address to the native initializer. The initializer binds the
resolved addresses, reads memory properties, and initializes capture
as before. Version selection, staging allocations and dispatch retain
their existing owners and order.

Source comparison matched all 14 names, the 30-slot private FFX table,
and the public declarations and disabled path. The existing Rust owner
fixture now exercises startup through fake lookup callbacks. Pinned
formatting, locked offline metadata and whitespace passed. Native
linkage, fixtures, and Vulkan device and game verification remain open.
