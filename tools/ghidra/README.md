# Ghidra tooling

Ghidra ships no analyser that understands the graphics stack, and no public extension provides
one. These scripts fill that gap: they give Ghidra the DirectX types it lacks, then use them to
decode COM calls. They are development tools and are not part of any build.

Everything here writes to an untracked output directory. Keep generated archives, reports, and
Ghidra projects in `.local/`.

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

Three outputs land in the output directory:

| File | Contents |
| --- | --- |
| `directx.h` | the selected headers flattened into one declaration-only translation unit |
| `directx-vtables.json` | every COM interface's vtable slots and byte offsets, plus 6100 interface IDs |
| `directx.gdt` | a Ghidra data type archive, importable from the Data Type Manager |

The header is preprocessed with a Windows target, then stripped of GCC attribute syntax and
inline function bodies, because Ghidra's C parser accepts neither. The builder cross-checks every
parsed vtable's length in the archive against the slot count it extracted independently, and
reports any interface where the two disagree.

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
