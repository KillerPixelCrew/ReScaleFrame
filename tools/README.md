# Research tools

`inspect-game.py` performs a read-only PE fingerprint, import-table inspection, and bounded search for known source-string leads. It does not load or modify the executable. String matches are not resolved functions or verified hook sites.

```powershell
python tools/inspect-game.py "path/to/Ace7Game.exe" ".local/ac7-executable.json"
```

Create the output directory before running it. Keep machine paths, binaries, captures, and Ghidra projects in `.local/` or another untracked workspace. The published research fingerprint uses only the executable name.

Ghidra and a Ghidra MCP bridge can support the next binary-analysis phase. They are external development tools, not runtime dependencies. Local installation pointers can be stored in `.local/tooling.json`; configuring the bridge is separate from building ReScaleFrame.

`ghidra/` holds the DirectX type builder and the COM call decoder that Ghidra does not provide on its own. See [ghidra/README.md](ghidra/README.md) for usage and [docs/research/ghidra-tooling.md](../docs/research/ghidra-tooling.md) for what they established about the installed game.

## Analysis tools

| Tool | Purpose |
| --- | --- |
| `map-source-files.py` | Attribute code to engine source files using embedded `__FILE__` strings |
| `find-string-refs.py` | Count code references to a string, for locating console variable handling |
| `parse-capture.py` | Summarise a RenderDoc capture's targets, pass timeline and what each pass reads |
| `ue4-view-layout.py` | Compute view uniform buffer offsets from engine source, and verify them against a dump |
| `analyze-view-buffers.py` | Identify view buffer fields by how they behave across captures |

The procedure these belong to is in [docs/research/methodology.md](../docs/research/methodology.md).
