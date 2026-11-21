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
memory and skips zero handles. The constructor's rollback order (clear sharpening, destroy
effect, release output, release mask, close) is unchanged.

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
