# Game plugins

Each game directory holds its plugin and an `engine.json` evidence summary. Record the exact executable build and the source of each rendering claim.

- `measured`: observed in the game or a capture.
- `inferred`: follows from evidence, with the reasoning recorded.
- `assumed`: an unconfirmed working hypothesis.

Useful fields cover engine version, velocity availability/encoding/coverage, jitter, temporal passes, and the output insertion point. Missing evidence stays unknown. An engine family or menu setting alone does not establish temporal support.

For engine identification, start with executable strings, then compare view-buffer layouts and package versions. Validate source-derived offsets against the game's data; a custom branch can differ from stock UE4.

Game hooks, structures, and conventions belong here. Shared resources, vendor SDKs, and presentation belong in `runtime/`. Follow [the analysis method](../docs/research/methodology.md) when adding a game.
