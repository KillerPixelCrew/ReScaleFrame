# AC7 binary analysis

Inspection and runtime capture: 6 September 2026. The initial inspection read the executable and Ghidra project; the later capture ran AC7 under Proton with the research proxy. [The hook map](ue418-hook-map.md) identifies the researched build.

## Why the runtime image was needed

| Section | Sampled entropy |
| --- | --- |
| `.text`, 38.9 MB | 7.997, 7.997, 7.998, 7.997 |
| `.rdata`, 17.1 MB | 4.118, 4.698, 4.513, 4.291 |
| `.bind`, 207 KB | 7.719, 7.997, 7.997, 7.997 |

The entry point is RVA `0x43e7310` in `.bind`. The shipped code has no useful references to the declared D3D11/DXGI import slots. Together with the later decrypted capture, this establishes that the shipped `.text` cannot be analysed as ordinary code. The 136,352 functions in the existing Ghidra project were disassembly of those protected bytes.

Plaintext `.rdata` still provides strings and GUIDs: 37 known interface IDs were found, including D3D12 device/queue/fence/swap-chain interfaces. `D3D12RHI` occurs 279 times and `D3D11RHI` 326 times. `d3d12.dll` and `D3D12CreateDevice` appear as strings rather than static imports. These are D3D12 implementation leads, not proof that AC7 or its middleware runs successfully with `-d3d12`. Plaintext `.pdata` also supplies function ranges.

RTTI recovery was unhelpful: the 20 unique descriptors were middleware/module types, not UE renderer classes.

## Tooling and checks

The [Ghidra scripts](../../tools/ghidra/README.md) generate DirectX types, COM slots/IIDs, and candidate graphics call sites from local headers and program analysis.

The recorded archive check matched all 513 vtable lengths. Comparison with compiler-derived types matched 523 of 530 shared sizes, including every DirectX structure; the seven differences were function-pointer typedefs and PE structures. A known DX11 test program verified creation/presentation call naming and preserved an ambiguous stack-local case as ambiguous.

Use [Ghidra MCP and Function ID databases](../tooling.md#ghidra-mcp-and-signatures) alongside these scripts. Function signatures identify compiled functions; `.gdt` archives describe types. Neither proves a hook's runtime suitability.

## Runtime dump

The proxy loads through AC7's `dinput8` import before the executable entry point. Its dump worker samples entropy every 250 ms and captures after the value falls below 7.0. The recorded game session reached an in-game state and behaved normally. See [current proxy settings](../../loader/README.md); the proxy now does more than this original dump experiment.

| Measurement | Value |
| --- | --- |
| `.text` entropy when the proxy loaded | 7.997, matching the on-disk file exactly |
| `.text` entropy when the dump was taken | 6.267 |
| `.text` entropy sampled across the dump afterwards | 6.107 to 6.467 |
| Sections captured | 10 of 10 |
| Bytes written | 71,385,127 |
| Imports described | 789 |
| Rip-relative calls through the import table | 19,888 |

| Import | Call site |
| --- | --- |
| `dxgi.dll!CreateDXGIFactory1` | `0x141fcb2a0` |
| `dxgi.dll!CreateDXGIFactory` | `0x141fcb2a6` |
| `d3d11.dll!D3D11CreateDevice` | `0x141fcb2b8` |

The import references provide stronger evidence of readable code than entropy alone. All three graphics creation call sites fell within one function.

Dump section raw offsets equal RVAs, and the image records the actual load base, so analysis addresses match the process. The dump is for analysis: it retains the wrapper entry point and does not rebuild imports into a runnable image. Keep it untracked; publish offsets, fingerprints, methods, and observations.
