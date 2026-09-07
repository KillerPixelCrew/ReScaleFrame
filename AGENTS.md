# Working on ReScaleFrame

Guidance for coding agents working in this repository. `CLAUDE.md` is a symlink to this file; keep
instructions here rather than in tool-specific files.

Use natural, concise language in documentation, code comments, issues, commits, and PRs. Avoid em dashes and filler.

## Research and tool setup

Repository skills use the open [Agent Skills format](https://agentskills.io/specification) under `.agents/skills/<name>/SKILL.md`. Keep one shared skill source with standard YAML metadata. Clients with different discovery paths can load the linked file explicitly; do not fork its instructions per agent.

Every reverse-engineering result must explain the question, why the approach was chosen, how it was found, the evidence, and where and when the runtime uses it. Record relevant fingerprints, source revisions, units, frame/view identity, resource lifetime, failed approaches, and uncertainty. Keep `games/<id>/engine.json`, research notes, and the implementation tracker consistent.

Every commit and PR includes its research or links to the relevant notes. For changes without a new experiment, give the rationale and checks performed. Never invent research or describe a source review as a runtime test. When evidence corrects an earlier conclusion, retain a concise account of what changed and why.

Check the environment before recommending installation. [docs/tooling.md](docs/tooling.md) lists build tools, Ghidra/PyGhidra, Ghidra MCP, Function ID signature databases, RenderDoc, vendor SDKs, and authorized Unreal source access. Use the local game-render-analysis skill or methodology for rendering questions. If older instructions conflict with current source or measured evidence, document the correction rather than repeating the old claim.

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
- Preserve unrelated work. Deploying the proxy into a game installation, injecting it, and loading
  modding frameworks to inspect a running game are normal work here and are how anything gets
  tested or found out. Do them when asked. What must not happen is routine build verification
  reaching into a game install, or a change to one that nobody asked for. Back up what you replace.
- Track implementation in `docs/implementation.md`. Distinguish built, synthetic-tested, game-tested, and device-tested results.
- Run `eng/verify.ps1` for native/SDK/Rust changes. Use focused checks for documentation-only edits.
- Keep component versions aligned through `VERSION` and the Cargo workspace version; verification checks the match.
- Keep downloaded reference checkouts and licensed engine/game source outside Git. Never copy reference code merely because it was useful to study.

## Build and verify

`eng/verify.ps1` is the native/lint gate and matches CI. It compares `VERSION` against the Cargo workspace
version, configures, builds, and tests the native tree, then runs `cargo fmt --check` and
`cargo clippy -D warnings`. Run `cargo test --workspace --locked` separately when Rust behaviour is affected; the script does not execute those tests.

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

## Changing engine behaviour

Patch the code. That is the default here, not the fallback.

When the engine does something that has to stop, find the instructions in Ghidra against the
matching Unreal source and replace them. Console variables, configuration files and engine settings
are all things the game can overwrite, arrive at by another route, or ignore, and every one of them
has cost a run of the game to find that out. The jitter gate, the velocity blend-mode gate and the
separate-translucency halving were all settled this way after other approaches were not.

The rule that makes it safe is the expected bytes. Every patch names what it expects to find and
writes only where it finds it, so a game update moves the code and the patch refuses instead of
corrupting an instruction. It is announced either way, because a patch that silently did nothing is
indistinguishable from one that worked until something downstream fails.

Patch after decryption, next to the others in `apply_*_patch`, and record the site in
`docs/research/ue418-hook-map.md` with its RVA, its bytes and the source it corresponds to. Name the
function in the Ghidra project while the evidence is in front of you.

## Native ABI

`sdk/game/include/rescaleframe/game_api.h` is the versioned contract between orchestrator and
plugins. It is MIT while the rest of the repository is GPL-3.0-only, so keep it self-contained and
free of GPL includes.

Mechanics that the header and `tests/plugin_contract.cpp` jointly enforce:

- Size-checked call structures lead with `struct_size`; embedded metadata is part of its enclosing ABI. Entry points reject a short call structure with
  `RSF_ERROR_INVALID_ARGUMENT` and a mismatched `abi_version` with `RSF_ERROR_ABI_MISMATCH`.
- Extend structs by appending fields and bumping `RSF_GAME_ABI_VERSION`, never by reordering.
- Plugin-returned strings are immutable, plugin-owned, and valid until the DLL unloads. Probe strings
  are borrowed for the duration of the call.
- Frame callbacks are deliberately absent for now (their shape, ABI 2, is specified in
  `docs/representation-plan.md` and lands with its M5). They land after the renderer experiments settle
  their shape.

The honesty rule is testable here: `rsf_game_info.rendering_ready` stays `0` and `status` stays
truthful until rendering works through the plugin lifecycle, and the contract test asserts it. The same applies to
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
skill at `.agents/skills/game-render-analysis/`. Read one of them before starting on a new game or
a new render question; both record failures worth not repeating.

`tools/inspect-game.py` is read-only PE inspection; a string match is a lead, not a verified hook
site. `.mcp.json` wires up a Ghidra MCP bridge for binary analysis. It is a development tool, not a
runtime dependency.
