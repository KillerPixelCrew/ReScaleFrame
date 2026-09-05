# Research tools

`inspect-game.py` performs a read-only PE fingerprint, import-table inspection, and bounded search for known source-string leads. It does not load or modify the executable. String matches are not resolved functions or verified hook sites.

```powershell
python tools/inspect-game.py "path/to/Ace7Game.exe" ".local/ac7-executable.json"
```

Create the output directory before running it. Keep machine paths, binaries, captures, and Ghidra projects in `.local/` or another untracked workspace. The published research fingerprint uses only the executable name.

Ghidra and a Ghidra MCP bridge can support the next binary-analysis phase. They are external development tools, not runtime dependencies. Local installation pointers can be stored in `.local/tooling.json`; configuring the bridge is separate from building ReScaleFrame.
