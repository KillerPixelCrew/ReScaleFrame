# Documentation audit, 7 October 2026

## Question, method and scope

Do the repository's user/developer guides accurately describe ReScaleFrame and its plugins?
Checked the current source at `fda5fd0f2730b40e5b96f419bc0a05cb0b73932b`, recorded research,
build/deployment/packaging scripts and the GitHub release listing. Source and dated acceptance
records were used because a component version, successful build or detection hash alone cannot
establish a shipped feature or working game path. The audit is documentation/source review,
not a fresh reverse-engineering experiment, Windows build or game/device test.

[Current implementation and validation](current-status.md) is the maintained feature/acceptance
summary. Historical research retains its original results, addresses, source revisions and hashes.
No historical device or game acceptance was invented or extended to new hardware.

## Corrections and evidence

| Area | Correction | Evidence checked |
| --- | --- | --- |
| SDK and lifecycle | Replace metadata-only/planned ABI 2 claims with game ABI 13, renderer/CPU callbacks and leased resources; retain readiness zero | `game_api.h`, `game_renderer.h`, `game_frame.h`, three plugin implementations and `plugin_session.cpp` |
| AC7 source versus release | Published v0.1.0 is SR-only; current source includes FG/latency and live provider replacement; preserve new AC7 vendor FG gaps | GitHub release metadata; SR/FG runtime, AC7/shared FG research and release notes |
| Unity/Drag'n Wash | Remove unimplemented Mono/Harmony/SR/FG claims; explain pre-DoF SR, pre-UI guide, Off/FSR1 inputs, final user acceptance and remaining Claw/MFG coverage | Managed/native plugin, `unity_sr_host.cpp`, Unity SR/shared FG notes and scripts |
| Project Wingman | Keep detection/ABI scaffold distinct from unsupported rendering/VR | Plugin source, contract fixture and engine evidence |
| Overlay | Replace F5/ABI 3 with Insert/ABI 9; document separate SR/FG intents, queried counts and measured FPS counter scope | Overlay header/Rust panel, native host and Unity host |
| Ownership and design | Replace proposed UI diversion as the default with native AC7 UI owners; distinguish carriers/runtime from version-only bootstrap and launcher | Native renderer, loader CMake/shim, orchestrator CMake and launcher source |
| Dependencies and packaging | Clarify helper netstandard2.1 versus Harmony net472, FG payloads, local Unity ZIP defaults and AC7 packager/deployer carrier mismatch | Managed csproj, build/deployment scripts, both packagers and notices |
| Research and plans | Scope dated pending/failed claims; point to later corrections and current headers; reconcile implemented Unity tracker steps | Research/tracker chronology, current native/managed source and repository skills |

The checked release listing has one published release, v0.1.0, with the AC7 Windows ZIP,
corresponding-source ZIP and SHA256SUMS. Its replacement assets were uploaded on 2 October 2026.
Its SR-only release description remains accurate. No Unity/Project Wingman release or GitHub Pages
site was listed. The enabled wiki flag does not establish a wiki: its Git endpoint returned
Repository not found, so no wiki contents were audited.

## Inventory

The audit covers repository Markdown, game evidence summaries, the INI sample's documentation
and the Unity packager's generated README. `CLAUDE.md` remains a symlink to `AGENTS.md`.

| Documentation set | Treatment |
| --- | --- |
| Root README, AGENTS and game READMEs | Current user/build/ownership claims reconciled with source and acceptance |
| SDK, runtime, graphics, overlay and loader guides | Current contracts/routes/controls corrected; compatibility mechanisms clearly scoped |
| Design, dependencies, tooling and tracker | Current architecture and script behavior reconciled; dated test runs preserved |
| Release guides/notes | Explicitly scoped to the published corrected SR ZIP; FG availability is described separately |
| Research records and plans | Dated experiment/design scope explicit; later acceptance/current status linked; original evidence retained |
| Methodology, source map, validation targets, WSGM and tool guides | Reviewed for scope/link consistency; references and planned targets retained |
| Local repository skills/references | Stale FG status corrected or scoped to recorded SR lessons; technical patch anchors preserved |
| Game `engine.json` files | Separate current validation summaries added; AC7 fingerprint linked to recorded executable evidence; Unity live API/status clarified |
| Historical evidence JSON | Parsed for validity; original snapshots preserved |
| Generated Unity package README and AC7 INI sample | Prose/comments corrected without changing script behavior or configuration defaults |

## Checks and remaining boundaries

Focused checks cover local Markdown targets/anchors, JSON validity, all three executable fingerprints
against plugin source, current documented ABI versions, and `VERSION`/Cargo agreement. The Unity
packager diff is restricted to its README here-string, and active INI assignments are unchanged.
Whitespace validation uses `git diff --check`.

Result: 79 Markdown files and 504 local targets/anchors checked without failures; all 19 tracked
JSON files parse. Fingerprints, ABI/version checks, unchanged packaging behavior, unchanged active
INI defaults, documentation-only scope and the agent-instruction symlink checks pass.

The workspace does not have PowerShell for a parser/native gate. No native, SDK or Rust behavior
changed, so no new native/game/device result is claimed. Existing MSVC, GPU, Rust and shipped-Mono
results remain attributed to their dated notes. Licensed game/engine source, captures and private
Ghidra hook addresses were not independently reproduced by this audit. Open lighting, motion,
new-provider acceptance, higher MFG, Claw and measured latency questions remain explicit.
