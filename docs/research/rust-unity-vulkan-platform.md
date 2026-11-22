# Unity Vulkan platform cutover

This source audit compares the Unity Vulkan route with `84a8b9eb`. It follows
both `execute_admitted` and `present_admitted` from their admitted frame through
the shared device, image import, two submissions, Present and retirement.

| U04 PORT path | Source disposition |
| --- | --- |
| `games/unity-mono/src/vulkan_state.h` | The three GPU ownership records now generate `vulkan_handles.rs` using bindgen. First-use layout, recording serial and submission-receipt routes are internal Rust functions in `vulkan_state.rs`. The remaining declarations connect Unity's native events, presentation and package lifecycle to Rust. |
| `integrations/unity/native/vulkan_bridge.cpp` | The typed graphics operations moved to `vulkan_platform.rs`, `vulkan_platform/commands.rs` and `vulkan_platform/surface.rs`. Unity SDK calls, Win32 child-window handling, foreign callback containment and Vulkan loader/Present forwarding remain at the native boundary. |
| `integrations/unity/native/vulkan_bridge.h` | The native package interface contains Unity entry points, window management and the preview caller's COM hand-off. The raw GPU call contract is now separately declared in `vulkan_gpu.h`. No public Unity or GameSDK ABI changed. |

## Shared device and images

`rsf_uvk_gpu_create_device` resolves a dispatch table using Unity's instance for
physical-device queries and its device for device and queue commands. A missing
entry refuses the shared device before allocating GPU resources. The original
loader-level `vkQueuePresentKHR` continuation still dispatches arbitrary incoming
queues. The owned table lives with the shared device record and releases after
the last child Vulkan call.

The Vulkan LUID selects the DXGI adapter before D3D12 device creation. Each
image keeps the D3D12 committed resource in COMMON, the Vulkan 2D transfer
image and the imported dedicated memory as one record. The port keeps the
same property flags, memory type selection, Win32 handle type and
import-handle closure. Rust publishes successful handles as each step
completes, so the existing rollback can destroy partial allocations.
`prepare_image` still drains all GPU users before replacing any of the six
shared entries across the three slots. The Vulkan drain waits on Unity's
queue and then proves the D3D12 fence through the ring's submitted value.
Checked shutdown polls each Vulkan slot fence and the D3D12 fence, and
releases images, slots and pool, the DXGI chain, then the shared device.

## Queue and presentation contracts

`rsf_uvk_gpu_begin` waits for the slot fence and resets both Vulkan command
buffers and the D3D12 allocator and list. It then reports the same
`(epoch, slot, serial, serial - 1)` receipt to the U02 per-view owner.
The ring epoch still begins at shared-device publication and retires
through its existing callback. Image layout first use follows the upload
receipt, with external queue-family acquire and release barriers.

`rsf_uvk_gpu_upload` ends Vulkan buffer 0, waits on any original Present
semaphores, and signals the imported D3D12 fence value. It records
successful Vulkan submission before advancing that value and asking the
D3D12 queue to wait. `rsf_uvk_gpu_submit` closes the D3D12 list and Vulkan
buffer 1, reserves the next interop value, executes and signals D3D12,
and submits the Vulkan hand-back with the slot fence. Only successful
fenced hand-back clears the partial-upload receipt. Native event and
Present callbacks retain their original ring advancement points.

`rsf_uvk_gpu_chain_create` creates the real Win32 child HWND's
three-buffer, RGBA8, flip-discard DXGI swapchain on the shared D3D12
queue. Its resize remains gated by the shared drain. Native Present
validates the window receipt and obtains the current DXGI buffer before
resetting the slot or consuming Unity's waits. The copied buffer stays
referenced through DXGI Present and the forwarded Vulkan Present. The
forwarded Present uses the hand-back semaphore. Unity-facing and host
callback handling, including the existing C++ catch boundaries, remain
in the native package.

## Verification

Rust 1.94.1 formatting, targeted bindgen 0.72.1 regeneration, locked
offline Cargo metadata and Git whitespace checks cover this source port.
Windows compile and link, device fixtures and Unity player acceptance
of submission, resize and teardown remain open.
