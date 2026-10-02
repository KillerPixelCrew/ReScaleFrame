# Development and research tools

Check what is already installed and recommend only the tools needed for the current task. Building the runtime, examining a binary, and replaying a frame need different environments.

## Build

The repository pins Rust in `rust-toolchain.toml` and selects Visual Studio 2022 in `CMakePresets.json`, which is what CI's `windows-2022` runner has. A machine with Visual Studio 2026 instead uses the `windows-x64-vs18` presets, or `eng/verify.ps1 -VS2026`; the build is otherwise the same.

| Tool | Purpose |
| --- | --- |
| [Git](https://git-scm.com/downloads) | Repository and reference revisions |
| [Visual Studio / C++ Build Tools](https://visualstudio.microsoft.com/downloads/) | VS 2022 or 2026 C++ tools and Windows SDK for the reference x64 build |
| [CMake](https://cmake.org/download/) | 3.25+ for the checked-in presets. Configuring fetches [MinHook](https://github.com/TsudaKageyu/minhook) v1.3.4 (BSD-2-Clause) from GitHub for the Present and cursor detours, so the first configure needs the network |
| [Streamline SDK](https://github.com/NVIDIA-RTX/Streamline/releases) | `sl.h` and its headers under `vendor/streamline/include` (untracked), or the DLSS backend compiles out and reports `RSF_DLSS_ERROR_NOT_COMPILED`; the game folder's `ReScaleFrame\streamline` holds the release's `bin/x64` |
| [Rust via rustup](https://rustup.rs/) | Pinned Rust toolchain and egui tests |
| [PowerShell](https://github.com/PowerShell/PowerShell) | `eng/verify.ps1` |
| [Python 3](https://www.python.org/downloads/) | PE, capture, and view-buffer tools |

Run `./eng/verify.ps1` and `cargo test --workspace --locked`. The script currently runs Clippy but does not execute Rust tests. Linux cross-builds additionally use MinGW-w64 and Wine; see [AGENTS.md](../AGENTS.md).

## Binary and frame analysis

Use [Ghidra](https://github.com/NationalSecurityAgency/ghidra) with PyGhidra and the JDK required by the installed release. The repository's scripts target Ghidra 12+. [The Ghidra tooling guide](../tools/ghidra/README.md) covers DirectX types and compiler-derived archives.

Use [RenderDoc](https://github.com/baldurk/renderdoc) for capture and replay. The recorded AC7 workflow used 1.45: the Windows DLL captured through DXVK under Proton, Windows `renderdoccmd` converted XML under Wine, and Windows replay supplied pixels. Keep this evidence separate from assumptions about other versions/platforms.

The accepted AC7 build uses [Streamline 2.14.1](https://github.com/NVIDIA-RTX/Streamline/releases/tag/v2.14.1)
and the runtimes recorded in [dependencies](dependencies.md). Check existing local SDK/reference
folders before recommending downloads. SDK headers, deployed DLLs and Ghidra tooling are distinct.

The Windows DLSS integration and the observed RenderDoc route did not coexist. AC7's bounded
F9 instrumentation supplies raw colour/depth/motion, constants, shader/binding facts and native
execution packets. Use that working path for producer questions rather than requiring a new
debugger. Steam launch/attachment and capture modules can affect external debugger tests;
record the actual setup instead of claiming an attachment succeeded. F9 usage is in
[the loader guide](../loader/README.md).

## Ghidra MCP and signatures

Use [bethington/ghidra-mcp](https://github.com/bethington/ghidra-mcp) for agent access to Ghidra. Its bridge command matches this repo's `.mcp.json`: `bridge-mcp-ghidra --transport stdio`, with `GHIDRA_MCP_URL=http://127.0.0.1:8089`. Follow that project's installation guide for the extension and Python bridge; neither is installed by ReScaleFrame. Verify the selected program and connection before analysis. Keep local paths in client-local configuration.

The suggested signature library is [threatrack/ghidra-fidb-repo](https://github.com/threatrack/ghidra-fidb-repo), a collection of Ghidra Function ID databases. Attach the relevant `.fidb` through **Tools → Function ID → Attach existing FidDB** and run the appropriate analysis. Select databases for the architecture/compiler/library being investigated, record their revision, and inspect ambiguous matches.

Function signatures identify compiled library functions. They do not supply engine structure layouts or verify AC7 hook addresses. Ghidra `.gdt` files provide types; this repo generates DirectX archives from local headers and can export compiler-derived types. Keep both kinds of generated/downloaded data untracked. [Ghidra's Function ID documentation](https://github.com/NationalSecurityAgency/ghidra/blob/master/Ghidra/Features/FunctionID/src/main/doc/fid.xml) explains creating and sharing databases.

## A Wine prefix with DXVK, for the tests that need it

`eng/wine-test-prefix.sh` builds `.local/wine-test-prefix` from an installed Proton, copying its
DXVK `d3d11`, `dxgi` and `d3d10core` and vkd3d-proton's `d3d12` and `d3d12core` over the prefix's
own and marking them native. Nothing is downloaded and nothing is added to the repository: these
are the same builds the game runs with.

```bash
eng/wine-test-prefix.sh            # once, or after Proton updates
ctest --preset linux-cross-dxvk    # the suite, in that prefix
ctest --preset linux-cross-debug   # the suite, in the default one
```

It exists because the two runtimes disagree about things this project depends on, and the default
prefix is the one that lies. Wine's own D3D11 does not implement shared NT handles, so
`shared_surface` skips there and the presentation bridge looks impossible; under DXVK every stage
passes and the bridge is possible. A fixture that had only run in the default prefix would have
reported the wrong answer about the only environment that matters.

Both presets are worth running. `texture_dump` currently passes under WineD3D and fails under DXVK
with the subprocess killed, reproducibly, which is unexamined and is the kind of difference the
second preset exists to surface.

## Unreal source

Recommend authorized [Unreal Engine GitHub access](https://www.unrealengine.com/en-US/ue-on-github) for Unreal research. The AC7 reference is stock `4.18.3-release`, recorded in [the hook map](research/ue418-hook-map.md). It explains engine behaviour but is not the game's exact source or a guarantee of matching offsets.

The checkout at `references/UnrealEngine` is a full clone whose default branch is 5.8.2. It is kept
checked out at `4.18.3-release` (`0a14a8d537a3`), and every read for this project must be at that
tag. The engine's structures are not stable across that gap: `FSimpleElementVertex` alone gained a
`FDFVector4` position, which moves every offset after it, so a signature taken from the default
branch matches nothing in the game and fails silently. If the shared checkout has moved, read
through `git show 4.18.3-release:<path>` rather than silently changing another task's checkout.

For shared renderer concepts, use [the Unreal 4 skill](../.agents/skills/unreal4-render-integration/SKILL.md).
For version-specific source/private layouts, use [the UE4.18 skill](../.agents/skills/ue418-render-integration/SKILL.md).
For cloud projection, mixed-resolution effects and depth-bound precision, use
[the TrueSky skill](../.agents/skills/truesky-render-integration/SKILL.md). These share one source
under `.agents/skills`; the [analysis skill](../.agents/skills/game-render-analysis/SKILL.md)
routes research and validation. TrueSky reference folders may be source snapshots rather than
Git repositories; record their actual provenance/file hashes instead of inventing revisions.

Keep licensed source and captures outside Git. Commit the method, evidence references, conclusions, uncertainty, and implementation use as required by [the agent instructions](../AGENTS.md).
