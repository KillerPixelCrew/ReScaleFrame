# Binary analysis tooling and what it established about AC7

Inspection date: 6 September 2026. The game was read as a file and its existing Ghidra project was
opened read-only. It was not launched, injected, patched, or configured.

## There is no existing graphics analyser for Ghidra

A survey of published extensions found nothing that understands DirectX, COM interfaces, or
Unreal Engine. `awesome-ghidra` lists no such entry and neither does the `ghidra-extension` topic.
The nearest candidates do not apply here:

- **Ghidra-Cpp-Class-Analyzer** and Ghidra's own `RecoverClassesFromRTTIScript` recover classes
  from RTTI. `Ace7Game.exe` contains 20 unique type descriptors, all from CRI middleware
  (`CriManaSound`, `CriMvEasyPlayer` and similar) plus `type_info`, `IDelegateInstance`,
  `IModuleInterface` and `FDefaultModuleImpl`. UE4 ships with RTTI disabled, so the engine and
  renderer classes have none. Both tools are dead ends for this binary.
- **Prebuilt type archives** cover the Win32 and kernel APIs only. No published `.gdt` includes
  DirectX.
- **UE4 tooling** is entirely runtime dumpers that emit a name list to import afterwards. They
  need the game running, and the maintained ones start at engine 4.19. AC7 is 4.18.

So the tooling in `tools/ghidra/` was written for this project. See its
[README](../../tools/ghidra/README.md) for usage.

## What the tooling provides

`build-directx-types.py` turns the mingw-w64 headers into three artefacts: a Ghidra type archive
covering DX11, DX12 and DXGI, a machine-readable table of every COM interface's vtable slots and
6100 interface IDs, and a C header of slot indices and IID byte arrays for in-process code.

Two independent checks back the archive. The builder compares each parsed vtable's length against
the slot count it extracted separately, and all 513 interfaces agree. Separately, the archive was
compared against types exported from a compiler's own debug information for the same headers: 523
of 530 shared types have identical sizes, and every DirectX structure matches. The seven that
differ are function pointer typedefs and PE header structures, none of them DirectX.

`find-graphics-entrypoints.py` locates the DX11 and DXGI creation imports, follows the objects
those calls store, and decodes indirect calls through COM vtables into interface and method names.
It was verified against a purpose-built DX11 program with known answers, where it named every call
correctly and flagged the one genuinely ambiguous case instead of guessing.

## The executable's code section is encrypted

This is the finding that matters most, and it changes the approach.

| Section | Entropy sampled at four points |
| --- | --- |
| `.text` (38.9 MB) | 7.997, 7.997, 7.998, 7.997 |
| `.rdata` (17.1 MB) | 4.118, 4.698, 4.513, 4.291 |
| `.bind` (207 KB) | 7.719, 7.997, 7.997, 7.997 |

`.text` is uniformly at the theoretical maximum for random data across its whole length. The
entry point is at RVA `0x43e7310`, inside the trailing `.bind` section rather than in `.text`.
`.bind` is the section the Steam DRM wrapper adds, and it decrypts the real code at startup.

Two independent observations confirm the code is unreadable on disk:

- The import table declares `D3D11CreateDevice` from `d3d11.dll` and `CreateDXGIFactory` and
  `CreateDXGIFactory1` from `dxgi.dll`, but a scan of the whole file for any rip-relative operand
  targeting those import slots finds nothing. Ghidra agrees: the import address table entries have
  no references.
- The existing Ghidra project has 136,352 functions defined. They are disassembly of encrypted
  bytes and carry no meaning.

Static analysis of AC7's code from the shipped file is therefore not possible, and no amount of
Ghidra configuration changes that. The graphics stack has to be read from the runtime image.

## What the file still gives us

`.rdata` is ordinary plaintext, which is why the earlier string research worked, and it holds more
than strings:

- **Interface IDs.** 37 known GUIDs appear verbatim, an exact identification rather than a lead.
  Alongside the expected `ID3D11Texture2D`, `IDXGIDevice`, `IDXGIFactory` and `IDXGIFactory1`,
  the binary contains a full D3D12 set: `ID3D12Device`, `ID3D12CommandQueue`,
  `ID3D12GraphicsCommandList`, `ID3D12Fence`, `ID3D12PipelineState`, `ID3D12RootSignature`,
  `IDXGIFactory4`, `IDXGIFactory5`, `IDXGISwapChain1`, `IDXGISwapChain3`, `IDXGISwapChain4` and
  `IDXGIOutput6`.
- **RHI markers.** `D3D12RHI` occurs 279 times and `D3D11RHI` 326 times. `d3d12.dll` and
  `D3D12CreateDevice` are present as strings but not as static imports, which is how UE4 resolves
  D3D12 at runtime.

UE4.18's D3D12RHI is compiled into the shipped executable. That is a fact about the binary. It is
not evidence that `-d3d12` initialises, that the cooked content includes what a D3D12 path needs,
or that trueSKY and the other middleware cope. Those are runtime questions, and a native D3D12
route would remove the need for the planned DX11 to DX12 presentation bridge, so it is worth an
early answer.

`.pdata` is also plaintext. Its 2.4 MB exception directory enumerates every function's bounds,
which stays useful for annotating a dump later.

## Consequences for the plan

1. Code analysis moves to a runtime image. The project's own bootstrap DLL is already in the
   process, so it is the natural place to write the decrypted module out for offline analysis.
2. The tooling here is unaffected and is what the dump gets analysed with. The type archive and
   the vtable decoder work better on a dump than they ever could on the shipped file, because the
   imports are resolved by then.
3. `directx-slots.h` is usable in the shim itself, so hook code names a slot rather than repeating
   an index.
4. Dumps stay in `.local/`. Only observations, offsets and the executable hash belong in published
   research, matching the rule the rest of this directory already follows.
