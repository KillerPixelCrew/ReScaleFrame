# Ghidra tooling

DirectX type generation and COM-call analysis for Ghidra. Keep generated archives, reports, and projects in an untracked directory such as `.local/`.

## Script map and outputs

| Script | Input and environment | Output and write behavior |
| --- | --- | --- |
| `build-directx-types.py` | Local mingw-w64 headers; normal Python, optional PyGhidra | Replaces flattened header, x64 COM slots/IIDs and optional `.gdt`; no target program changes |
| `build-fid.py` | Symbol-bearing reference binaries/PDBs; Ghidra headless Java scripts | Overwrites matching imported reference entries by default; creates/reuses and populates local `.fidb` |
| `export-types.py` | `currentProgram` inside Ghidra/PyGhidra | Replaces destination `.gdt`, preserving program architecture; reads source program |
| `find-graphics-entrypoints.py` | `currentProgram` plus generated vtable JSON | Writes JSON report; `--apply` adds IID labels and replaces resolved-site EOL comments |
| `apply-native-names.py` | `currentProgram` plus recovered native-registration JSON | Default counts actions; `--apply` renames existing entries and may replace folded-alias function comments |
| `replay-research-names.py` | Research manifest and explicitly selected running MCP program | Default verifies evidence; `--apply` checks, renames and requests program save |

DirectX slot generation and COM decoding assume Windows x64 with eight-byte vtable pointers.
Header parse results, reference/PDB type layouts, registration names, and byte fingerprints provide
different evidence; none alone proves a game path executes or a runtime hook works.

Script Manager/headless execution owns transactions and persistence for the scripts using
`currentProgram`; they do not call program save themselves. Research replay explicitly requests
MCP save. Its apply phase and native-name application can complete some renames before a later
failure, so review reported results before retrying. Generated files are kept separately from source
manifests so provenance remains available when projects/archives are rebuilt.

## Recovering researched names

Commit research manifests and replay scripts so a lost Ghidra database does not lose the names
and their evidence. Keep game binaries and databases untracked. Ghidra's GUI project locator
rejects path components starting with a dot, so use `references/` for GUI projects instead of
`.local/`.

`replay-research-names.py` uses the running Ghidra MCP HTTP server. It requires the selected
program explicitly, checks every recorded byte range and function entry before writing, and
preserves unrelated names. Recorded ranges are fingerprints, not a claim of whole function size.
It waits for auto-analysis before saving. The manifest records the original executable and dump
hashes for provenance; replay checks the imported code ranges and language, not the file on disk.

```powershell
python tools/ghidra/replay-research-names.py docs/research/evidence/ac7-fg-cpu-names-20260930.json --program Ace7Game.exe.dump
python tools/ghidra/replay-research-names.py docs/research/evidence/ac7-fg-cpu-names-20260930.json --program Ace7Game.exe.dump --apply
```

Replay is idempotent. The default command checks evidence without changing names. `--apply`
renames matching functions and saves the program; a save failure is reported and must be retried.
These names document static research and do not enable runtime hooks.

The schema-1 manifest supplies language, RVAs, evidence byte lengths/SHA-256 and intended names.
RVA checks use the selected program's image base. All records pass preflight before any rename;
only the intended name or a `FUN_` default is accepted. The apply phase waits up to
`--analysis-timeout` seconds (default 180) and rechecks names after that wait. Each HTTP request has
its own 30-second timeout. `--url` defaults to `http://127.0.0.1:8089`.

## apply-native-names.py

Run this script inside Ghidra with JSON from `tools/find-native-registrations.py` for the matching
decrypted/imported program. Unlike evidence replay, it uses recorded absolute addresses directly
and does not verify code hashes or rebase them.

```bash
python -m pyghidra.ghidra_launch --install-dir "$GHIDRA_INSTALL_DIR" \
    ghidra.app.util.headless.AnalyzeHeadless <project_dir> <project> -process <program> \
    -noanalysis -postScript tools/ghidra/apply-native-names.py \
    --names .local/ghidra/ac7-native-names.json --shape all --exact-only --apply
```

`--shape` selects exec thunks, reflection constructors, or both. `--exact-only` excludes arrays
without an exact recovered count. Class namespaces are enabled by default; the current CLI has no
flag to disable them. Existing conflicting `USER_DEFINED` names are preserved, while other
analysis/imported names can be replaced. Missing function entries are reported without creating
functions. Linker-folded aliases are written to the renamed function's comment. Per-function
failures are reported while remaining records continue. Omit `--apply` to count the same decisions.

## Prerequisites

- Ghidra 12 or newer. Set `GHIDRA_INSTALL_DIR`, or pass `--ghidra-home`.
- mingw-w64 headers for the DirectX declarations. On Arch that is `mingw-w64-headers`. A
  preprocessor that can target Windows is also needed: `mingw-w64-gcc`, or `clang`, which the
  builder invokes as `clang --target=x86_64-w64-windows-gnu`.
- No pip install is required. `build-directx-types.py` creates its own virtual environment from
  the PyGhidra wheels bundled inside the Ghidra installation.

## build-directx-types.py

Produces the type information Ghidra is missing for DX11, DX12 and DXGI.

```bash
python3 tools/ghidra/build-directx-types.py .local/ghidra --gdt --ghidra-home /opt/ghidra
```

Generated files:

| File | Contents |
| --- | --- |
| `directx.h` | the selected headers flattened into one declaration-only translation unit |
| `directx-vtables.json` | every COM interface's vtable slots and byte offsets, plus interface IDs from the installed headers |
| `directx-slots.h` | selected COM slot indices and IID bytes for native code |
| `directx.gdt` | a Ghidra data type archive, importable from the Data Type Manager |

The header is preprocessed with a Windows target, then stripped of GCC attribute syntax and
inline function bodies, because Ghidra's C parser accepts neither. The builder cross-checks every
parsed vtable's length in the archive against the slot count it extracted independently, and
reports any interface where the two disagree.

`--header` is repeatable and replaces the default DX11/DX12/DXGI header set; `--sysroot` selects
the local include directory. `--compiler` takes one executable path, whose target configuration
the caller supplies. Without it, the tool finds a mingw-w64 compiler or adds the Windows target
flag to generic clang. Packed IID bytes use little-endian GUID Data1/2/3 followed by Data4 bytes.
The generated JSON preserves the preprocessing command and source headers. An optional archive
failure can leave the already generated header/JSON files available. `--no-bootstrap` requires
PyGhidra to be importable; otherwise `--gdt` may create a private offline environment and re-exec.

Open `directx.gdt` in Ghidra through Data Type Manager, then Open File Archive. Applying the DXGI
and D3D11 signatures to the imports improves argument recovery, which the entry point script
depends on.

## find-graphics-entrypoints.py

A Ghidra script. It locates the DX11 and DXGI creation imports, follows the objects they produce,
and decodes indirect calls made through COM vtables into interface and method names.

```bash
python -m pyghidra.ghidra_launch --install-dir "$GHIDRA_INSTALL_DIR" \
    ghidra.app.util.headless.AnalyzeHeadless <project_dir> <project> -process <program> \
    -readOnly -noanalysis -postScript tools/ghidra/find-graphics-entrypoints.py \
    --vtables .local/ghidra/directx-vtables.json --output .local/ghidra/report.json
```

Plain `analyzeHeadless` cannot run Python scripts, hence the PyGhidra launcher. Inside the GUI the
script runs from the Script Manager with the same arguments. `--apply` writes labels and comments
into the program; without it the run only reads.

`--depth` defaults to one caller/callee expansion level. `--limit` defaults to 400 and bounds that
expansion; initial import/storage seeds can exceed it. Decompilation uses a cached 180-second
timeout per function, and failures appear in the report. `--vtables`/`--output` defaults can be
supplied by `RSF_VTABLES`/`RSF_OUTPUT`. A report record's `resolved` flag means there are matching
slot candidates; use its `interface`/`method` fields to distinguish a known object from ambiguity.

It works in four steps:

1. Every known interface ID that appears verbatim in the program's data is located. A 16 byte
   GUID match identifies an interface exactly.
2. The creation imports are resolved through whichever form the program uses: external function,
   bare label in the EXTERNAL block, import table slot, or thunk.
3. Objects those calls store into static memory are recorded. The interface comes from the call's
   `riid` argument where there is one, and from the documented argument position otherwise.
4. Indirect calls in the located functions are decoded to a vtable offset. When the object traces
   back to known storage the method is named exactly; otherwise every interface sharing that
   offset is listed as a candidate.

Verified against a purpose-built DX11 program where the answers were known in advance:
`GetBuffer`, `CreateRenderTargetView`, `OMSetRenderTargets`, `Draw`, `Present` and `ResizeBuffers`
were each named correctly, including one call the compiler had inlined into another function. The
single call through a stack local was correctly reported as ambiguous rather than guessed.

A resolved slot says which method an offset belongs to. It does not prove the path executes or
that a hook there works.

## build-fid.py

Builds a Function ID database from reference binaries that carry PDBs, so a shipped game's
functions can be matched against an engine build whose names are known.

```bash
python3 tools/ghidra/build-fid.py .local/ghidra/fid \
    --binary ~/ue4/Blank-Win64-Shipping.exe --variant Shipping-Win64 \
    --binary ~/ue4/Blank.exe --variant Development-Win64 \
    --library UnrealEngine --release 4.18.3
```

It imports each binary into a `<library>/<release>/<variant>` project folder with Function ID and
the demanglers switched off to preserve reference symbol names. It then creates or reuses the `.fidb` and
runs `CreateMultipleLibraries`, answering that script's prompts through a generated properties file.

Each `--variant` pairs with `--binary` in argument order; omitted variants use binary stems.
The per-binary analysis timeout is `--timeout` seconds (default 7200). `--skip-import` reuses the
existing reference project but still populates the database. Import uses Ghidra `-overwrite`;
earlier successful imports are retained if a later command fails. Outputs include the reference
project, generated prompt properties, database and duplication report.

Put the `.pdb` beside its `.exe`. A binary without symbols contributes addresses and no names, and
the script says so rather than producing an empty library quietly.

FID hashes instruction sequences with selected operands masked. Match the reference target and
optimization configuration to the investigated binary; different inlining or engine patches can
change matches. Shipping and Development variants can expose different function bodies. Inspect
ambiguous matches against the binary and research evidence before assigning engine names.

## export-types.py

A Ghidra script that writes the current program's data types to a `.gdt`. Use it on a binary that
carries debug information, so layouts come from the compiler rather than a header parse. This is
the route for engine types, which Ghidra's C parser cannot read from UE4 headers: templates,
namespaces and generated headers all defeat it.

```bash
python -m pyghidra.ghidra_launch --install-dir "$GHIDRA_INSTALL_DIR" \
    ghidra.app.util.headless.AnalyzeHeadless <project_dir> <project> \
    -import UE4Editor-D3D11RHI.dll -postScript tools/ghidra/export-types.py \
    --output .local/ghidra/ue4-d3d11rhi.gdt
```

Structure layouts depend on the build that produced the symbols. An editor build defines
`WITH_EDITOR` and `WITH_EDITORONLY_DATA`, which changes member sets and therefore offsets. A
monolithic shipping build is the closer reference for a shipped game, and neither matches a
patched engine exactly. Treat exported offsets as leads to validate, not as ground truth.

`--prefix` selects root types by category-path prefix; referenced dependencies may still enter the
archive from other categories. Export uses the current program's language/compiler data organization
so pointer sizes remain consistent. The destination archive is replaced; archive conflicts use the
replacement handler inside one transaction, followed by archive save/close.
