# FSR 4 bridge

`VulkanStoryFsr4.dll`: runs the FidelityFX DX12 upscaler for the Vulkan renderer through shared images and fences.
Moved from Optimum's `native/optimum-fsr4` (revision `386e0d0`).

Build on Windows with `g++`, `gcc`, the repository's pinned Rust toolchain with the
`x86_64-pc-windows-gnu` target, `VULKAN_SDK` set, and a MinHook checkout and the FidelityFX SDK available.
This repository has no submodules: the VulkanStory host keeps them as its own `sdk/minhook` and
`sdk/fidelityfx` submodules and passes them with `-MinHookRoot` and `-SdkRoot`.

```powershell
./runtime/vulkan/fsr4/build.ps1 -MinHookRoot <host>/sdk/minhook -SdkRoot <host>/sdk/fidelityfx -Output <dir>/VulkanStoryFsr4.dll
```

At runtime it loads `amd_fidelityfx_upscaler_dx12.dll` from the same directory.

The bridge state lives in the Rust [owner](owner.rs), compiled into the same
`rsf-fsr4-compat` archive that the Runtime and this DLL already link. It owns
argument and frame validation, provider selection and the FSR 4 pin, the
compatibility decision, allocator and fence sequencing, the ready/done handoff
and checked retirement. The Rust [capsule](native.rs) holds the DXGI, D3D12
and FidelityFX objects behind the owner's private callback table and defines
every export. The DLL has no C++ translation unit; `g++` only drives the link.

NVIDIA and Intel run FSR 4 through the shared [Rust INT8 compatibility owner](../../backends/fsr/compat/src/lib.rs).
It requires a pinned upscaler build, SDK 2.3.0 (4.1.1.2740) or SDK 2.4.0 (4.1.2.4555),
and otherwise refuses creation.
