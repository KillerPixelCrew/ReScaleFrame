# Streamline Vulkan corrections

`prepare.py` applies exact-anchor corrections to an isolated copy of Streamline
2.14.1 (`2122257e0fce486f91b385aa63b9a09b0a34b363`). The vendor checkout stays unchanged.

The pacer uses one binary present semaphore per real WSI image. Where the host
enables swapchain maintenance1, SDK-owned present fences establish completion
before destroying these semaphores. Existing caller-provided fences are retained.
NGX receives the SDK's corrected Vulkan dispatch callbacks. SDK-created images
enter their first GPU layout per mip, layer and aspect, preserving combined
depth/stencil transitions. Private copy/clear operations include transfer accesses
in memory dependencies. Acquire timeouts retain busy slots, and successful
suboptimal acquisition remains visible to the caller.

These follow the [present semaphore reuse guidance](https://docs.vulkan.org/guide/latest/swapchain_semaphore_reuse.html),
[presentation fence contract](https://docs.vulkan.org/refpages/latest/refpages/source/VkSwapchainPresentFenceInfoKHR.html),
and [synchronization and image layout rules](https://docs.vulkan.org/spec/latest/chapters/synchronization.html).
NVIDIA tracks the upstream pacer semaphore defect in [Streamline #112](https://github.com/NVIDIA-RTX/Streamline/issues/112).

`../build-sdk.ps1` builds Production `sl.common.dll` and `sl.interposer.dll`.
The interposer embeds the common DLL's SHA-256; the bridge embeds the resulting
interposer SHA-256. A verified handle denies writes and replacement while either
file is loaded. All closed NVIDIA feature libraries retain Authenticode checks.
The runtime receipt and applied replacement manifest accompany the package.

`dispatch-tests.cpp` checks first-use layout, combined depth/stencil aspects,
partial mip initialization and preservation of initialized image contents.
