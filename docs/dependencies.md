# Dependencies

The native build needs the Windows/C++ toolchain. Rust dependencies are pinned in `Cargo.toml` and `Cargo.lock`. MinGW-w64 and Wine support the optional Linux cross-build.

Vendor SDKs, reference checkouts, Epic source, and game binaries are kept outside version control. [The source map](research/source-map.md) records inspected revisions; Epic links require authorized access.

## Optional local SDKs

| Dependency | Inspected version | Local path | Purpose |
| --- | --- | --- | --- |
| RenderDoc header and x64 DLL | 1.45 | `vendor/renderdoc/` | In-application capture |
| Streamline headers | 2.12.0 | `vendor/streamline/include/sl.h` | Compile the DLSS adapter |
| Streamline/NGX x64 runtime | 2.12.0 release package | `vendor/streamline/bin/x64/` | Load and evaluate DLSS; `sl.dlss_g.dll`, `nvngx_dlssg.dll`, `sl.pcl.dll` and `sl.reflex.dll` are in the same package and are what frame generation will load |
| FidelityFX SDK headers | `60f4ea81909200d8542eca14dccb2628b763a9a3` (FSR 3.1 / FrameGeneration 4.0.1) | `vendor/fidelityfx/include/` (planned; `ffx_api.h`, `ffx_upscale.h`, `ffx_framegeneration.h`, `dx12/`) | Compile the FSR adapter |
| FidelityFX x64 runtime | same | `vendor/fidelityfx/bin/x64/` (planned) | `amd_fidelityfx_loader_dx12.dll`, `amd_fidelityfx_upscaler_dx12.dll`, `amd_fidelityfx_framegeneration_dx12.dll` |
| XeSS SDK headers | 3.0.2, `8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0` | `vendor/xess/inc/` (planned; `xess/`, `xess_fg/`, `xell/`) | Compile the XeSS adapter |
| XeSS x64 runtime | same | `vendor/xess/bin/x64/` (planned) | `libxess.dll`, `libxess_dx11.dll`, `libxess_fg.dll`, `libxell.dll` |

Without the headers, the checkout still builds; the relevant API reports that the feature is unavailable. `RSF_RENDERDOC_DLL`, `RSF_STREAMLINE_BIN`, and (once the backends exist) `RSF_FFX_BIN` and `RSF_XESS_BIN` select runtime locations. See [loader setup](../loader/README.md). The FidelityFX and XeSS rows are the layout the [representation plan](representation-plan.md) specifies; neither directory exists yet.

RenderDoc and Streamline source/header licenses are separate from the licenses covering NVIDIA runtime binaries such as `nvngx_dlss.dll` and `nvngx_dlssg.dll`. FidelityFX headers and the signed DX12 DLLs are MIT with a notice file to ship beside them. The XeSS headers and samples carry Intel's SDK licence and the `libxess*.dll` / `libxell.dll` binaries have separate Intel redistribution terms; confirm both against the shipped text before any redistribution. Check the exact release's included terms and notices in every case. ReScaleFrame does not currently include any of these binaries.

The Skyrim and Fallout 4 Community Shaders presentation bridges, fo4test, OptiScaler and SpecialK were studied for the plan and not copied: they are GPL or carry their own exceptions, and `AGENTS.md` forbids copying reference code because it was useful to study.

First-party code and documentation use GPL-3.0-only; `sdk/game/` uses MIT. Reference projects retain their own licenses. Record provenance and check compatibility before incorporating their code.
