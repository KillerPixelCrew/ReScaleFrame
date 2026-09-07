# Development and research tools

Check what is already installed and recommend only the tools needed for the current task. Building the runtime, examining a binary, and replaying a frame need different environments.

## Build

The repository pins Rust in `rust-toolchain.toml` and selects Visual Studio 2022 in `CMakePresets.json`.

| Tool | Purpose |
| --- | --- |
| [Git](https://git-scm.com/downloads) | Repository and reference revisions |
| [Visual Studio / C++ Build Tools](https://visualstudio.microsoft.com/downloads/) | VS 2022 C++ tools and Windows SDK for the reference x64 build |
| [CMake](https://cmake.org/download/) | 3.25+ for the checked-in presets |
| [Rust via rustup](https://rustup.rs/) | Pinned Rust toolchain and egui tests |
| [PowerShell](https://github.com/PowerShell/PowerShell) | `eng/verify.ps1` |
| [Python 3](https://www.python.org/downloads/) | PE, capture, and view-buffer tools |

Run `./eng/verify.ps1` and `cargo test --workspace --locked`. The script currently runs Clippy but does not execute Rust tests. Linux cross-builds additionally use MinGW-w64 and Wine; see [AGENTS.md](../AGENTS.md).

## Binary and frame analysis

Use [Ghidra](https://github.com/NationalSecurityAgency/ghidra) with PyGhidra and the JDK required by the installed release. The repository's scripts target Ghidra 12+. [The Ghidra tooling guide](../tools/ghidra/README.md) covers DirectX types and compiler-derived archives.

Use [RenderDoc](https://github.com/baldurk/renderdoc) for capture and replay. The recorded AC7 workflow used 1.45: the Windows DLL captured through DXVK under Proton, Windows `renderdoccmd` converted XML under Wine, and Windows replay supplied pixels. Keep this evidence separate from assumptions about other versions/platforms.

For DLSS development, obtain the matching [Streamline release](https://github.com/NVIDIA-RTX/Streamline/releases/tag/v2.12.0) and follow [local dependency paths](dependencies.md). SDK headers and runtime DLLs are separate from Ghidra tooling.

## Ghidra MCP and signatures

Use [bethington/ghidra-mcp](https://github.com/bethington/ghidra-mcp) for agent access to Ghidra. Its bridge command matches this repo's `.mcp.json`: `bridge-mcp-ghidra --transport stdio`, with `GHIDRA_MCP_URL=http://127.0.0.1:8089`. Follow that project's installation guide for the extension and Python bridge; neither is installed by ReScaleFrame. Verify the selected program and connection before analysis. Keep local paths in client-local configuration.

The suggested signature library is [threatrack/ghidra-fidb-repo](https://github.com/threatrack/ghidra-fidb-repo), a collection of Ghidra Function ID databases. Attach the relevant `.fidb` through **Tools → Function ID → Attach existing FidDB** and run the appropriate analysis. Select databases for the architecture/compiler/library being investigated, record their revision, and inspect ambiguous matches.

Function signatures identify compiled library functions. They do not supply engine structure layouts or verify AC7 hook addresses. Ghidra `.gdt` files provide types; this repo generates DirectX archives from local headers and can export compiler-derived types. Keep both kinds of generated/downloaded data untracked. [Ghidra's Function ID documentation](https://github.com/NationalSecurityAgency/ghidra/blob/master/Ghidra/Features/FunctionID/src/main/doc/fid.xml) explains creating and sharing databases.

## Unreal source

Recommend authorized [Unreal Engine GitHub access](https://www.unrealengine.com/en-US/ue-on-github) for Unreal research. The AC7 reference is stock `4.18.3-release`, recorded in [the hook map](research/ue418-hook-map.md). It explains engine behaviour but is not the game's exact source or a guarantee of matching offsets.

Keep licensed source and captures outside Git. Commit the method, evidence references, conclusions, uncertainty, and implementation use as required by [the agent instructions](../AGENTS.md).
