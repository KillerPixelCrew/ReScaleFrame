# Project Wingman

The Windows x64 plugin recognizes the researched executable, exposes game-plugin ABI 13 and
observes the native temporal graph. Its dedicated XInput carrier is deployed and the desktop
observer is game-tested. GPU resource leases, upscaling, HUD separation, frame generation,
latency and VR processing remain unimplemented.

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

`prepare` checks the running executable's name/hash and the recorded 32-byte temporal-entry
fingerprint before creating a disabled hook. A different process or modified entry returns
`RSF_ERROR_NATIVE_REFUSED`. `start` enables observation; native TAA still runs unchanged.
`quiesce` stops publication and `stop` disables the hook, returning `BUSY` while calls retain
host services. The module and one trampoline are pinned for process lifetime, keeping late
native entries safe after disable. Rendering readiness remains zero.

The dedicated `rsf_proxy_wingman` target produces `wingman/bin/Release/xinput1_3.dll` under the
build directory. It forwards the system DLL's names and ordinals, including Wingman's imports
2/3. Place it beside the shipping executable, and the runtime/plugin DLLs in a `ReScaleFrame`
subdirectory. Add `[ProjectWingman] ObserveNative=1` to `ReScaleFrame.ini`. Its worker uses the
orchestrator's plugin-session lifecycle outside the loader lock and writes
`ReScaleFrame/project-wingman.log`. `ObserveNative=0` prevents startup on the next launch.

Records contain copied view/history keys, jitter, rectangles and opaque RDG references, sampled
once per history owner per second and on rectangle changes. They do not identify stereo eyes,
retain textures or authorize GPU/Present work. The desktop run verified readable native calls;
VR hardware was unavailable. See the research for the recorded dimensions and remaining work.

Build and run the contract checks from the repository root:

```powershell
./eng/verify.ps1 -Configuration Release -VS2026
```

Omit `-VS2026` and use `windows-release` with Visual Studio 2022. The DLL lands in
`build/windows-x64/bin/Release/`. The contract test loads it in a test process, verifies detection,
ABI refusal, inactive status and cleanup, and does not modify or launch the game.

[Engine evidence](engine.json) records capability limits and static layouts.
[Renderer investigation](../../docs/research/project-wingman-renderer.md) records the question,
method, source revision, named functions and remaining runtime experiments. Only the guarded
default temporal-upscaler entry is hooked; the other candidate addresses remain research data.

[Current status](../../docs/current-status.md) compares this scaffold with the implemented AC7
and Unity paths. Desktop observation acceptance does not establish SR, FG, latency or VR support.
