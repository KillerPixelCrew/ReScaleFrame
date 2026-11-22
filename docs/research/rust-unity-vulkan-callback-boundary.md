# Unity Vulkan callback boundary

This source audit follows the remaining U04 work in
`integrations/unity/native/vulkan_bridge.cpp` from `fb27ab2f`. The earlier
[platform audit](rust-unity-vulkan-platform.md) covers shared-device import,
command calls and DXGI ownership. This change moves the callers of those
owners into Rust.

- `vulkan_bridge.rs` reads the same Unity texture requests and copies just
  image, layout and aspect into the admitted event under the state mutex.
  It drops that mutex before calling `AccessQueue` and supports an immediate
  queue callback. The event is held through the callback and its failure
  handling. A caught `AccessQueue` exception faults the bridge and retains
  any queued event, because Unity may already have received its address.
- `vulkan_bridge/queue.rs` preserves shared-device initialization, slot
  fence wait and reset, and the U02 recording tuple. Reconfiguration drains
  after slot begin and before preparing images. The phase-one host callback
  falls between accepted upload and fenced hand-back. Phase zero runs after
  hand-back and before ring advancement. The formats, rectangles, probe
  omissions, COMMON transitions, and hand-back gate match the original
  queue callback.
- `vulkan_bridge/present.rs` matches the same Unity instance, device,
  queue, swapchain, image index and HWND parent before consuming the
  receipt. It holds the DXGI backbuffer across both presents and preflights
  it before consuming the incoming waits. The original Vulkan driver
  receives the original record on the native path, or a copy whose only
  changed fields select the hand-back wait. The entry and presentation
  lease still cover forwarding. Surface resize uses the same queue drain
  and D3D12 completion proof in the same order. Checked retirement still
  releases images, command slots and pool, DXGI swapchain, then the shared
  device.
- `vulkan_bridge/window.rs` keeps the same in-process `UnityWndClass`
  parent, child class, Win32 styles, hit-test and activation behavior, and
  window-thread requirement. It republishes the actual HWND and creates
  the chain under a nonblocking presentation lease. The player keeps the
  child on session release, as before. The typed DXGI owner still creates
  the real three-buffer flip-discard chain against that HWND.

The remaining C++ dispatch uses `IUnityGraphicsVulkanV2` for interface
discovery, interception and event configuration, instance and texture
borrows, and `AccessQueue`. It keeps C++ exception guards around the Unity
SDK and the foreign host log, render pass, and loader Present calls.
Vulkan loader continuations still use the loader's owning dispatch, and the
native preview caller still receives WRL references transferred from Rust.
The private boundary now carries the generated facts and foreign function
declarations in `vulkan_foreign_abi.rs`. No U02 or U03 package paths changed.

## Verification

Rust 1.94.1 formatting of the Unity crate, targeted bindgen 0.72.1
regeneration, locked offline Windows Cargo metadata, and MSVC `/Zs`
syntax checking of `vulkan_bridge.cpp` with the Unity and Vulkan headers
passed. The first syntax attempt exposed the direct dependency on
`IUnityGraphics.h` for `UnityRenderingEventAndData`, which is now included.
Rust compilation, linking, device runs and Unity runtime acceptance
remain open.
