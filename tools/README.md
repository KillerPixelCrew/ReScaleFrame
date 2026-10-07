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
| `inspect-game.py` | Hash and inspect ordinary PE32+ files without loading them |
| `map-source-files.py` | Map candidate references to embedded `__FILE__` paths in a module dump |
| `find-string-refs.py` | Scan candidate RIP-relative string references for disassembler follow-up |
| `parse-capture.py` | Summarise RenderDoc allocations and binding-based pass candidates |
| `ue4-view-layout.py` | Derive stock view-buffer offset candidates and inspect dumped values |
| `verify-view-layout.py` | Check AC7 matrix, jitter, and size relationships across captured buffers |
| `analyze-view-buffers.py` | Identify view buffer fields by how they behave across captures |
| `parse-ue-sdk.py` | Index a dumped Unreal reflection SDK: class layouts, field offsets, functions |
| `find-native-registrations.py` | Join that index to a module dump through the engine's own registration arrays |
| `analyze-ac7-motion-capture.py` | Summarize F9 engine/draw/sample evidence and write occupancy masks |
| `exposure-probe.cpp` | Manual frozen-input NVIDIA DLSS precision/transport experiment |
| `package-ac7.py` | Package existing Release artifacts, pinned SR runtimes, source and notices |

The reflection pair recovered 10,728 named addresses in AC7 and is written up in
[docs/research/ue-reflection-mapping.md](../docs/research/ue-reflection-mapping.md). It names
gameplay and UMG code only; the renderer has no reflection data and needs the source-string route.

The procedure these belong to is in [docs/research/methodology.md](../docs/research/methodology.md).

The capture parser groups work by render-target changes. It does not fully track inherited SRV bindings, compute-only passes, or deferred contexts. Treat its read lists as leads for per-draw inspection.

## Inputs, outputs and limits

The Python analysis tools use the standard library and do not execute game code. `inspect-game`
reads a normal file-backed PE; string/source mapping expects the loader's reconstructed module
dump, whose raw offsets equal RVAs. Those scanners test displacements at every byte without
decoding instructions. Confirm reported candidate addresses in a disassembler before naming
functions or selecting hooks. ASLR addresses and preferred image addresses are distinct.

View tools read little-endian float32 dumps. `ue4-view-layout` implements a narrow macro parser
and stock packing rules; unknown types make later offsets provisional. Its plausibility check
is weaker than `verify-view-layout`'s build-specific matrix/camera/jitter relationships. Behavioral
classification in `analyze-view-buffers` identifies candidates, not semantic proof.

`parse-ue-sdk` preserves the dumper's comments, including unknown padding and inferred owners.
Registration mapping uses pointer-table phase, sorted-name boundaries and greedy class matching.
It retains folded aliases and distinguishes full-set matches from partial ones; recovered exec
thunks are not the member functions they call. Neither tool resolves unreflected renderer code.

Analysis inputs are retained. Optional `--output` paths and required JSON destinations can be
overwritten. The F9 motion analyzer additionally writes `analysis.json` and `fNNN_written.pgm`
inside the capture directory. Masks count encoded pixels, not displacement correctness.

## Manual probe and packaging

The native `rsf_exposure_probe` target reads tightly packed colour, optional float32 depth and
synthetic zero motion/camera state on a Windows NVIDIA D3D11 device. It repeatedly evaluates
one frozen image and writes the final output. Optional compile gates enable NGX tracing and
RenderDoc capture; CLI help and environment options are documented in its source. The probe
is outside CTest and cannot establish moving-scene temporal quality.

`package-ac7.py --help` describes required local SDK roots and optional expected DLL hashes.
It requires a clean git revision and existing Release binaries; it does not build, deploy or
launch them. It validates pinned FSR/XeSS runtime hashes and package defaults, collects notices,
then writes binary/source ZIPs and checksums under `build/releases` unless overridden. Cargo
metadata may fetch dependencies, and missing crate notices may be downloaded at an exact VCS
revision. Version-named archives can replace earlier packages and failed runs leave staging
material. ZIP/hash integrity does not prove source-to-binary correspondence or game acceptance.
