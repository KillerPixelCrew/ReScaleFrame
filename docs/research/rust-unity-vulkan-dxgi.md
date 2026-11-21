# Unity Vulkan DXGI swapchain ownership

The baseline is `6355b2fc668a8655f6f25447964cdc6a3a2350ae` on
`rust/orchestrator-anomaly`. This source audit follows `configure_chain`, the player surface
entry points, intercepted Present and checked release in `integrations/unity/native/vulkan_bridge.cpp`.
It builds on the [shared graphics device audit](rust-unity-vulkan-shared-device.md).

## Surface transaction and presentation

- The window thread keeps the first child HWND and performs the same client-rectangle check
  and `SetWindowPos` before bridge admission. If the chain already exists it returns ready.
  Initial creation tries the presentation lease without waiting; it rechecks ownership after
  acquiring it. A concurrent Present can publish the chain instead. The HWND survives release.
- Rust borrows the already published device and creates the DXGI owner under the entry and
  exclusive presentation lease. The typed leaf uses the same D3D12 queue, `CreateSwapChainForHwnd`
  descriptor and `IDXGISwapChain3` query as the baseline. The factory and temporary chain stay
  scoped to the leaf. Rust takes the single transferred reference after a successful query.
- Present configuration compares the actual DXGI buffer width and height from `GetDesc1` with
  the intercepted Unity swapchain's extent. Only a mismatch calls the existing render-side
  drain, which waits for the Vulkan graphics queue and checks or waits up to 10 seconds for the
  submitted D3D12 fence value. `UINT64_MAX` still refuses the drain. A successful drain allows
  `ResizeBuffers` with the same format, buffer count and flags. A refused drain or resize keeps
  the DXGI owner for a later Present; a refused creation leaves nothing published. Native DXGI
  calls run outside the Rust mutexes under the existing lease.
- The Present image and ready semaphore still come from Unity's intercepted swapchain handle
  and presented image index. Source lookup, surface configuration and shared image preparation
  precede the DXGI backbuffer lookup. Native code keeps the current `GetBuffer` COM reference
  through command recording, the window render callback, DXGI Present and forwarded Vulkan
  Present. The same leased chain supplies the buffer, the window pass identity and DXGI Present.
- A source miss or failed surface/image setup forwards Unity's original waits. A failed buffer
  lookup or slot reset also faults before forwarding those waits. Upload consumes Unity's waits,
  signals the next shared value and queues the D3D12 wait. The subsequent hand-back keeps the
  D3D12 signal, Vulkan wait, ready semaphore signal and slot fence in their original order.
  Submission failure faults and returns `VK_ERROR_DEVICE_LOST`. On success DXGI Present and
  optional window show precede ring advance and native Present, which waits on that image's
  registered ready semaphore. The lease covers the forwarded Present, including source misses.

## Checked release and package residual

Checked bridge release closes idle admission, tries the presentation lease and reuses the exact
proof for every Vulkan slot fence and the shared D3D12 fence against the ring. An accepted upload
without a fenced hand-back, a busy lease, incomplete work or device removal retains ownership.
Release polls those fences. After proof, Rust takes the records, destroys the images and
commands, releases the DXGI chain, then drops the shared semaphore, fence, queue and device. It
resets the ring and reopens after destruction. Unity's intercepted swapchains and retained HWND
remain in the presentation registry.

Package cleanup still has a gap between the idle probe and checked release. The probe's entry
and lease end before `drained_and_ended` calls `rsf_fg12_end_session`. Package deactivation
clears the bridge's activity flag. Native Present can independently enter using the stored window
frame. An entry or a lease holder in this interval can make the later probe or close in
`rsf_unity_bridge_release` refuse. New submissions can also invalidate the earlier fence proof.
The refusal retains the Vulkan and bridge owners. The package has already invalidated temporal
readiness and destroyed its SR views, and the void callback then lets it clear `started` anyway.
`rsfu_shutdown` consequently reports success, and a restart can proceed to `open` without
retrying the retained bridge. A later native shutdown can attempt cleanup again; Unity's device
shutdown also proceeds to bridge unload after its best-effort retirement call.

The old C++ static `ComPtr` could also release the DXGI reference on DLL detach after a refusal.
The checked release path now owns the only `Release` of that reference. An actual unload
following a refused teardown can leave it retained until process exit, together with the
already Rust-owned shared graphics resources.

A return value alone would leave the view teardown ahead of the final proof. Covering the gap
requires a retained admission and proof through Runtime session retirement and view destruction,
with the package shutdown, restart and Unity device-shutdown paths accounted for. The package
also calls its D3D11 and D3D12 bridges through the same callback, while the Mono plugin has its
own D3D12 lifecycle. This change leaves those call paths as found.

## Verification scope

Source comparison covers the native leaves, both window routes, the pre-upload fallbacks,
the fence and forwarding order, the private C/Rust layout, and preservation of the public DLL
and managed ABI. Rust 1.94.1 formatting, locked offline workspace metadata and whitespace checks
passed. Native and Rust compilation, linking, Clippy, tests, resize and Present on hardware,
fault recovery and Unity shutdown still require user verification.
