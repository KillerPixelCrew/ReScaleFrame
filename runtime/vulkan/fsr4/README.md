# FSR 4 bridge

`VulkanStoryFsr4.dll`: runs the FidelityFX DX12 upscaler for the Vulkan renderer through shared images and fences.
Migrated from Optimum's `native/optimum-fsr4` (revision `386e0d0`); its provenance and notices apply.

Build (Windows, `g++` and `gcc`, `VULKAN_SDK` set, `sdk/minhook` initialized); the SDK root defaults to the `sdk/fidelityfx` submodule when built through `scripts/build-provider-bridges.ps1`:

```powershell
./runtime/vulkan/fsr4/build.ps1 -MinHookRoot <host>/sdk/minhook -SdkRoot <host>/sdk/fidelityfx -Output <dir>/VulkanStoryFsr4.dll
```

At runtime it loads `amd_fidelityfx_upscaler_dx12.dll` from the same directory.

NVIDIA and Intel run FSR 4 through the INT8 compatibility hook in `runtime/backends/fsr/src/fsr4_compat_core.cpp`, shared with the DX12 backend (GPL-3.0-only); it requires a pinned upscaler build, SDK 2.3.0 (4.1.1.2740) or SDK 2.4.0 (4.1.2.4555), and otherwise refuses creation.
