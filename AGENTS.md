# Working on ReScaleFrame

Use natural, concise language in documentation, code comments, issues, commits, and PRs. Avoid em dashes and filler.

## Project boundaries

- Keep all first-party code in this monorepo. Do not split the loader, orchestrator, SDK, UI, or game plugins into separate repositories or submodules.
- The orchestrator owns plugin lifecycle, vendor SDKs, graphics interoperability, settings, and presentation. A game plugin owns its game detection, hooks, renderer preparation, input conventions, and output reinsertion.
- Loading the orchestrator is distinct from activating a graphics pipeline. It selects/prepares the plugin first.
- Keep public native interfaces as C ABI contracts. Do not pass STL containers, Rust types, ownership-ambiguous allocations, or exceptions across DLL boundaries.
- A recognized executable fingerprint is not proof of working hooks or rendering support. Report capability and validation status honestly.
- Keep standalone use independent of WSGM. The initial complete AC7 goal includes SR, MFG, and latency integration.

## Delivery

- Use a dedicated branch and PR for changes after the initial repository bootstrap.
- Preserve unrelated work. Do not edit game installations or run injection as part of routine build verification.
- Track implementation in `docs/implementation.md`. Distinguish built, synthetic-tested, game-tested, and device-tested results.
- Run `eng/verify.ps1` for native/SDK/Rust changes. Use focused checks for documentation-only edits.
- Keep component versions aligned through `VERSION` and the Cargo workspace version; verification checks the match.
- Keep downloaded reference checkouts and licensed engine/game source outside Git. Never copy reference code merely because it was useful to study.
