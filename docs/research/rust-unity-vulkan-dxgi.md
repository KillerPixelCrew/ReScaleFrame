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

## Checked release in the swapchain port

Checked bridge release closes idle admission, tries the presentation lease and reuses the exact
proof for every Vulkan slot fence and the shared D3D12 fence against the ring. An accepted upload
without a fenced hand-back, a busy lease, incomplete work or device removal retains ownership.
Release polls those fences. After proof, Rust takes the records, destroys the images and
commands, releases the DXGI chain, then drops the shared semaphore, fence, queue and device. It
resets the ring and reopens after destruction. Unity's intercepted swapchains and retained HWND
remain in the presentation registry.

At `f68978a`, package cleanup had a gap between the idle probe and checked release. The probe's entry
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

## Package retirement after `f68978a`

`rsfu_shutdown`, restart through `rsfu_initialize`, and Unity's graphics-device shutdown use the
same `package_state.rs` retirement sequence under the package mutex. The native close deactivates
packet and CPU producers. Queued render callbacks can return their packets, and callbacks already
using the session finish before the shared frame owner closes admission. Vulkan also stops new
bridge entries, so the last saved window frame cannot keep generating work during a retry.
Already admitted queue events and intercepted presents retain their entries through recording,
forwarding, and fault handling. A new Vulkan Present forwards Unity's original call while bridge
admission is stopped.

The verified close carries through `rsf_fg12_end_session`, invalidation of temporal readiness,
SR view destruction, and bridge destruction. The native commit consumes proof from the close
without an intervening drain or lease acquisition. A native close refusal runs no Runtime or view
teardown. A Runtime refusal gives up the Vulkan presentation lease and keeps admission closed,
the same session ID, the views, and all bridge resources. Each live-device retry checks the
retained submissions again. This leaves the presentation lease available to Unity's swapchain
registration and destruction between attempts. A partially opened session is also retired
before another open, because player Vulkan surface setup can acquire a device before returning a
managed API. After commit the package clears its API and input frame. Later shutdown calls use
that stopped state without probing released resources again. The next open resumes Vulkan entries
and the next activation reopens frame callbacks after preparing the bridge.

| Consumer | Proof and destruction order |
| --- | --- |
| Vulkan player and Editor bridge | Close after entries, device creation and queued events are idle. Under the presentation lease, poll all Vulkan command-slot fences and compare the shared D3D12 fence with the interop ring's last submission. An accepted upload with no fenced hand-back prevents close, and the device-removal sentinel refuses proof. Destroy images and commands, then the DXGI chain, then the shared semaphore, fence, queue and device. Reset the ring and release the lease with bridge entries still closed. Unity swapchain identities and their ready semaphores remain in the registry for the existing swapchain destruction callback. |
| Package D3D12 | Close shared callback admission before polling Unity's frame fence. Use both the submitted command slots and the latest inline pre-UI copy completion. Runtime ends the retained session before SR teardown, then the bridge unhooks its message hook and clears its command slots, packet leases, fence and services. The separately linked Mono plugin keeps its previous quiesce, drain, managed stop and release sequence. |
| Package D3D11 | Poll the bridge D3D12 completion counter and the D3D11 copy proof. Copies are marked before recording and the D3D11 shared fence value is published after its signal and flush. The output copy after the existing hand-back wait adds a D3D11 signal and flush so the final copy has proof even if Unity stops producing frames. The earlier native submission and hand-back calls keep their order. A copy with no completed covering fence retains ownership. After proof and view destruction the bridge releases its shared surfaces, fence, commands and graphics references. |
| Editor preview | Preview records directly onto its queue and has its own fence. After bridge producers close, its submitted fence must also complete. Commit clears preview slot source references and the DXGI chain before its queue and device, then destroys the renderer bridge. The preview window remains available for reuse. |

`rsfu_shutdown` returns `0` while any checked owner refuses and `1` after commit or when the
device is already stopped. `rsfu_initialize` returns the existing retry result `2` while a live
bridge refuses retirement or Unity has yet to initialize the device. Unity's device-shutdown
callback now also ends the Runtime session. It clears the renderer interfaces only after commit.
If that callback refuses, later package initialization returns `0` and Unity initialization
events leave the retained bridge alone. That callback returns `void`, so Unity can destroy the
device even when RSF retains objects. A later callback cannot safely poll the old borrowed
`VkDevice`; recovery then requires process restart. Unity can also unload the DLL despite the
refusal. That path can still run native static destructors or leave Rust-owned resources retained
until process exit. The Unity-owned Vulkan resolver/swapchain callback lifetime and the retained
window procedures require host or process lifetime guarantees outside the retirement transaction.

Runtime's `rsf_fg12_end_session` reports refusal for matching source or vendor work that still
owns a slot. Its internal latency cleanup is a void call whose CPU ledger can decline a different
session. No new Runtime ABI is used to report that separate outcome. D3D11's normal live surface
replacement and Unity's cancellation of already queued callbacks at physical device loss also
remain outside the checked package cleanup.

## Verification scope

Source comparison with `f68978a` covers the unchanged managed and public C signatures, the
43-slot private C/Rust table, the Mono lifecycle calls, Vulkan queue and admitted Present order,
and the checked close, Runtime refusal and destruction sequence. Rust 1.94.1 formatting, locked
offline workspace metadata and whitespace checks passed. Compilation and linking, Clippy, tests,
player and device checks, deployment and push are pending under the source-only scope. In
particular the new D3D11 tail signal and flush, simultaneous Vulkan shutdown and Present, Editor
preview restart, and faulted physical device shutdown have no runtime verification on this commit.
