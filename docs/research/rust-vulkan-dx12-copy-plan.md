# Vulkan-to-D3D12 FG transfer plan in Rust

Baseline: `301ad289d88552521dbb2b48e30f1e41340ae73d`. Before this change,
`runtime/orchestrator/src/native_fg_dx12_vulkan.cpp` still decided how each frame-generation
input reaches its D3D12-shared image: `copy_input` chose copy or blit and planned layouts and
queue ownership, `transition` derived access masks, `clear` planned its own two transitions,
and `platform_create` held the blit-format admission predicate. The FSR4 and XeSS Vulkan FG
backends share this owner (`vulkan_dx12_fg.rs`) and the same native ops table, so both paths
move together. No new experiment was run. This note records the source review behind the port.

## What moved and what stays native

`vulkan_dx12_fg.rs` now builds an ordered step array for each recorded transfer and replays
it through four typed native leaves: `barrier`, `copy`, `blit` and `clear`. Rust owns:

- the copy-or-blit choice (`source.format == target.format` copies, otherwise blits);
- each barrier's image, old and new layout, source and target queue family, access masks and
  stage masks;
- the placeholder clear's discard and release transitions;
- the blit-format admission predicate, exported as `rsf_core_dx12gvk_blit_formats_supported`.

Native code keeps the `VkImageMemoryBarrier`, `VkImageCopy` and `VkImageBlit` descriptors,
the single colour subresource (aspect colour, mip 0, layer 0, one level and layer), the
`VK_FILTER_NEAREST` blit filter, the `int32_t` blit offsets, the zero clear colour, and the
format-property query. The queue family the plan uses is `rsf_fgvk_setup::queue_family`,
the same value the native platform stored as `family` and used for its command pool.

The admission predicate stays inside `platform_create` as a call into Rust so its position is
unchanged: after the device and instance entry points resolve and the two format queries run,
and before `vkGetPhysicalDeviceMemoryProperties` and capture initialization. A failure still
returns `VK_ERROR_FORMAT_NOT_SUPPORTED` with no platform object created. Moving the check after
`platform_create` would have required retiring a half-initialized owner through drain and vendor
teardown, which is new behaviour.

## Command order

Every barrier uses `VK_PIPELINE_STAGE_ALL_COMMANDS_BIT` for both stage masks, no dependency
flags and one image barrier per `vkCmdPipelineBarrier`, as before. Access is
`MEMORY_READ | MEMORY_WRITE` (`0x18000`) except where noted. `F` is the graphics queue family.

| Recorded transfer | Steps in order |
| --- | --- |
| Copy or blit, target not yet initialized | Source `layout → TRANSFER_SRC`, families ignored, access both. Target `UNDEFINED → TRANSFER_DST`, `F → F`, source access 0. Copy (target extent) or blit (source and target extents, nearest). Source `TRANSFER_SRC → layout`, families ignored. Target `TRANSFER_DST → GENERAL`, `F → EXTERNAL`, target access 0. |
| Copy or blit, target initialized | As above, except the target acquire is `GENERAL → TRANSFER_DST`, `EXTERNAL → F`, source access 0. |
| Placeholder clear | `UNDEFINED → TRANSFER_DST`, `F → F`, source access 0. Clear to zero. `TRANSFER_DST → GENERAL`, `F → EXTERNAL`, target access 0. |

The call sites and their order are unchanged. Preparation records depth then motion, each
followed by its capture record; HUD-less records its copy then its capture record; present
begins the command buffer, clears the depth and motion placeholders once, copies the presented
colour, then ends the buffer before the timeline submit. Initialization flags, the release and
wait timeline values, the fence marks and the capture groups are untouched.

In practice depth (`R32_SFLOAT`) and motion (`R16G16_SFLOAT`) always copy, a BGRA8 swapchain
copies, and RGBA8 colour and HUD-less inputs blit into the BGRA8 shared images. That is why the
admission predicate requires `BLIT_SRC` on RGBA8 and `BLIT_DST` on BGRA8 optimal tiling.

## ABI and constants

The internal `rsf_dx12gvk_ops` table grows from 24 to 26 pointers because `copy` and `clear`
are replaced by `barrier`, `copy`, `blit` and `clear`. The FSR4 ops table embeds it by value, so
its size assertion moves from 37 to 39 pointers on both sides. The XeSS table holds a pointer
and is unchanged. `rsf_dx12gvk_barrier` is 40 bytes and `rsf_dx12gvk_transfer` 32 bytes, both
asserted in Rust and C++. No public header, exported entry point or game ABI changed.

Rust carries these values as raw numbers. The native file asserts each against the Vulkan
headers, and they were also checked against `vulkan_core.h` from Vulkan SDK 1.4.357:

| Constant | Value |
| --- | --- |
| `VK_IMAGE_LAYOUT_UNDEFINED`, `GENERAL`, `TRANSFER_SRC_OPTIMAL`, `TRANSFER_DST_OPTIMAL` | 0, 1, 6, 7 |
| `VK_QUEUE_FAMILY_IGNORED`, `VK_QUEUE_FAMILY_EXTERNAL` | `~0u`, `~1u` |
| `VK_ACCESS_MEMORY_READ_BIT`, `VK_ACCESS_MEMORY_WRITE_BIT` | `0x8000`, `0x10000` |
| `VK_PIPELINE_STAGE_ALL_COMMANDS_BIT` | `0x10000` |
| `VK_FORMAT_FEATURE_BLIT_SRC_BIT`, `VK_FORMAT_FEATURE_BLIT_DST_BIT` | `0x400`, `0x800` |

## Scope

The FFX Vulkan FG owner (`vulkan_ffx_fg.rs`) and DLSS Vulkan FG have their own copy paths and
are not touched. SDK preparation, bridge calls, the interop importer and the WSI marshalling
in the same native file are unchanged.

## Checks

Source review compared the old native sequence against the new step arrays line by line,
including access-mask conditions, queue families and the copy extent (target size) versus blit
offsets (source and target sizes). New unit tests in `vulkan_dx12_fg/tests.rs` assert the exact
step arrays for an uninitialized copy, an initialized acquire, the RGBA8-to-BGRA8 blit, the
placeholder clear and the admission truth table, using the header values written out rather
than the module's constants. Rust 1.94.1 formatting, locked offline Cargo metadata and Git
whitespace checks passed. Compile and link, Clippy, the Rust and native tests, device and game
verification, deployment and push remain user-owned.

## Graphics admission and dispatch setup

Compared with `27f17a38`, Rust now allocates the native capsule through
a leaf, resolves the 22 required device functions in order, and then
resolves the two instance functions. Device lookup stops at the first
miss. Both instance functions are queried before testing their
addresses. The native binder installs the same typed Vulkan functions
only after resolution succeeds.

Rust queries RGBA8 and then BGRA8 optimal-tiling capabilities before
admitting the blit. Memory properties and capture initialization follow
that decision. On refusal Rust destroys the unpublished capsule
directly, before it can enter platform drain or vendor initialization.
The shared Vulkan interop lookup leaf now catches resolver exceptions
and reports the existing feature-failed code.
Missing entries and missing format capabilities keep their previous
result codes.

The private shared table grows from 26 to 30 pointers and FSR4's
embedded table grows from 39 to 43. XeSS still borrows the shared table.
Source comparison covered all 24 names, both admission orders and
public declarations. The extended Rust fixture covers device
short-circuit and the paired instance queries. Pinned formatting,
locked offline metadata and whitespace passed. Native linkage,
fixtures and live provider verification remain open.

## Present admission and waits

Compared with `a551200b`, Rust admits the shared XeSS and FSR4 Present
shape before native code reads the Vulkan arrays. The request must
have one swapchain, its image index, no extension chain, and at most
16 binary waits. The native reader copies only the admitted wait
count into the Rust array and retains the exact source record and
explicit-source flag. The typed result write still happens only when
the caller supplied `pResults` and the provider changes the incomplete
sentinel.

The Rust submission plan keeps the binary wait order and initializes
each value to zero. When a prior submission must complete, it appends
the shared timeline semaphore and the prior done value in slot 17 or
earlier. Every wait uses `ALL_COMMANDS`. The native submit still
signals the same ready value through the host's graphics queue call.
Command recording, the ready and done calculation, and the failure
path stay in their existing order.

The private records are 32 and 368 bytes on x64. The extended fixture
covers refusal before borrowed-array reads, the result sentinel,
the first submit without a timeline wait, and the full binary wait
list with the prior timeline wait. Source, formatting, locked offline
metadata and whitespace checks passed. Builds, fixture execution,
and live provider checks remain open.
