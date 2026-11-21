# Unity Vulkan shared graphics device ownership

The source audit starts at `c5c143b5395d3c0a43b2d9ebe26ae6166033a215` on
`rust/orchestrator-anomaly` and follows the command resource work in the
[previous audit](rust-unity-vulkan-command-resources.md). The ownership question was whether
the bridge could publish and retire one shared graphics device and its dependent allocations
under the same admission and fence proof already used for its command slots.

## Creation and use

Rust claims creation before the native leaf reads `IUnityGraphicsVulkanV2::Instance`.
The bridge uses its Vulkan device, physical device, graphics queue and queue family. The LUID
selects the same DXGI adapter as before. The native leaf transfers each successful D3D12
device, queue and shared fence reference, and the Vulkan semaphore as soon as it is created.
The temporary Win32 shared handle closes after the import attempt. Unity still owns its
Vulkan device and queue. Rust keeps the claim through import, command pool and slot creation,
and success logging, then publishes the complete record in one step. Upon refusal, it frees
any slot resources and the pool before the semaphore and COM references, then clears the
claim. This lets another entry retry without observing an older partial record.

The render callback borrows the device under its running event and presentation lease.
Intercepted Present borrows under its entry, then takes the presentation lease for source
lookup, recording, submission and forwarding. Shared image preparation uses the same
device for the drain, allocations and imports. On allocation failure or a fault that arrives
during creation, it destroys the pending handles and keeps the previous image. Replacement
still requires the render-side drain. The callback packet and window frame still supply the
same frame ID, generation and host services. The presented image and ready semaphore still
come from Unity's intercepted swapchain and the presented image index.

The ring and both native submit helpers keep the base order. Vulkan submits the upload and
signals the next fence value, Rust records the accepted upload, then D3D12 waits. D3D12 executes
and signals the slot value, then Vulkan waits for it and signals the slot fence. Only that
accepted fenced submission clears partial-submission retention. Intercepted Present still
uses Unity's waits on upload, signals the registered ready semaphore on hand-back and forwards
Present with that semaphore. The package's internal graphics lookup now takes COM references
for its callers. The Vulkan branch takes them before leaving its entry, so adapter inspection
and Editor preview setup retain the device and queue through their use.

## Checked release

Idle probes take an entry and try the presentation lease before reading the same fences as
release. Release first closes admission, then takes the lease and checks each Vulkan slot fence
and the shared D3D12 fence against the ring. A busy lease, outstanding work, incomplete proof
or accepted upload without a fenced hand-back retains the resources. The D3D12 removal value
`UINT64_MAX` now refuses the proof and render-side drain. Normal queue waits and fence signal
order are preserved.

After proof, Rust takes the image and device records out of the state mutex. It destroys images,
slot resources and the pool, asks native code to reset the DXGI swapchain, then destroys the
imported semaphore and releases the shared fence, queue and device. The swapchain reset now
precedes semaphore destruction, keeping the queue and device live for both. Rust resets the
ring and reopens only after destruction. Allocation, reporting, fence reads and destruction run
outside the state mutex. The creation claim or closed admission prevents reentrant code from
publishing a second device during creation or destruction. Native package bridge bookkeeping
now also remains intact if the Vulkan release refuses after the initial drained check.

## Verification scope

Source comparisons cover the private Rust/C boundary, managed and DLL-facing signatures,
both submit paths, source identity, the presentation lease and proof, and rollback order.
Native DXGI swapchain ownership and resize remain separate. The wider package lifecycle still
has a void release callback, so it cannot observe a refusal after its earlier idle probe. Native
and Rust compilation/linking, Clippy, tests, real queue synchronization, callback timing, resize,
device removal and Unity teardown need user verification.
