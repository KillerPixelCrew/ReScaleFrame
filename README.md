# ReScaleFrame

**A framework for engine-aware upscaling, frame generation and latency integration.**

ReScaleFrame connects game-specific rendering hooks to a shared native runtime. Each game
integration finds the right scene data and puts the processed result back into the engine's frame.
The runtime handles the vendor SDKs, graphics resources, settings and presentation.

The goal is to share that infrastructure across games and GPU vendors while keeping each game's
rendering rules in its own integration. A [KillerPixelCrew](https://github.com/KillerPixelCrew) project.

## Current status

**Ace Combat 7 is the first working integration.** Its engine-owned upscaling path and corrected
wing/cloud rendering are accepted in game, with full-resolution UI and the in-game settings overlay.
The AC7 proxy loads the game plugin and shared runtime services.

| Area | Status |
| --- | --- |
| DLSS Super Resolution / DLAA | Working in AC7 |
| Game detection and native SDK | Game ABI 13 with lifecycle, CPU events and renderer callbacks; AC7 and Unity Mono paths implemented; Project Wingman remains a scaffold |
| Frame generation and latency | Current source implements DLSS-G, FSR3/4 FG, XeSS FG, Reflex/XeLL and live provider switching; AC7 DLSS-G and Unity corrections have user acceptance; broader hardware/scene and measured latency validation remain open |
| FSR and XeSS | FSR 2/3/4 and XeSS SR available in AC7; optimized D3D11/D3D12 transfer; FSR 4 INT8 compatibility device-tested on RTX 4070 |
| Standalone launcher and WSGM | Scaffolding and planned integration |

The published v0.1.0 AC7 ZIP provides SR and the overlay; it does not contain the later FG/Reflex
work on `main`. Unity currently has local test packages, not a published release.
See [current implementation and validation](docs/current-status.md) for evidence and remaining limits.

For downloads, supported builds, installation and controls, see the relevant game's README:

| Game | Availability | Guide |
| --- | --- | --- |
| Ace Combat 7 | DLSS/DLAA, FSR 2/3/4, XeSS SR and overlay on Windows x64 | [AC7 README](games/ac7/README.md) |
| Project Wingman | Scaffold and static UE4.27.2 research; rendering unsupported | [Project Wingman README](games/project-wingman/README.md) |
| Drag'n Wash | Unity Mono DX12 SR, shared overlay and runtime FG switching in source/local test builds; Claw acceptance pending | [Drag'n Wash README](games/drag-n-wash/README.md) |

[Releases](https://github.com/KillerPixelCrew/ReScaleFrame/releases)
· [Implementation tracker](docs/implementation.md)
· [Issues](https://github.com/KillerPixelCrew/ReScaleFrame/issues)

## How the framework is divided

A game integration owns engine knowledge: executable fingerprints, hooks, camera data, motion-vector
conventions, render resolution and composition boundaries. Vendor SDK code and presentation belong
to the shared runtime. Loading that runtime is separate from activating a rendering pipeline.

| Component | Responsibility |
| --- | --- |
| Bootstrap | Load the runtime into the game process |
| Orchestrator | Select and prepare the game integration; own settings, vendor SDKs and shared graphics services |
| Game integration | Detect the game, prepare the renderer, collect frame data and reinsert the result |
| Game SDK | Keep the plugin/runtime boundary versioned and explicit through a C ABI |
| Frontends | Configure sessions and show status through bounded IPC |

The AC7 DirectInput carrier prepares the game plugin and links shared runtime objects into its
proxy. The Unity version shim loads the runtime DLL and shared Mono plugin. Bootstrap and launcher
remain scaffolds; frontend IPC is intended architecture rather than an implemented control route.

Super resolution consumes scene colour before tonemapping. Frame generation needs a completed
HUD-less image later in the frame. ReScaleFrame treats those as separate inputs and keeps frame/view
identity, resource lifetime, colour space, motion and jitter conventions with the data. UI and
translucency can keep their own resolution and composition paths.

The standalone frontend is intended to work independently of WSGM. Neither frontend receives GPU
textures or runs the game's graphics loop.

See the [design](docs/design.md), [architecture research](docs/research/architecture.md) and
[vendor contracts](docs/research/vendor-fg-contracts.md) for the detailed boundaries and remaining work.

## Repository

All first-party components live in this monorepo and share one release version.

| Directory | Contents |
| --- | --- |
| [`loader/`](loader/) | Bootstrap, AC7 proxy and loading diagnostics |
| [`runtime/orchestrator/`](runtime/orchestrator/) | Shared lifecycle and frame-processing coordination |
| [`runtime/backends/`](runtime/backends/) | Vendor adapters and upscaling contracts |
| [`runtime/graphics/`](runtime/graphics/) | Resource handling, observation, state restoration and composition |
| [`runtime/presentation/`](runtime/presentation/) | Shared surfaces, synchronization and presentation work |
| [`games/`](games/) | Game-specific integrations and their evidence |
| [`sdk/game/`](sdk/game/) | Public native C ABI |
| [`ui/overlay/`](ui/overlay/) | In-game settings overlay |
| [`apps/launcher/`](apps/launcher/), [`integrations/wsgm/`](integrations/wsgm/) | Frontend work |
| [`tests/`](tests/), [`tools/`](tools/), [`docs/`](docs/) | Verification, analysis tools and documentation |

## Build and verify

Windows/MSVC is the reference platform. You need Visual Studio C++ tools, a Windows SDK, CMake,
PowerShell and the Rust toolchain pinned in `rust-toolchain.toml`. Unity managed builds additionally
need .NET, Unity native headers and the researched player's Managed assemblies; see
[the Unity guide](games/unity-mono/README.md).

```powershell
./eng/verify.ps1 -Configuration Release
cargo test --workspace --locked
cargo build --release --locked -p rescaleframe-overlay
```

Use `-VS2026` on the verification command if you have Visual Studio 2026 instead of 2022.
Shared native outputs are written to `build/windows-x64/bin/Release`; the dedicated AC7 carrier is
`build/windows-x64/ac7/bin/Release/dinput8.dll`. The Rust overlay is in `target/release`.

The verification gate builds and tests the native tree, checks Rust formatting and runs Clippy.
Rust tests use the separate command above. Verification never launches a game or changes an
installation.

Vendor SDKs are optional local dependencies. A backend built without its SDK reports that it is
unavailable. See [dependencies and packaging](docs/dependencies.md) and [tool setup](docs/tooling.md)
for the expected versions and paths. A MinGW/Wine development route is also documented in
[AGENTS.md](AGENTS.md); it does not replace Windows verification.

## Development approach

Rendering support is established from engine behaviour and captures. A matching executable hash
only identifies a build; it does not prove that its hooks or rendering path work.

Research notes record the question, method, evidence, failed approaches and remaining uncertainty.
Tests and status distinguish built, synthetic-tested, game-tested and device-tested results. Engine
patches check their expected bytes before writing and report when they cannot apply.

Start with [AGENTS.md](AGENTS.md), the [render-analysis method](docs/research/methodology.md) and
the [implementation tracker](docs/implementation.md). Game-specific findings belong with the
integration and its research notes. Licensed engine/game source, vendor binaries and raw captures
stay outside Git.

## License

First-party code and documentation are [GPL-3.0-only](LICENSE), except the
[MIT-licensed Game SDK](sdk/game/LICENSE).

Third-party libraries and vendor runtimes retain their own licenses. Game release packages include
the applicable notices; bundled NVIDIA runtime files are unmodified and are not relicensed under
GPL. Game binaries and licensed engine source are not distributed with ReScaleFrame.
