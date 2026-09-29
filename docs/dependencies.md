# Dependencies

The native build needs the Windows/C++ toolchain. Rust dependencies are pinned in `Cargo.toml` and `Cargo.lock`. MinGW-w64 and Wine support the optional Linux cross-build.

Vendor SDKs, reference checkouts, Epic source, and game binaries are kept outside version control. [The source map](research/source-map.md) records inspected revisions; Epic links require authorized access.

## Optional local SDKs

| Dependency | Inspected version | Local path | Purpose |
| --- | --- | --- | --- |
| RenderDoc header and x64 DLL | 1.45 | `vendor/renderdoc/` | In-application capture |
| Streamline headers | 2.14.1 | `vendor/streamline/include/sl.h` | Compile the DLSS adapter |
| Streamline/NGX x64 runtime | 2.14.1 release package | `vendor/streamline/bin/x64/` | Load and evaluate DLSS; `sl.dlss_g.dll`, `nvngx_dlssg.dll`, `sl.pcl.dll` and `sl.reflex.dll` are in the same package and are what frame generation will load |
| FidelityFX SDK headers | `60f4ea81909200d8542eca14dccb2628b763a9a3` (SDK 2.3.0; FSR 2.3.4, 3.1.5 and hardware-dependent 4.1.1) | `vendor/fidelityfx/Kits/FidelityFX/api/include/` and `upscalers/include/` | Compile the FSR adapter |
| FidelityFX x64 runtime | same | `vendor/fidelityfx/Kits/FidelityFX/signedbin/` | `amd_fidelityfx_loader_dx12.dll`, `amd_fidelityfx_upscaler_dx12.dll`, `amd_fidelityfx_framegeneration_dx12.dll` |
| XeSS SDK headers | 3.0.2, `8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0` | `vendor/xess/inc/` (`xess/`, `xess_fg/`, `xell/`) | Compile the XeSS adapter |
| XeSS x64 runtime | same | `vendor/xess/bin/` | `libxess.dll`, `libxess_dx11.dll`, `libxess_fg.dll`, `libxell.dll` |

Without the headers, the checkout still builds; the relevant API reports that the feature is unavailable. `RSF_RENDERDOC_DLL`, `RSF_STREAMLINE_BIN`, `RSF_FFX_BIN`, per-family `RSF_FSR2_BIN` / `RSF_FSR3_BIN` / `RSF_FSR4_BIN`, and `RSF_XESS_BIN` select runtime locations. See [loader setup](../loader/README.md). The new SR adapters use the SDK checkout layouts above. A single current AMD package can provide all three FSR algorithms; separate runtime directories and explicit version overrides also permit side-by-side SDK releases. See [the switching evidence](research/orchestrator-sr-switching.md). Vendor checkouts remain untracked.

RenderDoc and Streamline source/header licenses are separate from the licenses covering NVIDIA runtime binaries such as `nvngx_dlss.dll` and `nvngx_dlssg.dll`. FidelityFX headers and the signed DX12 DLLs are MIT with a notice file to ship beside them. The XeSS headers and samples carry Intel's SDK licence and the `libxess*.dll` / `libxell.dll` binaries have separate Intel redistribution terms; confirm both against the shipped text before any redistribution. Check the exact release's included terms and notices in every case. The AC7 release includes the retail Streamline interposer/common/DLSS/PCL DLLs and NVIDIA DLSS 310.9.1.0, unmodified, with the SDK licenses and notices. Frame generation, Reflex and other vendor feature runtimes are not part of this package. Vendor binaries stay untracked in Git.

The Skyrim and Fallout 4 Community Shaders presentation bridges, fo4test, OptiScaler and SpecialK were studied for the plan and not copied: they are GPL or carry their own exceptions, and `AGENTS.md` forbids copying reference code because it was useful to study.

First-party code and documentation use GPL-3.0-only; `sdk/game/` uses MIT. Reference projects retain their own licenses. Record provenance and check compatibility before incorporating their code.


## Packaging the AC7 release

Build and verify the Release configuration, build the Rust overlay with `cargo build --release
--locked -p rescaleframe-overlay`, and commit the release source. Use the official
[Streamline 2.14.1 SDK](https://github.com/NVIDIA-RTX/Streamline/releases/tag/v2.14.1), with the
unmodified retail DLLs from `bin/x64` and its accompanying license files.

```
python tools/package-ac7.py --streamline-root .local/streamline/sdk
```

The packager writes the Windows archive, corresponding-source archive and SHA256SUMS.txt under
`build/releases`. It refuses a dirty source tree, includes a source-commit/file-hash manifest,
collects dependency notices from pinned Cargo packages (using their exact recorded upstream
revision when a crate omits a root license), and verifies every archived file against its manifest.
The optional `--expected-proxy-sha256` also requires the exact game-tested proxy. It never reads or
writes the game installation. The recorded release packaging uses the same proxy and overlay bytes
accepted in the final briefing test.
