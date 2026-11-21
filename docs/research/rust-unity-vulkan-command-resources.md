# Unity Vulkan command resource ownership

The baseline is `4598dac`. This change moves the lifetimes of the command pool and slots
from `integrations/unity/native/vulkan_bridge.cpp` to `games/unity-mono/src/vulkan_state.rs`.
The source audit followed shared-device creation through both the Unity queue callback and
intercepted Present, then traced the idle probe and release back to the same fences. Both
recording paths use the presentation lease and select their slot from `bridge_interop.rs`.

## Allocation, submission and retirement

- Rust creates one pool and three slots under the existing shared-device claim. The native
  leaves use Unity's graphics queue family, two primary command buffers per slot, an initially
  signalled Vulkan fence, a direct D3D12 allocator and a direct command list closed before use.
  Rust publishes only a complete set. A failed leaf leaves the successful handles with the
  pending owner. It destroys the slot fences and releases the COM references before destroying
  the pool, which frees the buffers. Native shared-device rollback follows.
- The render event and intercepted Present borrow the exact ring slot under the existing
  presentation lease. Buffer 0 still uploads and signals the imported D3D12 fence. The D3D12
  queue waits, executes the render work and signals the next fence value. Buffer 1 waits for
  that value, performs the Vulkan hand-back and signals the slot fence. Only an accepted
  fenced hand-back clears the partial-submission state. The intercepted Present keeps the
  lease through forwarding.
- Retirement requires a closed idle bridge, the presentation lease, every slot fence
  signalled and the shared D3D12 fence completed through the interop ring's submitted value.
  Rust removes the image and command records before native destruction. Images go first,
  then the slots and the pool, then the native shared objects. The idle probe observes the
  same fences. Fence waits for reuse and queue waits for render-side drain and swapchain
  retirement stay on their existing paths.

The DXGI back buffer lookup now runs before slot reset and before the Vulkan upload consumes
Unity's Present waits. On lookup failure, Present faults the bridge and forwards with the
original waits. A failed slot checkout or reset also faults before forwarding, so the next
Present cannot retry a partially reset slot. The shared semaphore enters native ownership
only after Vulkan creation succeeds.

## Remaining ownership and verification

C++ still owns the Unity interface and instance, the D3D12 device, queue and shared fence,
the imported Vulkan semaphore, and the DXGI swap chain. It keeps the Unity texture access,
window management, COM and Vulkan calls, transfer barriers, typed effect calls, queue
submission and wait operations. Native swapchain intercepts and semaphore retirement
continue to use the existing Rust presentation registry. The shared-device borrowing
contract and GPU teardown across the Unity, Vulkan, D3D12 and DXGI owners still need live
validation. An accepted upload without a fenced hand-back retains the bridge resources
under the existing proof.

Source comparison against `4598dac` checked the native signatures, graphics operation order
and wait sites. The Rust 1.94.1 formatter and `git diff --check` passed. Native compile and link,
Windows and Vulkan device checks, injected Unity playback, resize, failure recovery and
teardown remain open.
