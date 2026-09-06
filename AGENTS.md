# Working on ReScaleFrame

Guidance for coding agents working in this repository. `CLAUDE.md` is a symlink to this file; keep
instructions here rather than in tool-specific files.

Use natural, concise language in documentation, code comments, issues, commits, and PRs. Avoid em dashes and filler.

## Project boundaries

- Keep all first-party code in this monorepo. Do not split the loader, orchestrator, SDK, UI, or game plugins into separate repositories or submodules.
- The orchestrator owns plugin lifecycle, vendor SDKs, graphics interoperability, settings, and presentation. A game plugin owns its game detection, hooks, renderer preparation, input conventions, and output reinsertion.
- Loading the orchestrator is distinct from activating a graphics pipeline. It selects/prepares the plugin first.
- Keep public native interfaces as C ABI contracts. Do not pass STL containers, Rust types, ownership-ambiguous allocations, or exceptions across DLL boundaries.
- A recognized executable fingerprint is not proof of working hooks or rendering support. Report capability and validation status honestly.
- Keep standalone use independent of WSGM. The initial complete AC7 goal includes SR, MFG, and latency integration.

## Delivery

- While the project is still scaffolding, commit to `main` directly. Rapid iteration matters more
  than review ceremony at this stage. Move to branches and pull requests once there is a working
  pipeline worth protecting.
- Preserve unrelated work. Do not edit game installations or run injection as part of routine build verification.
- Track implementation in `docs/implementation.md`. Distinguish built, synthetic-tested, game-tested, and device-tested results.
- Run `eng/verify.ps1` for native/SDK/Rust changes. Use focused checks for documentation-only edits.
- Keep component versions aligned through `VERSION` and the Cargo workspace version; verification checks the match.
- Keep downloaded reference checkouts and licensed engine/game source outside Git. Never copy reference code merely because it was useful to study.

## Build and verify

`eng/verify.ps1` is the full gate and matches CI. It compares `VERSION` against the Cargo workspace
version, configures, builds, and tests the native tree, then runs `cargo fmt --check` and
`cargo clippy -D warnings`.

```powershell
./eng/verify.ps1                        # Debug
./eng/verify.ps1 -Configuration Release # what .github/workflows/verify.yml runs
```

Native only:

```powershell
cmake --preset windows-x64          # build/windows-x64, BUILD_TESTING=ON
cmake --build --preset windows-debug
ctest --preset windows-debug
ctest --preset windows-debug -R game_plugin_contract   # a single test
```

Binaries land in `build/windows-x64/bin/<Config>/`.

### Building off Windows

`eng/verify.ps1` needs PowerShell, and MSVC on Windows is the reference build and the only thing CI
runs. A Linux checkout can still build and test the native tree by cross-compiling:

```bash
cmake --preset linux-cross-x64          # mingw-w64, build/linux-cross-x64
cmake --build --preset linux-cross-debug
ctest --preset linux-cross-debug        # runs the test executables under Wine
```

This needs `mingw-w64-gcc` and `wine`. The Windows-only guard in `CMakeLists.txt` passes because a
mingw toolchain sets `WIN32`; the MSVC-specific warning flags are guarded and the cross build
enforces `-Wall -Wextra -Werror` instead.

A mingw-and-Wine run is a real build and a real test run, but it is neither MSVC nor Windows.
Report it as "cross-built and tested under Wine", never as verified on Windows. If no `rustup` is
present the `rust-toolchain.toml` pin is not applied and the system toolchain is used instead, so
formatting and lint results can differ from CI. Do not report a native change as building on
Windows unless it was actually built there.

## Architecture

Three artifacts load into the game process, plus one out-of-process frontend:

| Target | Output name | Role |
| --- | --- | --- |
| `rsf_bootstrap` | `ReScaleFrame.Bootstrap.dll` | injected entry point, loads the orchestrator |
| `rsf_orchestrator` | `ReScaleFrame.Runtime.dll` | vendor SDKs, plugin lifecycle, GPU resources, presentation |
| `rsf_game_ac7` | `ReScaleFrame.Game.AC7.dll` | game plugin: detection, hooks, engine data |
| `rsf_launcher` | `ReScaleFrame.exe` | standalone launcher and profile frontend |
| `rsf_game_sdk` | `ReScaleFrame::GameSDK` (INTERFACE) | the C header both sides compile against |

Working consequences of the ownership split above:

- The plugin knows where and how to hook the game. The orchestrator knows how to process the data.
  Game-specific signatures, structures, velocity encoding, camera resets, and HUD boundaries belong
  in `games/<id>`. Vendor contexts (XeSS, DLSS, FSR), swap-chain ownership, and presentation belong
  in `runtime/`. Do not move logic across that line for convenience.
- A game plugin never loads a second runtime. The orchestrator establishes configuration and shared
  services first, prepares the plugin, and only then touches graphics.
- SR input and FG input are different points in the render pipeline: pre-tonemap scene colour for
  super resolution, the completed HUD-less image for frame generation. They are not interchangeable.
- Frontends (standalone launcher, WSGM) exchange configuration and bounded status over IPC. They
  never receive GPU textures and never run a per-frame graphics loop.

`docs/design.md` describes the intended runtime. `docs/research/architecture.md` separates inspected
behavior from proposed implementation, and `docs/research/ue418-hook-map.md` holds the AC7 and
UE4.18 hook research.

## Native ABI

`sdk/game/include/rescaleframe/game_api.h` is the versioned contract between orchestrator and
plugins. It is MIT while the rest of the repository is GPL-3.0-only, so keep it self-contained and
free of GPL includes.

Mechanics that the header and `tests/plugin_contract.cpp` jointly enforce:

- Every struct leads with `struct_size`. Entry points reject a short struct with
  `RSF_ERROR_INVALID_ARGUMENT` and a mismatched `abi_version` with `RSF_ERROR_ABI_MISMATCH`.
- Extend structs by appending fields and bumping `RSF_GAME_ABI_VERSION`, never by reordering.
- Plugin-returned strings are immutable, plugin-owned, and valid until the DLL unloads. Probe strings
  are borrowed for the duration of the call.
- Frame callbacks are deliberately absent for now. They land after the renderer experiments settle
  their shape.

The honesty rule is testable here: `rsf_game_info.rendering_ready` stays `0` and `status` stays
truthful until the pipeline actually works, and the contract test asserts it. The same applies to
prose. Built is not injected, recognized is not supported, and a higher presentation counter is not
lower latency. Do not soften README or status wording ahead of the code.

## Conventions

- Version is single-sourced: `VERSION` feeds CMake `PROJECT_VERSION`, which generates
  `rescaleframe/version.h` (`RSF_VERSION_STRING`). A bump edits `VERSION` and the Cargo workspace
  `version` together.
- Warnings are errors everywhere. MSVC builds with `/W4 /WX`; the Rust workspace denies
  `unsafe_op_in_unsafe_fn` and Clippy warnings.
- `.editorconfig`: LF, UTF-8, 4 spaces, 2 for json, yml, and toml.

## Out-of-tree material

Game binaries, Epic engine source, vendor SDKs, reference checkouts, captures, and Ghidra projects
stay untracked. `.local/`, `references/`, `vendor/`, `captures/`, and `dumps/` are gitignored. Only
the executable name and hash appear in published research. See `docs/dependencies.md` before adding
any third-party code or binary.

The procedure this project uses to analyse a game is written down in
[docs/research/methodology.md](docs/research/methodology.md), with the operational version as a
skill at `.claude/skills/game-render-analysis/`. Read one of them before starting on a new game or
a new render question; both record failures worth not repeating.

`tools/inspect-game.py` is read-only PE inspection; a string match is a lead, not a verified hook
site. `.mcp.json` wires up a Ghidra MCP bridge for binary analysis. It is a development tool, not a
runtime dependency.
