# ReScaleFrame

**A framework for engine-aware upscaling, frame generation and latency integration.**

ReScaleFrame connects game-specific rendering hooks to a shared native runtime. Each game
integration finds the right scene data and puts the processed result back into the engine's frame.
The runtime handles the vendor SDKs, graphics resources, settings and presentation.

The goal is to share that infrastructure across games and GPU vendors while keeping each game's
rendering rules in its own integration. A [KillerPixelCrew](https://github.com/KillerPixelCrew) project.

## Current status

**Ace Combat 7 is the first working integration.** Its DLSS upscaling path is complete and
accepted in game, including full-resolution briefing rendering and the in-game settings overlay.
It currently runs through the AC7 proxy while the shared plugin lifecycle is being built.

| Area | Status |
| --- | --- |
| DLSS Super Resolution / DLAA | Working in AC7 |
| Game detection and native SDK | Versioned C ABI and executable recognition implemented; full rendering lifecycle still in progress |
| Frame generation and latency | Planned; graphics interoperability research and shared-surface tests are in place |
| FSR and XeSS | Backend scaffolding and integration plans; no released game support yet |
| Standalone launcher and WSGM | Scaffolding and planned integration |

For downloads, supported builds, installation and controls, see the relevant game's README:

| Game | Released features | Guide |
| --- | --- | --- |
| Ace Combat 7 | DLSS Super Resolution and Native/DLAA on Windows x64 | [AC7 README](games/ac7/README.md) |

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

These are the framework's ownership boundaries. The current AC7 proxy still contains some glue
that will move into the plugin lifecycle; the working game path and that migration are tracked
separately.

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
PowerShell and the Rust toolchain pinned in `rust-toolchain.toml`.

```powershell
./eng/verify.ps1 -Configuration Release
cargo test --workspace --locked
cargo build --release --locked -p rescaleframe-overlay
```

Use `-VS2026` on the verification command if you have Visual Studio 2026 instead of 2022.
Native outputs are written to `build/windows-x64/bin/Release`; the Rust overlay is in `target/release`.

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
