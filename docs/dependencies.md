# Dependencies

## REA development tooling

`tools/rea` pins `rea-agents` 6.4.0 from npm, whose published Git revision is
`a56f7bb668ab23fec8d33f7af353ac7ed2fe1895` in [morluto/rea](https://github.com/morluto/rea).
The package and its bundled `reverse-engineer-anything` workflow are MIT licensed.
The shared skill is copied unchanged from that release, with its upstream license;
the npm lockfile records dependency versions and integrity hashes. Installed
`node_modules` and provider paths remain ignored. REA is used only for development
and research and is not linked into or shipped with the runtime.
[Installation and client scope](tooling.md#rea-for-codex-and-claude-code).

The headless IDA bridge pins MIT-licensed `mrexodia/ida-pro-mcp` revision
`c133c3853faa111a9b00ee615c013b720d0c4acd`, matching REA's verified IDA 9.3
integration, plus `idapro` 0.0.11 and `tomli-w` 1.2.0 in `ida-requirements.txt`.
Its Python packages stay under ignored `.local/rea/ida-venv`. IDA Pro and Hex-Rays
retain their commercial licenses and are not copied into Git or runtime packages.
[Setup and real-provider verification](tooling.md#ida-pro-through-rea-and-direct-mcp).

## Godot developer integration

The [Godot dependency lock](../integrations/godot/dependencies.json) pins Godot
4.7.2, godot-cpp, official standard/.NET archives, vendor SDKs and the Godot Mesa
NIR binary used by the Windows D3D12 engine build. Engine/reference checkouts,
SDKs and build tools remain ignored. The GDExtension links godot-cpp; the custom
engine builds the first-party module from this monorepo. The add-on preserves
the Godot/godot-cpp license texts and the vendor payload's bundled notices.

Python tooling uses the local environment and pinned SCons, NumPy and Pillow
requirements. .NET validation uses the matching official GodotSharp assemblies
and bundled NuGet packages, without changing their generated public API.
Read the [build and validation record](research/godot-engine-integration.md) for
exact revisions, binary hashes, exported-player coverage and image failures.

## Unity Mono DX12 adapter

The local character-shader patch builder pins UnityPy 1.25.0 through its inline uv
metadata. It also requires a caller-supplied 3DMigoto-compatible shader decompiler and
the installed Windows D3DCompiler 47. These are development tools, not runtime payloads.
Original and derived game shader data stays in ignored local output; no game shader
source or binary is committed. [Patch evidence and limits](research/drag-n-wash-character-motion-20261020.md).

The managed helper uses Harmony 2.4.2 (MIT), pinned by its NuGet lockfile, with the executable
net472 Harmony assembly for the shipped Mono runtime. The RSF helper targets netstandard2.1.
Unity assemblies are compile references from the public Unity SDK or a prepared reference
cache; a game installation is not required and these assemblies are never included in the ZIP.
`eng/prepare-unity-references.ps1` gathers engine modules from Unity 6000.3.14f1 and URP
assemblies from an SDK template cache or a separately supplied URP reference directory.
Its ignored `references.json` records the engine revision, URP reference version and hashes.
Use a provisioned SDK/reference cache in CI; running the Unity Editor to compile a public
URP project requires a valid Unity license. The native adapter uses
the public Unity rendering interfaces from NativeRenderingPlugin revision
`522254181faf188efa8b50c3e3bf6fce720b26e4` under `vendor/unity-native/include`; those headers
carry Unity's Companion License and stay untracked. Preserve that notice with distributed
Unity-dependent artifacts. Build scripts require these headers and compile references;
they do not install tools or rewrite game assemblies. See the
[0.1.1 build record](releases/v0.1.1-drag-n-wash.md) for this release's actual reference versions
and the distinction between compilation and target-player contract validation.

The package includes Streamline/DLSS, FidelityFX and XeSS runtime notices, Harmony's
license, the first-party GPL license, Game SDK MIT license, Rust notices and MinHook's license.
Its payload comes from Release artifacts and vendor SDKs, with an explicit allowlist that
excludes game binaries, game assemblies, decompiles, logs, backups and machine-specific
original-file baselines. The older verified-deployment input remains an optional mode.

The native build needs the Windows/C++ toolchain. Rust dependencies are pinned in `Cargo.toml` and `Cargo.lock`. MinGW-w64 and Wine support the optional Linux cross-build.

Vendor SDKs, reference checkouts, Epic source, and game binaries are kept outside version control. [The source map](research/source-map.md) records inspected revisions; Epic links require authorized access.

## Optional local SDKs

The RTX 40 MFG path incorporates selected MIT-licensed MFG-Unlocker sources at
`2b52823e3769c31cf52d3b368c6963cff8fc5344`, with its notice in
`runtime/backends/dlss/third_party/mfg_unlocker/LICENSE`. It derives the guarded temporal
program from the installed NVIDIA runtime in memory. No external GPU cache, Intel-derived
shader data or MFGUnlocker DLL is included. Distributions using this path must retain the
MIT notice alongside the existing vendor runtime terms. [Integration](research/rtx40-mfg-unlock.md).

| Dependency | Inspected version | Local path | Purpose |
| --- | --- | --- | --- |
| RenderDoc header and x64 DLL | 1.45 | `vendor/renderdoc/` | In-application capture |
| Streamline headers | 2.14.1 | `vendor/streamline/include/sl.h` | Compile the DLSS adapter |
| Streamline/NGX x64 runtime | 2.14.1 release package | `vendor/streamline/bin/x64/` | Load and evaluate DLSS; `sl.dlss_g.dll`, `nvngx_dlssg.dll`, `sl.pcl.dll` and `sl.reflex.dll` are in the same package and are loaded by current-source FG/Reflex |
| FidelityFX SDK headers | `60f4ea81909200d8542eca14dccb2628b763a9a3` (SDK 2.3.0; FSR 2.3.4, 3.1.5 and hardware-dependent 4.1.1) | `vendor/fidelityfx/Kits/FidelityFX/api/include/` and `upscalers/include/` | Compile the FSR adapter |
| FidelityFX x64 runtime | same | `vendor/fidelityfx/Kits/FidelityFX/signedbin/` | `amd_fidelityfx_loader_dx12.dll`, `amd_fidelityfx_upscaler_dx12.dll`, `amd_fidelityfx_framegeneration_dx12.dll` |
| XeSS SDK headers | 3.0.2, `8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0` | `vendor/xess/inc/` (`xess/`, `xess_fg/`, `xell/`) | Compile the XeSS adapter |
| XeSS x64 runtime | same | `vendor/xess/bin/` | `libxess.dll`, `libxess_dx11.dll`, `libxess_fg.dll`, `libxell.dll` |

Without the headers, the checkout still builds; the relevant API reports that the feature is unavailable. `RSF_RENDERDOC_DLL`, `RSF_STREAMLINE_BIN`, `RSF_FFX_BIN`, per-family `RSF_FSR2_BIN` / `RSF_FSR3_BIN` / `RSF_FSR4_BIN`, and `RSF_XESS_BIN` select runtime locations. See [loader setup](../loader/README.md). The new SR adapters use the SDK checkout layouts above. A single current AMD package can provide all three FSR algorithms; separate runtime directories and explicit version overrides also permit side-by-side SDK releases. See [the switching evidence](research/orchestrator-sr-switching.md). Vendor checkouts remain untracked.

RenderDoc and Streamline source/header licenses are separate from the licenses covering NVIDIA runtime binaries such as `nvngx_dlss.dll` and `nvngx_dlssg.dll`. The FidelityFX signed DX12 DLLs carry the SDK release's bundled binary redistribution terms; ship `Kits/FidelityFX/docs/license.md` and `3rdpartynotice.md` beside them. The earlier statement grouping these signed binaries under MIT was incorrect for the inspected SDK 2.3 package. The XeSS headers and samples carry Intel's SDK licence and the `libxess*.dll` / `libxell.dll` binaries have separate Intel redistribution terms; confirm both against the shipped text before any redistribution. Check the exact release's included terms and notices in every case. The AC7 release includes the retail Streamline interposer/common/DLSS/PCL DLLs and NVIDIA DLSS 310.9.1.0, unmodified, with the SDK licenses and notices. The corrected AC7 package also includes FidelityFX SDK 2.3.0 upscaling (FSR 2/3/4) and XeSS SR 2.0.2 with their terms and notices. Frame generation and Reflex are not included in that published SR package. Current-source
deployments use separate FG DLLs and shared latency services. Vendor binaries stay untracked in Git.

The Skyrim and Fallout 4 Community Shaders presentation bridges, fo4test, OptiScaler and SpecialK were studied for the plan and not copied: they are GPL or carry their own exceptions, and `AGENTS.md` forbids copying reference code because it was useful to study.

First-party code and documentation use GPL-3.0-only; `sdk/game/` uses MIT. Reference projects retain their own licenses. Record provenance and check compatibility before incorporating their code.


## Packaging the AC7 release

For an existing local AC7 installation, deploy the SR dependencies explicitly with
`eng/deploy-ac7-sr-runtimes.ps1 -GameDirectory <directory-containing-Ace7Game.exe>`. The helper
uses the vendor paths above, copies the SR DLLs with their notices and verifies destination
hashes. Missing FidelityFX/XeSS runtime directories cause LOAD_FAILED refusals even when the
proxy was built with their headers. [Diagnosis and deployment evidence](research/sr-runtime-deployment.md).

Build and verify the Release configuration, build the Rust overlay with `cargo build --release
--locked -p rescaleframe-overlay`, and commit the release source. Use the official
[Streamline 2.14.1 SDK](https://github.com/NVIDIA-RTX/Streamline/releases/tag/v2.14.1), with the
unmodified retail DLLs from `bin/x64` and its accompanying license files.

```
python tools/package-ac7.py --streamline-root .local/streamline/sdk --fidelityfx-root vendor/fidelityfx --xess-root vendor/xess
```

The packager writes the Windows archive, corresponding-source archive and SHA256SUMS.txt under
`build/releases`. It refuses a dirty source tree, includes a source-commit/file-hash manifest,
collects dependency notices from pinned Cargo packages (using their exact recorded upstream
revision when a crate omits a root license), and verifies every archived file against its manifest.
The current-source package includes the dedicated AC7 proxy, game plugin, Rust overlay,
retail Streamline/DLSS/DLSS-G/Reflex files, FidelityFX SR/FG and XeSS SR/FG/XeLL
runtimes with their licenses and notices. The published v0.1.0 download remains SR-only.
The optional `--expected-proxy-sha256`, `--expected-plugin-sha256` and `--expected-overlay-sha256`
require the exact validated binaries. FSR/XeSS runtime hashes are pinned to the tested SDK files.
The packager never reads or writes the game installation. As corrected on 10 October,
its proxy input matches the deployer: `build/windows-x64/ac7/bin/Release/dinput8.dll`.
The allowlist includes generation dependencies; its development README and manifest
distinguish this package from the historical SR release and fresh AC7 gameplay acceptance.

## Current-source frame-generation runtime files

DLSS-G requires matched Streamline interposer/common/DLSS-G/Reflex/PCL and NGX FG files; shared
DLSS SR also needs the SR plugin and NGX SR DLL. FSR3/4 FG uses
`amd_fidelityfx_framegeneration_dx12.dll` with the loader/upscaler set from the pinned SDK. XeSS
FG uses `libxess_fg.dll` and `libxell.dll`. Keep each SDK's notices with its runtime family.
FSR4 SR INT8 compatibility does not establish FSR4 FG support.

`eng/deploy-unity-sr.ps1` copies all three SR/FG families even when its `-FrameGeneration` default
is Off. `eng/package-unity-sr.ps1` packages local Release artifacts and the allowlisted vendor
SDK payload without reading a game directory. It defaults SR and FG independently to Auto,
sets SR Quality, excludes preference files and verifies every archived file hash. Optional
`-GameDirectory` input verifies an existing deployment before replacing its first-party files
with the Release artifacts. See [Unity guide](../games/unity-mono/README.md)
and [current status](current-status.md).
