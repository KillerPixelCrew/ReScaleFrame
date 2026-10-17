# Vulkan host integration

ReScaleFrame owns the native Vulkan provider bridges migrated from VulkanStory-Rewrite:
NGX SR, FidelityFX Vulkan SR/FG, FidelityFX DX12 SR interop, XeSS-FG interop and
Streamline Vulkan FG/Reflex/PCL. Their existing DLL and export names are retained
while the host migrates. These names are compatibility ABI, not a second source tree.

VulkanStory consumes this repository through its pinned `sdk/rescaleframe` submodule.
Its build scripts pass the host's pinned SDK and MinHook paths explicitly. The
FidelityFX and Streamline SDK build entries take `-HostRoot` to find those SDKs
and place corrected builds under that host's `artifacts/rescaleframe/` directory.
SDK sources remain unchanged; guarded corrections are applied to build copies.

FSR4 INT8 compatibility uses `runtime/backends/fsr/src/fsr4_compat_core.cpp` for
both the existing DX12 backend and the Vulkan interop bridge. The Vulkan bridge
passes the adapter it already matched to the physical device; the DX12 backend
resolves its own device's adapter before entering the same compatibility code.

This is the provider-source migration. VulkanStory's managed SR, FG and latency
controllers still need migration to ReScaleFrame; moving these bridges alone does
not finish orchestration integration or establish runtime acceptance.
