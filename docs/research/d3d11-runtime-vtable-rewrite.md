# The Windows D3D11 runtime rewrites its own vtable

Research history: findings, hook addresses and pending statuses below apply to their recorded
experiments. Later increments can supersede earlier conclusions. See
[current implementation and validation](../current-status.md) before using this as a feature list.

Measured 26 September 2026 on Windows 11 Pro 26200 with the stock `d3d11.dll`, on a hardware
device created with no flags, feature level 11.0. Nothing here is from a game; every number is from
a scratch probe linked against the built `rsf_graphics` library.

## The question

`tests/frame_tap.cpp` had passed under Wine on DXVK in every recorded run and had never been run on
Windows. Built with MSVC for the first time, it failed 23 checks. The binding-side checks passed:
render targets were substituted, viewports scaled, gates opened. Every check that needed a draw to
be observed failed, and the tap's own counters said why: after the depth replay stage, zero draws
were ever reported again while binding calls kept being counted.

## What was tried

1. The same test against the pre-change frame tap, with only the two tap source files stashed.
   Same 23 failures, so the tree at `75daac4` fails on Windows too and my edits were not the cause.
2. A probe that installs the tap, binds a target, watches it and draws twice, with no shaders. Both
   draws reported. So the hooks work on this runtime.
3. The same with a depth view bound and one call through each draw entry point: `DrawIndexed`,
   `DrawIndexedInstanced`, `DrawInstanced`, `Draw`. Every one reported with the right kind.
4. A trace line at the entry of the instanced hook, in the failing test. It never printed for the
   test's second draw. The hook was not entered, although its slot had been patched.
5. A probe that snapshots the context's vtable pointer at every step. The pointer never changed.
6. A probe that snapshots every entry of the table before and after install and reports, after each
   call, which entries no longer hold the tap's hooks and whether they hold the pre-install pointer
   or a third value. That answered it.

## What the runtime does

The immediate context's vtable is on the heap, in the same allocation region as the context object,
not in `d3d11.dll`'s read-only data. The runtime rewrites a fixed family of its entries:

| After the call | Slots no longer holding the hook | What they hold |
| --- | --- | --- |
| creating resources, `OMSetRenderTargets`, `RSSetViewports`, `Draw` | none | hooks intact |
| `CopyResource` | none | hooks intact |
| `Map` of a staging texture for reading | 12 13 20 21 38 39 40 41 42 46 47 48 49 50 51 52 53 54 57 115 116 | a third value: a different implementation |
| `Unmap` | the same | the same |
| the next `Draw` | 12 13 20 21 38 39 40 50 | the pre-install pointer |
| `OMSetRenderTargets` again, `Map` of a dynamic buffer with write-discard, `Draw` | the same | unchanged |
| `Flush` | the full family again | the third value again |
| the next `Draw` | 12 13 20 21 38 39 40 50 | the pre-install pointer |

The slots are, counting the three `IUnknown` and four `ID3D11DeviceChild` entries: `DrawIndexed`,
`Draw`, `DrawIndexedInstanced`, `DrawInstanced`, `DrawAuto`, `DrawIndexedInstancedIndirect`,
`DrawInstancedIndirect`, `Dispatch`, `DispatchIndirect`, `CopySubresourceRegion`, `CopyResource`,
`UpdateSubresource`, `CopyStructureCount`, `ClearRenderTargetView`, `ClearUnorderedAccessViewUint`,
`ClearUnorderedAccessViewFloat`, `ClearDepthStencilView`, `GenerateMips`, `ResolveSubresource`, and
`ID3D11DeviceContext1`'s `CopySubresourceRegion1` and `UpdateSubresource1`: the whole
work-submission family. The second table only reports the eight of those the tap patches, because
that is what the probe compared; the "third value" rows list every slot whose content changed.

So the runtime keeps two sets of implementations for that family and flips the table between them:
to one set when a flush-class call runs (a read-back `Map`, `Flush`; a write-discard `Map` of a
dynamic buffer does not), and back on the next piece of work. What the two sets do differently is
not established here and does not need to be; what matters is that each flip is a plain write of
the runtime's own pointers over whatever was in the slots, which removes any hook patched there.
DXVK's table is static and never rewritten, which is why the test passed under Wine and why no
Windows run of this project could ever have observed a draw after the first read-back.

## What was done

`runtime/graphics/src/frame_tap.cpp`, frame tap ABI 8:

- Every hook goes through `enter_hook`, which compares one sentinel slot (13, `Draw`) against the
  tap's own function and, when it differs, re-applies the whole patch table, recording what the
  runtime had written as the new original. That is the right thing to forward to: the variant in the
  slot is the one for the runtime's current state, and the runtime rewrites the slot again before
  that state changes.
- The work-submission hooks re-check after forwarding, because the flip back happens inside the
  call itself.
- The flush-class calls and the rest of the family, `Map`, `Unmap`, `Flush`, `Dispatch`, the copies,
  the clears, `GenerateMips`, `ResolveSubresource` and the two `ID3D11DeviceContext1` members, are
  hooked as pass-throughs whose only purpose is that check after forwarding. The two
  `ID3D11DeviceContext1` slots are patched only when the context has that interface, because a
  table for the base interface has no slot 115.
- `rsf_frame_tap_refresh` exists for the present hook, since the flush inside Present is the one
  flip no context hook sees. The bridge calls it every present.
- `rsf_frame_tap_status.vtable_refreshes` counts the re-applications. Zero under DXVK.

`tests/frame_tap.cpp` then passes on Windows: 22 of 22 tests, MSVC 14.51, Visual Studio 2026.

## What is not covered

- A flip caused by a call this does not hook, such as `GetData` on a query, is noticed at the next
  hooked call of any kind. Draws between the two are unobserved. Unreal issues binding calls before
  nearly every draw, so the window is small, and `vtable_refreshes` against `calls_seen` in the F7
  report will say how often it happens in the game.
- Only the immediate context's table is patched. Deferred contexts have their own; the tap does not
  observe them, which was already the case.
- One runtime, one machine. The slot family and the flip points are what this `d3d11.dll` does; a
  different build of the runtime could differ, and the sentinel check would still catch a rewrite
  of slot 13 but not one that spared it.
