# Game plugins

Game directories hold plugin source and `engine.json` evidence summaries. A game may use a shared
engine plugin. Record the exact executable build and the source of each rendering claim.

| Directory | Implementation and documentation |
| --- | --- |
| [ac7](ac7/README.md) | UE4.18/D3D11 native renderer, view decoding, UI rules, resource leases and optional diagnostics |
| [unity-mono](unity-mono/README.md) | Shared Mono/Harmony URP adapter and native D3D12 callback bridge |
| [drag-n-wash](drag-n-wash/README.md) | Recognized Unity build and evidence; uses the shared Unity Mono DLL |
| [project-wingman](project-wingman/README.md) | Researched executable detection and inactive lifecycle scaffold |

[Unity Mono](unity-mono/README.md) supplies Mono/Harmony URP RenderGraph hooks, DX12 SR and
shared runtime FG for the fingerprinted [Drag'n Wash](drag-n-wash/README.md) player, without a
duplicated renderer DLL. Live SR execution and user acceptance of the final FG corrections are
recorded. Other Unity players, HDRP and built-in pipelines still need verified adapters.

[AC7](ac7/README.md) has the accepted native SR/UI/cloud integration and DLSS-G path.
[Project Wingman](project-wingman/README.md) remains a detection/lifecycle scaffold with rendering
unsupported. All native plugin APIs retain `rendering_ready = 0`; that flag is separate from
individual deployed-path acceptance. [Current status](../docs/current-status.md) distinguishes
source features, downloads and remaining validation.

Each plugin README maps its source files and documents preparation, activation and cleanup.
The native lifecycle contract is [game_api.h](../sdk/game/include/rescaleframe/game_api.h);
render packets and CPU observations are [game_renderer.h](../sdk/game/include/rescaleframe/game_renderer.h).
Successful detection or callback installation alone does not establish rendering support.

- `measured`: observed in the game or a capture.
- `inferred`: follows from evidence, with the reasoning recorded.
- `assumed`: an unconfirmed working hypothesis.

Useful fields cover engine version, velocity availability/encoding/coverage, jitter, temporal passes, and the output insertion point. Missing evidence stays unknown. An engine family or menu setting alone does not establish temporal support.

`engine.json` files are evidence records, including dated earlier experiments and corrections.
Their fields name executable identity, observed capabilities, validation limits and research paths.
Preserve unknown (`null`) values and historical limits when adding newer evidence; JSON has no
comment syntax, so explanatory context belongs in named fields and linked research notes.

For engine identification, start with executable strings, then compare view-buffer layouts and package versions. Validate source-derived offsets against the game's data; a custom branch can differ from stock UE4.

Game hooks, structures, and conventions belong here. Shared resources, vendor SDKs, and presentation belong in `runtime/`. Follow [the analysis method](../docs/research/methodology.md) when adding a game.
