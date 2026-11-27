# Project Wingman

There is no published Project Wingman package. The observer preview was withdrawn;
SR, FG, latency and VR processing remain unimplemented. The observer build and
research notes below document development work only.

The Windows x64 plugin recognizes the game by executable name, exposes the game-plugin ABI (`sdk/game/include/rescaleframe/game_api.h`) and
observes the native temporal graph. Its dedicated XInput carrier was removed with the withdrawn
preview; the generic `rsf_proxy_plugin` carrier is the one XInput carrier. GPU resource leases,
upscaling, HUD separation, frame generation,
latency and VR processing remain unimplemented.

| Item | Value |
| --- | --- |
| Plugin ID | `project-wingman` |
| CMake target | `rsf_game_project_wingman` |
| Output | `ReScaleFrame.Game.ProjectWingman.dll` |
| Executable | `ProjectWingman-Win64-Shipping.exe` |
| Researched build | Steam `16671873`, SHA256 `092e84225624a4de9c05d2404ff55269bd4a2aa2ff0548f2a183c6bf36abc85a` (logged, never compared) |
| Engine | UE4.27.2, identified from executable metadata and source/binary comparisons |
| Rendering readiness | `0` |

Detection requires the executable basename (ASCII case-insensitive) and an x64 PE machine; any
build is recognized. Recognition establishes identity only: `prepare` discovers whether the
running build is usable.

`prepare` refuses another process, then finds every site in the loaded executable by signature
(`src/discovery.rs`) and logs the executable's SHA-256 and PE timestamp as evidence. A missing or
ambiguous site returns `RSF_ERROR_NATIVE_REFUSED` with a status reason naming the site; the game
keeps running natively. Each hooked entry is matched against its pattern again in live memory
before its disabled hook is created. `start` enables observation; native TAA still runs unchanged.
`quiesce` stops publication and `stop` disables the hook, returning `BUSY` while calls retain
host services. The module and its trampolines are pinned for process lifetime, keeping late
native entries safe after disable. Rendering readiness remains zero.

Construction records contain copied view/history keys, jitter, rectangles and opaque RDG
references. They are sampled once per history owner per second and on rectangle changes.
Execution records acquire four COM texture leases on the native RHI stream and release them
when the diagnostic command drains. The earlier desktop run verified readable construction
calls. The new execution, panel and VR paths still need game validation.

Build and run the contract checks from the repository root:

```powershell
./eng/verify.ps1 -Configuration Release -VS2026
```

Omit `-VS2026` and use `windows-release` with Visual Studio 2022. The DLL lands in
`build/windows-x64/bin/Release/`. The contract test loads it in a test process, verifies detection,
ABI refusal, inactive status and cleanup, and does not modify or launch the game.

## Site discovery

Every pattern must match exactly once in the executable sections; rel32 branches and RIP
displacements are wildcards. RVAs are those of build 16671873, recorded for the test.

| Site | How it is found | Build 16671873 |
| --- | --- | --- |
| `FDefaultTemporalUpscaler::AddPasses` (hooked) | Entry pattern | `0x5c442b0` |
| `FMemStackBase::FreeChunks` (hooked) | Entry pattern | `0x53821d0` |
| `AddTemporalAAPass` (hooked) | Entry pattern | `0x5c44910` |
| TAA compute pass helper (hooked) | Its call in `AddTemporalAAPass` (the entry is shared by five sibling template instances), entry bytes checked | `0x5c2d4a0` |
| RDG pass setup (hooked) | Entry pattern | `0x6569660` |
| Tracked allocation vtable | `lea` in the helper's first 0x180 bytes; fixes the 0x120-byte allocation from graph + 0x10 | `0x8390f78` |
| Compute pass vtable | `lea` in the helper's first 0x180 bytes; fixes the pass at owner + 8 | `0x83f6838` |
| Tracked allocation destructor | Slot 0 of its vtable, entry bytes checked | `0x591d000` |
| Compute pass execute (hooked) | Slot 1 of its vtable (an identical twin follows it), entry bytes checked; fixes parameters at pass + 0xf0 | `0x5c4c9c0` |
| View camera cut `+0xd90` | Displacement of the cut test in `AddTemporalAAPass` | `0x5c449d8` |
| View jitter `+0x4fa4` | Displacement of the jitter read in `AddTemporalAAPass` | `0x5c44ecc` |
| View state `+0x1680` | Displacement of the read before the output-history add in the Gen4 helper | `0x5c4120b` |
| View rectangle `+0x1670` | Displacement of the Gen4 helper's entry read | `0x5c41065` |

The camera matrices, family frame fields, RDG texture and UAV fields and the RHI command list
remain documented constants of build 16671873: no discovered code reveals them. The camera decode
refuses a view whose matrices are not an inverse pair with a valid projection, and `bind_pass`
admits a pass only after its own and its owner's vtables match the discovered ones.

The signatures were proven on build 16671873 recovered byte-exact from the user's Ghidra project
after the game was uninstalled. `cargo test -p rsf-project-wingman` checks them against the
installed executable, or the copy `RSF_PROJECT_WINGMAN_EXE` names, and skips without one.
Discovery in a running game has not been exercised since the conversion.

[Engine evidence](engine.json) records capability limits and static layouts.
[Renderer investigation](../../docs/research/project-wingman-renderer.md) records the question,
method, source revision, named functions and remaining runtime experiments. The other candidate
addresses there remain research data.

[Current status](../../docs/current-status.md) compares this scaffold with the implemented AC7
and Unity paths. Desktop observation acceptance does not establish SR, FG, latency or VR support.