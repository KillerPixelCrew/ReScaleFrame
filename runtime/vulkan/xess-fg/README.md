# XeSS frame-generation bridge

`VulkanStoryXessFg.dll`: drives XeSS-FG and XeLL through a DX12 presenter fed from the Vulkan renderer. Diagnostic environment keys use the `VULKANSTORY_XESS` prefix.
Migrated from Optimum's `native/optimum-xess-fg` (revision `386e0d0`); its provenance and notices apply.

Build (Windows, `g++`, `VULKAN_SDK` set); the SDK root defaults to the `sdk/xess` submodule when built through `scripts/build-provider-bridges.ps1`:

```powershell
./runtime/vulkan/xess-fg/build.ps1 -SdkRoot <host>/sdk/xess -Output <dir>/VulkanStoryXessFg.dll
```

At runtime it loads `libxess_fg.dll` and `libxell.dll` from the same directory.
