# ReScaleFrame

ReScaleFrame adds upscaling and frame generation to games through engine-aware hooks. It is a KillerPixelCrew project, starting with Ace Combat 7's modified UE4.18 renderer.

**DLSS runs in AC7 through the research proxy.** The recorded mission test evaluated 7,917 frames at 1024×576 into a 2048×1152 output. F7 displays that output over the game. Reinsertion into the game's post-processing is still pending, so this debug view lacks the game's grading and HUD.

**The interface reaches the screen at native resolution.** AC7 rasterizes its front end at a fixed 1920×1080 and draws it into the scene as world-space quads at render resolution, which no amount of promoting a target could sharpen. Diverting those draws into a mod-owned layer and compositing at present does sharpen them, and was measured doing so on 7 September 2026. It also produces the wrong picture: the quads read the scene and its glow chain, so compositing at present skips AC7's own interface composite, its glow and its grade. The route is being changed to promote the game's own interface target instead, which keeps all three and yields the same premultiplied layer frame generation will need. See [the extraction note](docs/research/ac7-ui-extraction.md).

**The presentation bridge is possible where the game runs.** Every frame generation SDK this targets is D3D12 and AC7 is D3D11, so the frame has to cross devices. Under a Wine prefix built from Proton's DXVK and vkd3d-proton, a D3D11 texture opens on a D3D12 device and a shared fence signals across, measured rather than assumed. Nothing above that has been built.

The longer-term target is XeSS-SR, XeSS MFG, and XeLL on the MSI Claw 8, with DLSS and FSR using the same runtime. XeSS/MFG, the standalone launcher, WSGM integration, and in-game egui wiring are unfinished.

## Get started

- [Research proxy setup and hotkeys](loader/README.md)
- [Current work and todo](docs/implementation.md)
- [AC7 captures and verified engine data](docs/research/ac7-frame-capture.md)
- [Design](docs/design.md) and [source references](docs/research/source-map.md)
- [Repository review, 7 September 2026](docs/review.md)

## Build

The Windows build uses Visual Studio 2022 C++ tools, a Windows SDK, CMake 3.25+, and Rust 1.94.1, pinned in `rust-toolchain.toml`.

```powershell
./eng/verify.ps1
cargo test --workspace --locked
```

The verification script builds native targets, runs CTest, checks Rust formatting, and runs Clippy. Rust tests need the separate command above. Neither command launches a game. Native Debug output is in `build/windows-x64/bin/Debug`.

Streamline and RenderDoc are optional local dependencies. Without their headers, the corresponding features build as unavailable. See [dependencies](docs/dependencies.md) for paths and licensing references.

## Repository

| Directory | Contents |
| --- | --- |
| `loader` | Research proxy, module capture, and bootstrap scaffold |
| `runtime/orchestrator` | Frame assembly and DLSS pipeline |
| `runtime/contract` | What a reconstruction or frame generator has to be, for all three vendors, and the negotiation that chooses between them |
| `runtime/backends` | Streamline DLSS adapter, FSR and XeSS providers that answer without their SDKs present, and the Rust input model |
| `runtime/presentation` | Surfaces and a fence two devices can share, for the D3D11 to D3D12 bridge |
| `runtime/graphics` | D3D11 observation, resource handling, decode, interface extraction, and UI rendering |
| `games/ac7` | Build recognition and view-uniform reader |
| `sdk/game` | C ABI for game plugins |
| `ui/overlay` | egui settings and status DLL; runtime wiring pending |
| `apps/launcher`, `integrations/wsgm` | Frontend scaffolding and integration plans |
| `tests`, `tools`, `docs` | Tests, analysis utilities, and research |

All first-party components share one repository and release version. Game-specific hooks belong in plugins; vendor SDKs and presentation belong in the runtime. The research proxy still carries AC7-specific glue that needs to move into that structure.

## Agent instructions

Read [AGENTS.md](AGENTS.md) before working here. The repository's [game-render-analysis skill](.agents/skills/game-render-analysis/SKILL.md) uses the open Agent Skills format in `.agents/skills/`. Use it with the [analysis method](docs/research/methodology.md) for rendering research.

**Knowledge should be reusable.** Document every reverse-engineering result end to end: the question, why the approach was chosen, how the result was found, the evidence, and where and when the runtime uses it. Include build fingerprints, source revisions, units, resource lifetimes, failed approaches, and remaining uncertainty where relevant.

Every commit and PR must include its research or link to the relevant evidence in `docs/research/`. For changes that need no new experiment, state the rationale and checks performed. Update `games/<id>/engine.json` and [the todo](docs/implementation.md) when findings or implementation status change. Never invent a measurement to fill a template.

Check the user's environment and recommend the tools needed for the task:

| Work | Tools |
| --- | --- |
| Build on Windows | Git, Visual Studio 2022 C++ tools and Windows SDK, CMake 3.25+, PowerShell, Rust via rustup |
| Binary and buffer analysis | Python 3, Ghidra with PyGhidra, and the JDK required by that Ghidra release |
| Agent access and function signatures | [Ghidra MCP](https://github.com/bethington/ghidra-mcp), [Ghidra Function ID signature library](https://github.com/threatrack/ghidra-fidb-repo) |
| D3D11 frame capture and replay | RenderDoc; a compatible Windows replay environment for the recorded AC7 workflow |
| Linux cross-build | MinGW-w64, Wine, CMake, and the relevant Proton/DXVK setup for game tests |
| Vendor integration | The backend's matching SDK headers and runtime; currently Streamline 2.12.0 for DLSS |

See [tool setup](docs/tooling.md) for download links and requirements. Recommend authorized [Unreal Engine source access](https://www.unrealengine.com/en-US/ue-on-github) when studying an Unreal game; stock source is a reference to validate against the shipped build.

Keep licensed source, binaries, raw captures, machine paths, and credentials out of commits. Publish the findings, reproducible methods, and permitted metadata. Write plainly, preserve uncertainty, and distinguish built, synthetic-tested, capture-validated, game-tested, and target-device-tested results.

## License

First-party code and documentation use [GPL-3.0-only](LICENSE), except the [MIT Game SDK](sdk/game/LICENSE). Third-party dependencies retain their own licenses. Game binaries, Epic source, and vendor runtime DLLs are not included.
