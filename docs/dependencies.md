# Dependencies and references

All first-party code lives in this repository. The initial native scaffold depends only on the Windows/C++ toolchain. The egui dependency is pinned in the Cargo workspace, with transitive versions recorded in `Cargo.lock`.

No vendor graphics SDK, reference repository, game binary, or Epic engine source is copied into this repository. The research source map records inspected revisions and upstream links. Epic links require authorized access.

Vendor backends will need separately managed SDK/runtime dependencies and their applicable notices. Before incorporating reference code, check that project's actual license and compatibility with ReScaleFrame's selected license. Studying a source file does not make it first-party code.

First-party runtime, loader, launcher, UI, game plugins, tools, and documentation use GPL-3.0-only. The Game SDK under `sdk/game/` uses MIT. No third-party runtime binaries are currently distributed.

## Development-only dependencies

These support research and are not part of any shipped artifact. Nothing here is committed; all of it lands in the untracked `vendor/` directory and the build treats it as optional.

| Item | Source | License | Used by |
| --- | --- | --- | --- |
| `renderdoc_app.h` | `baldurk/renderdoc`, tag `v1.45` | MIT | `loader/diagnostics/src/frame_capture.c` |
| `renderdoc.dll` (x64) | `renderdoc.org/stable/1.45/RenderDoc_1.45_64.zip` | MIT | loaded at runtime by the research proxy |

Fetch both into `vendor/renderdoc/`. Capture support compiles only when the header is present, so a checkout without it still builds and tests. The DLL is never loaded from the search path, only from the explicit path in `RSF_RENDERDOC_DLL`.

The in-application capture approach, and specifically the need to allow NVIDIA vendor extensions so that RenderDoc does not cause device removal on hybrid graphics, follow Skyrim Community Shaders' `src/Features/RenderDoc.cpp`. That is a studied approach, not copied code.

mingw-w64 and Wine support the Linux cross build described in `AGENTS.md`. Neither is required on Windows.
