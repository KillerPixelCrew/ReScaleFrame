# Unity D3D11 queued packet texture leases

The source audit starts at `31a06a94`, after the [shared surface audit](rust-unity-d3d11-surfaces.md).
The question was whether the package D3D11 bridge's `pending_leases`, the texture references a
queued packet holds from `enqueue` until its render event ends, could move into the existing Rust
packet store without changing slot identity, GPU order, callback behaviour, the managed or native
public ABI, or the Vulkan and Mono D3D12 routes.

## What moved

`games/unity-mono/src/bridge_state.rs` (boundary `bridge_state.h`) already stored the packets. Each
store entry can now also carry a lease: the four resource pointers in packet order (colour, depth,
motion, output) and the leaf that releases them. A leased entry has a third state besides free and
queued, consuming, for the time its render event uses it. `d3d11_state.rs` (boundary
`d3d11_state.h`) adds the D3D11 route's admission, `rsf_unity_d3d11_admit_packet`, and the typed
leaf table `rsf_unity_d3d11_texture_ops`. The leases do not depend on the graphics record: packets
are admitted before the record exists, as before.

`integrations/unity/native/bridge.cpp` keeps the typed calls: `retain_texture` (`IUnknown::AddRef`)
and `release_texture` (`IUnknown::Release`). The native `pending_leases` array and its `Leases`
alias are gone. The bridge's `guard` still serializes the host services that `enqueue` reads, but
no longer covers the take.

Mono D3D12 (`games/unity-mono/src/bridge.cpp`) keeps its own `pending_leases` of `ID3D12Resource`
and calls `rsf_unity_frames_admit_packet` and `rsf_unity_frames_take_packet` as before. Its packets
carry no lease, so take frees them at once and `rsf_unity_frames_end_packet` is never called.
Vulkan packets in the package bridge are admitted through `rsf_unity_frames_admit_packet` without
a lease, also as before, because image handles are not reference counted.

## Trace

Admission. Managed code calls `enqueue` through `rsf_unity_native_api` with a packet it built for
one event: `ReScaleFrameUpscaler.cs` issues the reconstruction packet on `EventId`,
`ReScaleFrameRuntime.cs` the window packet on `EventId + 1` and `ReScaleFrameRendererFeature.cs`
the HUD-less packet on `EventId + 2`, each only when `enqueue` returned an address. `enqueue` is not
a frame callback; admission is gated by the store's `closed` flag and the activity flag. On D3D11 it
now calls `rsf_unity_d3d11_admit_packet`, which reads the size field, copies the packet, validates
it against the session, ABI, event kind resources and camera, and finds a free slot exactly as the
unleased admission does. Under the store mutex it then calls `retain` for each non-null pointer of
the stored copy and records the lease, and only then returns the slot's address. A packet refused
at any step retains nothing. The C++ retained the caller's pointers after admission under `guard`,
which also held the take; the values are the same and the address was never usable before the
references existed in either version.

Packet and event generation. The packet's `generation` is the policy generation stamped by the
managed side; admission does not check it and execute11 applies a pending change through
`rsf_unity_frames_pending_change` and `settle_change` as before. The event index (`event_id`, `+1`,
`+2`) selects the reconstruction, window or HUD-less path in `execute11`, and Vulkan in
`rsf_uvk_render`. Neither depends on the lease.

Render start. `render_event` enters the frame callback, refuses ids outside the reserved range
(the packet stays queued, unchanged) and calls `rsf_unity_frames_take_packet`. Take matches only a
queued entry at that address, copies the packet out and, for a leased entry, marks it consuming
instead of freeing it. The address can then not be taken a second time, and the slot cannot be
reused while the event reads the textures. `TakenPacket` holds the slot index for the rest of the
event and is declared after the frame callback guard, so it ends first, as the C++ `Leases` local
did.

Consumption. `execute11` queries `ID3D11Texture2D` from the packet pointers, copies them into the
shared surfaces, records and submits the D3D12 pass and copies the output back. All of this runs
while the store holds the references, as the native local held them before. The immediate
context's own references cover the copies after the event returns, so the release point relative
to GPU work is unchanged.

End, dropped and failed packets. When `render_event` returns, by any path, `TakenPacket` calls
`rsf_unity_frames_end_packet(slot)`. Rust frees the consuming slot under its mutex and calls
`release` for each non-null resource after unlocking. The paths are the C++ ones: inactive bridge,
refused record, refused surface lease, timed-out slot retirement, refused copies or fences, a
completed frame and an exception unwinding to the handler. An end for a slot that is not consuming
does nothing, which covers Vulkan packets (freed at take) and a slot that another packet reused
after such a take. A packet whose event Unity never runs stays queued with its lease, as the native
array kept it.

Checked retirement. `rsf_unity_frames_try_close` refuses while any entry is used, which now
includes consuming entries; a consuming entry also implies an entered frame callback, which already
refused. `rsf_unity_d3d11_try_close` and the record release are unchanged. On commit,
`rsf_unity_package_bridge_finish_retirement` used to drop `pending_leases` before
`rsf_unity_frames_release`; now `rsf_unity_frames_release` collects every remaining lease, resets
the store and releases them outside its mutex. After a successful checked close there are none, so
this is a defensive cancellation in both versions.

## Intentional differences

- A leased slot stays reserved until its render event ends instead of being freed at take. One
  event runs at a time on Unity's render thread, so at most one of the 32 slots is held longer.
- AddRef runs under the store mutex and Release after it, instead of both under the bridge's
  `guard`. Take no longer locks `guard`.
- Retirement releases leftover leases after the store is reset rather than before. Nothing can
  observe the order because callback admission is closed.

## Verification scope

Source comparison covered `enqueue`, `render_event`, every `execute11` path's use of the packet
textures, the managed enqueue and event issue sites, `try_close_shared_bridge`,
`rsf_unity_package_bridge_finish_retirement`, the Mono D3D12 bridge's admission, take and release,
and the Vulkan packet route. The `rsf_unity_*` exports, `rsf_unity_native_api`, the managed
package, `vulkan_bridge.cpp` and the Mono bridge sources are unchanged. New unit tests cover leased
admission order, refused admission, take and double take, end and its no-op cases, slot reuse and
release cancellation. Formatting used rustfmt 1.11.0, since the pinned 1.94.1 toolchain is not
installed here. Locked offline Cargo metadata and Git whitespace checks also passed. Native and
Rust compilation and linking, Clippy, the unit tests, the D3D11 fixtures, and Unity Editor and
player runs on D3D11, Vulkan and Mono D3D12 need user verification.
