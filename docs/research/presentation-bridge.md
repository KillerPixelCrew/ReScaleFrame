# The DX11 to DX12 presentation bridge

Design record for the bridge in the [representation plan](../representation-plan.md), written
7 September 2026 before any of it was built. Measurements are added per milestone; until then every
statement here is design, and the status section says so.

## Why it exists

No frame generation SDK runs on D3D11 ([vendor contracts](vendor-fg-contracts.md)), and two of
three super resolution paths need D3D12 as well. AC7 is D3D11. So the game keeps a D3D11 swap chain
facade while a same-adapter D3D12 queue owns the real presentation chain, which is what
[architecture.md](architecture.md) already asked for: "Expose a DX11-facing swap-chain facade to
the game while a same-adapter DX12 queue owns the real presentation chain. Preserve the outward COM
contract. Do not create a competing HWND swap chain."

## Intercept

The chain has to be replaced at creation, not wrapped afterwards, because a BRIDGED facade never
creates the game's D3D11 chain at all. Hooks: `IDXGIFactory::CreateSwapChain` (vtable slot 10, from
a factory the carrier creates), `IDXGIFactory2::CreateSwapChainForHwnd` (15), and
`D3D11CreateDeviceAndSwapChain` in the game's import table (the fo4test route). `ID3D11Device` slots
3, 5, 11, 12, 15 are patched once the first creation call reveals the device (the allocation watch).
Installed from the carrier's first worker at attach; a chain that already existed when the hook
armed refuses BRIDGED with `restart_required = INTERCEPT_LATE`.

## Facade

`SwapChainFacade final : IDXGISwapChain4`, two modes fixed at creation. PASSTHROUGH wraps the game's
chain and runs the UI composite and the overlay inside `Present`. BRIDGED returns the D3D11 side of
a shared `game_backbuffer` from `GetBuffer(0)`, synthesises descriptors from the game's request
(never the flip-model count), forwards fullscreen state, forwards `DXGI_PRESENT_TEST` only, returns
the real chain's `HRESULT` (`DXGI_STATUS_OCCLUDED` passes through), and reports
`GetCurrentBackBufferIndex` as 0. MSAA requests degrade to PASSTHROUGH.

Inner chain by frame generation owner: DLSS-G, the Streamline proxy chain on the
`slUpgradeInterface`d factory, device and queue; FSR-FG, the chain FidelityFX created; XeFG, the
proxy from `xefgSwapChainD3D12GetSwapChainPtr`; no owner, our own flip-model
`CreateSwapChainForHwnd` (`FLIP_DISCARD`, `max(2, requested)` buffers, tearing when supported).
Owners are fixed at creation; a vendor or mode change is `NEEDS_RESTART`.

## Sharing and synchronisation

Same-adapter `D3D12CreateDevice` (feature level 12_0), one DIRECT queue, per-slot allocators and
lists. One `ID3D12Fence` created shared and opened on D3D11 through `ID3D11Device5::OpenSharedFence`.
Surfaces are created on D3D11 with `D3D11_RESOURCE_MISC_SHARED | D3D11_RESOURCE_MISC_SHARED_NTHANDLE`
and opened on D3D12 through `OpenSharedHandle`, stamped with a resource generation. Set:
`game_backbuffer` (game format), `hudless[ring]`, `ui[ring]` (`R8G8B8A8_UNORM`), `depth[ring]`
(`R32_FLOAT`, render size), `motion[ring]` (`R16G16_FLOAT`, decoded), `present_copy[ring]`
(D3D12-only), and the `sr_*` set when super resolution runs on D3D12.

Per Present, BRIDGED and not a TEST, on the render thread:

1. `s = present_index % ring`; CPU wait until the fence reaches `slot_fence[s]`.
2. `CopyResource(hudless[s], game_backbuffer)`.
3. Composite `ui[s]` over `game_backbuffer`, then the overlay.
4. Depth and motion held from this interval's SR gate into `depth[s]` and `motion[s]`; no SR pass
   this interval means `interpolate = 0`.
5. `ctx4->Signal(fence11, ++v)`; `queue->Wait(fence12, v)`.
6. List `s`: barriers; `CopyResource(present_copy[s], game_backbuffer)`; transitions to the tagged
   states; `fg->tag_frame`; execute.
7. `present_chain->Present`; `fg->after_present`.
8. `queue->Signal(fence12, ++v)`; `slot_fence[s] = v`; `ctx4->Wait(fence11, v)` on the GPU, so the
   game's next writes to `game_backbuffer` wait for the D3D12 copy.
9. Clear `ui[(s+1) % ring]` to transparent; `present_index++`; close the tap's gates.

Without an owner, steps 6 and 7 copy into the real back buffer, which is how the bridge is tested
before any vendor exists. The rules from `architecture.md` hold: a CPU signal is not GPU
completion; a bounded ring needs fenced reuse; 4x MFG does not imply four application slots; neither
Present returning nor a fence on one queue proves every SDK queue is done.

## Resize and edge cases

`ResizeBuffers`: drain both sides to the last slot fence, release surfaces and real-buffer
references, normalise the count (0 means current; the game's 1 is ignored on the real chain),
`fg->resize` or `present_chain->ResizeBuffers`, recreate on size or format change, bump the
generation, return the `HRESULT`. Fullscreen is forwarded and reported. Minimised or zero size means
`interpolate = 0` and a forwarded Present. `DXGI_ERROR_DEVICE_REMOVED` or `RESET` marks the bridge
dead and every later Present forwards without work. Loading, video, menus and cuts arrive as
`NO_FG` from the plugin's screen policy: `interpolate = 0`, null tags for DLSS-G, `reset` on cuts.
Sync interval and `ALLOW_TEARING` pass through unchanged.

## What the fixtures can and cannot prove

Synthetic tests run under Wine with DXVK for D3D11 and vkd3d-proton for D3D12 in a dedicated
prefix. A D3D12 device, a flip-model chain, fences and the facade's COM contract are testable there.
Whether a DXVK-exported NT handle and a DXVK `ID3D11Fence` open on a vkd3d-proton device under
system Wine is unmeasured; the fixture prints the `HRESULT`s and skips with 77 rather than failing,
and the first game run under Proton is the real measurement. Windows results are a separate status
class and are recorded when they exist.

## Measured: shared handles are not available in the plain Wine prefix

7 Sep 2026, `tests/shared_surface.cpp` against `~/.wine` under system Wine.

`IDXGIResource1::CreateSharedHandle` returns `E_NOTIMPL` (`0x80004001`). The D3D11 texture is
created with `SHARED | SHARED_NTHANDLE` without complaint and then no handle can be obtained from
it, so the export half refuses and the import half is never reached. A D3D12 device is created on
the same adapter successfully, so D3D12 itself is present and working.

The prefix matters and the result does not generalise. `~/.wine/drive_c/windows/system32/d3d11.dll`
is 469 KB and `dxgi.dll` 252 KB, which are Wine's own WineD3D rather than DXVK; DXVK's are several
megabytes. So what this measures is that **WineD3D does not implement shared NT handles**, which is
a different statement from anything about DXVK or vkd3d-proton, and a much less interesting one.

## Measured: under DXVK and vkd3d-proton, sharing works

7 Sep 2026, the same fixture in a prefix built by `eng/wine-test-prefix.sh` from an installed
Proton's DXVK and vkd3d-proton.

Every stage passes. A D3D11 texture created with `SHARED | SHARED_NTHANDLE` exports an NT handle,
an `ID3D12Device` on the same adapter opens it, and the opened resource describes the same extent
and format on both sides. A fence created on D3D11 with `D3D11_FENCE_FLAG_SHARED` opens on D3D12,
and a value signalled through `ID3D11DeviceContext4::Signal` is observed by
`ID3D12Fence::GetCompletedValue` on the other side.

That is the whole foundation of the bridge, and it means the bridge is possible where the game
actually runs. Risk 7 in the plan is retired by measurement.

The two results together are the useful part: WineD3D cannot do this and DXVK can, so a fixture
that had only ever run in the default prefix would have reported the bridge impossible and been
wrong about the only environment that matters. The script exists so the distinction cannot be
skipped by accident.

One test does not survive the move. `texture_dump` fails under DXVK with the subprocess killed,
reproducibly rather than intermittently, while passing under WineD3D. That is a real difference and
not the flakiness seen before; it is unexamined and recorded here rather than left as a red run
somebody explains away.

The fixture skips with 77 rather than failing, here and anywhere else the two runtimes disagree,
because that is a fact about the environment and not a defect in the code. What it must never do is
pass quietly while the bridge cannot work, which is why every step prints what it got.

## Status

The surfaces and the fence are built (`shared_surface.cpp`), with the create, export, open and
signal round trip covered. Everything else in this file is design. The plan's M4 builds the bridge
with a pass-through present and no frame generation; M6 to M8 add the vendors; the measurements from
each run are appended here.
