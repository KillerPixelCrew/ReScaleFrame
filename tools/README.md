# Research tools

`inspect-game.py` performs a read-only PE fingerprint, import-table inspection, and bounded search for known source-string leads. It does not load or modify the executable. String matches are not resolved functions or verified hook sites.

```powershell
python tools/inspect-game.py "path/to/Ace7Game.exe" ".local/ac7-executable.json"
```

Create the output directory before running it. Keep machine paths, binaries, captures, and Ghidra projects in `.local/` or another untracked workspace. The published research fingerprint uses only the executable name.

Ghidra/PyGhidra are development tools, not runtime dependencies.

`ghidra/` holds the DirectX type builder and the COM call decoder that Ghidra does not provide on its own. See [ghidra/README.md](ghidra/README.md) for usage and [docs/research/ghidra-tooling.md](../docs/research/ghidra-tooling.md) for what they established about the installed game.

## Analysis tools

| Tool | Purpose |
| --- | --- |
| `map-source-files.py` | Attribute code to engine source files using embedded `__FILE__` strings |
| `find-string-refs.py` | Count code references to a string, for locating console variable handling |
| `parse-capture.py` | Summarise RenderDoc allocations and binding-based pass candidates |
| `ue4-view-layout.py` | Derive stock view-buffer offset candidates and inspect dumped values |
| `verify-view-layout.py` | Check AC7 matrix, jitter, and size relationships across captured buffers |
| `analyze-view-buffers.py` | Identify view buffer fields by how they behave across captures |
| `parse-ue-sdk.py` | Index a dumped Unreal reflection SDK: class layouts, field offsets, functions |
| `find-native-registrations.py` | Join that index to a module dump through the engine's own registration arrays |

The reflection pair recovered 10,728 named addresses in AC7 and is written up in
[docs/research/ue-reflection-mapping.md](../docs/research/ue-reflection-mapping.md). It names
gameplay and UMG code only; the renderer has no reflection data and needs the source-string route.

The procedure these belong to is in [docs/research/methodology.md](../docs/research/methodology.md).

The capture parser groups work by render-target changes. It does not fully track inherited SRV bindings, compute-only passes, or deferred contexts. Treat its read lists as leads for per-draw inspection.
