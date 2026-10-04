# Project Wingman

The plugin is a Windows x64 scaffold. It recognizes the researched executable and exposes the
shared game-plugin ABI. Renderer hooks, upscaling, HUD separation, frame generation and VR
integration are not implemented or game-tested.

| Item | Value |
| --- | --- |
| Plugin ID | `project-wingman` |
| CMake target | `rsf_game_project_wingman` |
| Output | `ReScaleFrame.Game.ProjectWingman.dll` |
| Executable | `ProjectWingman-Win64-Shipping.exe` |
| SHA256 | `092e84225624a4de9c05d2404ff55269bd4a2aa2ff0548f2a183c6bf36abc85a` |
| Engine | UE4.27.2, identified from executable metadata and source/binary comparisons |
| Rendering readiness | `0` |

Detection requires the executable basename, x64 PE machine and exact hash, with ASCII case
insensitivity for the name and hex digest. A different build is unknown. Recognition establishes
identity only.

`prepare` validates the call, host ABI and nonzero session identity, then returns
`RSF_ERROR_NOT_READY`. `start` also refuses. Neither installs hooks nor retains host services.
`quiesce` and `stop` validate their calls and succeed because there are no owned resources.
Renderer status always reports unprepared, inactive and not ready, with a reason string.

The shared runtime can load this DLL through its existing explicit plugin-path API. Automatic
game selection and a Project Wingman loading/deployment route are still pending. The AC7 research
proxy is specific to AC7 and is not a Project Wingman loader.

Build and run the contract checks from the repository root:

```powershell
./eng/verify.ps1 -Configuration Release -VS2026
ctest --preset windows-release-vs18 -R 'game_.*plugin_contract'
```

Omit `-VS2026` and use `windows-release` with Visual Studio 2022. The DLL lands in
`build/windows-x64/bin/Release/`. The contract test loads it in a test process, verifies detection,
ABI refusal, inactive status and cleanup, and does not modify or launch the game.

[Engine evidence](engine.json) records capability limits and static layouts.
[Renderer investigation](../../docs/research/project-wingman-renderer.md) records the question,
method, source revision, named functions and remaining runtime experiments. Candidate addresses
remain research data rather than executable hook code.
