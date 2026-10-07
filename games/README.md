# Game plugins

Each game directory holds its plugin and an `engine.json` evidence summary. Record the exact executable build and the source of each rendering claim.

[Unity Mono](unity-mono/README.md) supplies Mono/Harmony URP RenderGraph hooks, DX12 SR and
shared runtime FG for the fingerprinted [Drag'n Wash](drag-n-wash/README.md) player, without a
duplicated renderer DLL. Live SR execution and user acceptance of the final FG corrections are
recorded. Other Unity players, HDRP and built-in pipelines still need verified adapters.

[AC7](ac7/README.md) has the accepted native SR/UI/cloud integration and DLSS-G path.
[Project Wingman](project-wingman/README.md) remains a detection/lifecycle scaffold with rendering
unsupported. All native plugin APIs retain `rendering_ready = 0`; that flag is separate from
individual deployed-path acceptance. [Current status](../docs/current-status.md) distinguishes
source features, downloads and remaining validation.

- `measured`: observed in the game or a capture.
- `inferred`: follows from evidence, with the reasoning recorded.
- `assumed`: an unconfirmed working hypothesis.

Useful fields cover engine version, velocity availability/encoding/coverage, jitter, temporal passes, and the output insertion point. Missing evidence stays unknown. An engine family or menu setting alone does not establish temporal support.

For engine identification, start with executable strings, then compare view-buffer layouts and package versions. Validate source-derived offsets against the game's data; a custom branch can differ from stock UE4.

Game hooks, structures, and conventions belong here. Shared resources, vendor SDKs, and presentation belong in `runtime/`. Follow [the analysis method](../docs/research/methodology.md) when adding a game.
