# FSR 4 bridge

`VulkanStoryFsr4.dll`: runs the FidelityFX DX12 upscaler for the Vulkan renderer through shared images and fences.
Moved from Optimum's `native/optimum-fsr4` (revision `386e0d0`).

Build on Windows with `g++`, `gcc`, the repository's pinned Rust toolchain with the
`x86_64-pc-windows-gnu` target, `VULKAN_SDK` set, and `sdk/minhook` initialized.
The SDK root defaults to the `sdk/fidelityfx` submodule when built through
`scripts/build-provider-bridges.ps1`.

```powershell
./runtime/vulkan/fsr4/build.ps1 -MinHookRoot <host>/sdk/minhook -SdkRoot <host>/sdk/fidelityfx -Output <dir>/VulkanStoryFsr4.dll
```

At runtime it loads `amd_fidelityfx_upscaler_dx12.dll` from the same directory.

NVIDIA and Intel run FSR 4 through the shared [Rust INT8 compatibility owner](../../backends/fsr/compat/src/lib.rs).
It requires a pinned upscaler build, SDK 2.3.0 (4.1.1.2740) or SDK 2.4.0 (4.1.2.4555),
and otherwise refuses creation.
