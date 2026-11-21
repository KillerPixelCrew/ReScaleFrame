# Unity D3D11 shared surface ownership

The source audit starts at `1eda132`, after the [shared graphics audit](rust-unity-d3d11-shared-device.md)
moved the package D3D11 bridge's devices, fences and command slots into one Rust record. The
question was whether the bridge's six shared surfaces, their key and replacement, and the hold a
frame keeps on them could join that record without changing the managed, native public or layer
table ABI, the surfaces the actual callbacks consume, or the Vulkan and Mono paths.

## What moved

`games/unity-mono/src/d3d11_state.rs` (boundary `d3d11_state.h`) now also owns the six
`rsf_shared_surface` slots of the C++ `surfaces` array, in the same indices: colour, depth, motion,
output, the HUD-less scene and the Editor preview source. Each slot keeps the key the C++ compared,
the source texture's width, height and DXGI format, and the surface created for it. Rust decides
whether a slot is borrowed as it is or replaced, waits for retirement before a replacement, and
owns the surface lease described below.

`integrations/unity/native/bridge.cpp` keeps the typed work as leaves of the record's ops: reading
`D3D11_TEXTURE2D_DESC`, choosing the shared format (`R32_FLOAT` for depth, `R16G16_FLOAT` for
motion, typeless and sRGB colour formats mapped to their `FLOAT` or `UNORM` forms; the
[pass plan](rust-unity-d3d11-pass-plan.md) later moved this choice into Rust), the completion
fence wait, `rsf_shared_surface_create` with the same render-target setup and `services` log
routing, and `rsf_shared_surface_destroy`. The copies, barriers and render-pass calls still use
`rsf_shared_surface_d3d11` and `rsf_shared_surface_d3d12` on the view Rust lends. The D3D11 texture
leases that queued packets hold from `enqueue` to their render event stayed native here: they belong
to the packet store, exist before the record is created and are shared with the Vulkan path's
admission. They later moved into the Rust packet store; see
[queued packet texture leases](rust-unity-d3d11-packet-leases.md).

## Creation, partial failure and replacement

The record starts with every surface slot empty. A failed record creation therefore never has
surfaces to roll back; the existing rollback order is unchanged.

`rsf_unity_d3d11_prepare_surface` runs under the surface lease. A slot whose key matches lends its
surface without any leaf. Otherwise Rust copies the record's handles and ops, releases its mutex
and, when an existing surface will be replaced, calls the wait leaf with
`rsf_unity_interop_submitted()`. Every submission that could read the old surface, ring slots and
the Editor preview's slotless window submission alike, took a completion value no later than that
one, so the completion fence reaching it retires the old surface on D3D12. The D3D11 copies into or
out of it are on Unity's immediate context, which keeps its own references until the copies run.
Rust then calls the create leaf, publishes the new surface under the mutex and destroys the old
one outside it.

Refusals keep the published surface, as the C++ did: a timed-out or failed wait (10 s, the same
`wait` as before) and a refused creation both return no view, the frame returns before recording
and the next frame tries again. A refused creation's null output goes to the destroy leaf, which
accepts null. If the lease or record had gone between creation and publication, the new surface is
destroyed instead; close and release refuse while the lease is held, so this is a defensive path.

## Per-frame borrows

`execute11` holds the surface lease (`SurfaceLease`, `rsf_unity_d3d11_surfaces_begin` and
`rsf_unity_d3d11_surfaces_end`) inside `render_event`'s frame callback:

- The Editor preview window path takes it before querying the source texture and keeps it through
  the upload copy, the shared fence pair, the D3D12 wait, the window render pass, the hand-back
  and `retire_interop_submission`. The plain window path borrows no surface and takes no lease.
- The reconstruction and HUD-less path takes it before choosing its ring slot and keeps it through
  the input copies, barriers, both render-pass calls, the hand-back and the output copy into Unity's
  texture.

A view lent under the lease stays valid until it ends: within one lease a borrowed slot is never
replaced. The managed packets never ask for one slot twice in a render event. The reconstruction
packet fills slots 0 to 3 once each, and the HUD-less packet (`ReScaleFrameRendererFeature.cs`)
carries only colour, so slot 4 is prepared once. The surfaces these callbacks consume are therefore
the C++ ones. The lease has one holder at a time. Unity issues D3D11 plugin events on its render
thread, so a second holder is not expected; one would see a refused begin and leave native output
for that frame.

## Exact retirement and checked close

The checked close is unchanged in what it proves: `try_close_shared_bridge` closes callback
admission, then `rsf_unity_d3d11_try_close` reads the completion fence against the ring and the
shared fence against the last D3D11 copy. Because every surface user holds a frame callback, the
lease cannot be held once callback admission has closed. The Rust close still refuses, without
closing, while a lease is held, and release refuses with it. On commit
`rsf_unity_d3d11_release` drops the record, which destroys the surfaces first, then the command
slots, then the shared fence, completion fence, queue and devices, then forgets the ring. That is
the C++ order: `rsf_unity_package_bridge_finish_retirement` destroyed the surfaces under the bridge
mutex immediately before the record release, and the release still runs under that mutex.

## Intentional differences

- A first creation of a slot no longer waits for the completion fence. With no surface to retire
  the C++ wait had nothing to protect. Replacements wait exactly as before.
- When release refuses without a checked close, the surfaces stay owned by the record with the
  devices. The C++ destroyed them before calling the refusing release.
- Unreleased surfaces are no longer destroyed by C++ static destruction at DLL unload. They stay
  with the unreleased record, as its devices already did.
- Within one lease, a second request for a borrowed slot with a different key is refused instead of
  replacing the surface under the first view. No managed packet makes such a request.
- A frame whose surface lease is refused returns before choosing its ring slot. Before, nothing
  could refuse there.

## Verification scope

Source comparison covered `prepare_surface` against its C++ version (key, format mapping, wait
value, setup, publication and destruction order), every surface use in `execute11` (Editor
preview, reconstruction, HUD-less and output copy), the HUD-less managed packet, the close and
release order, and confirmed that the `rsf_unity_*` exports, `rsf_unity_package_platform`,
`rsf_unity_native_api`, the managed package and `vulkan_bridge.cpp` are unchanged. The ops record
grows from six to nine pointers and is asserted on both sides. New unit tests cover borrow,
replacement order, refusals, the one-holder lease and release order. The formatting check used
rustfmt 1.11.0 (Rust 1.100), since the pinned 1.94.1 toolchain is not installed here. Locked
offline Cargo metadata and Git whitespace checks also passed. Native and Rust compilation and
linking, Clippy, the unit tests, the D3D11 fixtures, Unity Editor and player runs, resolution and
format changes that replace surfaces, device removal and teardown timing need user verification.
