# Shared Vulkan FG calls in Rust

Base revision `821642ae`. The remaining shared C++ resource table built image,
view, memory, and fence descriptions and called Vulkan on behalf of the Rust
resource owner. The shared command header similarly translated Rust barrier
and copy plans into Vulkan commands. This change replaces those implementations
with typed `ash::vk` calls while retaining the C-facing owner and resource tables.

| Path | Source evidence |
| --- | --- |
| Images, memory, views, fences | `vulkan_fg_resources/device.rs` resolves the same device commands and uses the same image shape, usage and local memory mask. The C++ `Device` in `vulkan_fg_resources_native.h` holds the Rust dispatch table for the native leaf's lifetime. The resource owner still commits replacements and retires old allocations in `vulkan_fg_resources.rs`. |
| Barriers and image copies | `vulkan_fg_copy.rs` supplies the Vulkan barrier and copy recording for FFX, DLSS, FSR4 SR and XeSS SR leaf call sites. It uses the plan's stages, access, layouts and families and records color mip zero, layer zero and one layer. |
| WSI forwarding | `vulkan_fg_wsi.rs` forwards typed calls with the SDK's device, queue, timeout, semaphores, fence and output pointers. Create and destroy remain inside DLSS-G suppression only. Acquire and image enumeration use the latency scope within suppression, and Present uses the existing latency Present entry. The outer leaf borrow and total timing still surround the synchronous call. |
| Swapchain descriptions and Present chain extraction | `rsf_core_ffxgvk_create_native` reads the tagged Vulkan create record and hands the same facts to the owner. `single_present_chain` is shared by the FFX and DLSS readers and preserves the single-swapchain requirement. |
| CPU fixtures | Rust owns the corrected SDK dispatch assertions in `tests/vulkan-fg/src/wsi.rs`. The fixture's C++ bridge still supplies the SDK header implementation and fake Vulkan entry points. The native FFX harness now resolves its fake commands into the Rust resource table and creates through the typed swapchain reader. |

The fence callback still submits on the caller's graphics function with the
caller user pointer, zero submits and the same fence. The resource owner's
wait timeout, reset order and completion handling are unchanged. The FFX
source and consumer receipts, presentation admission and capture retirement
retain their existing owners and call sites. SDK descriptor construction,
registration, capture and provider registration glue remain in the native leaves.

The dispatch contract with fake providers and the FFX reset, submission and
WSI route scenario bodies remain C++ fixtures. The public Vulkan FG header
and existing platform and frame record layouts remain unchanged.

Verification was source-only. The bindings were regenerated with bindgen
0.72.1. Rust 1.94.1 formatting for the affected packages, locked offline
workspace metadata and `git diff --check` passed. The workspace formatting
check reported an existing import-order difference in
`games/fallout4/src/platform_abi.rs`. Build, fixture execution and runtime
behavior remain unverified for this revision.
