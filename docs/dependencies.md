# Dependencies

The native build needs the Windows/C++ toolchain. Rust dependencies are pinned in `Cargo.toml` and `Cargo.lock`. MinGW-w64 and Wine support the optional Linux cross-build.

Vendor SDKs, reference checkouts, Epic source, and game binaries are kept outside version control. [The source map](research/source-map.md) records inspected revisions; Epic links require authorized access.

## Optional local SDKs

| Dependency | Inspected version | Local path | Purpose |
| --- | --- | --- | --- |
| RenderDoc header and x64 DLL | 1.45 | `vendor/renderdoc/` | In-application capture |
| Streamline headers | 2.12.0 | `vendor/streamline/include/sl.h` | Compile the DLSS adapter |
| Streamline/NGX x64 runtime | 2.12.0 release package | `vendor/streamline/bin/x64/` | Load and evaluate DLSS |

Without the headers, the checkout still builds; the relevant API reports that the feature is unavailable. `RSF_RENDERDOC_DLL` and `RSF_STREAMLINE_BIN` select runtime locations. See [loader setup](../loader/README.md).

RenderDoc and Streamline source/header licenses are separate from the licenses covering NVIDIA runtime binaries such as `nvngx_dlss.dll`. Check the exact release's included terms and notices before redistribution. ReScaleFrame does not currently include these binaries.

First-party code and documentation use GPL-3.0-only; `sdk/game/` uses MIT. Reference projects retain their own licenses. Record provenance and check compatibility before incorporating their code.
