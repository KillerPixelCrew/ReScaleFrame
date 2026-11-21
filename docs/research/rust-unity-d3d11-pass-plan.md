# Unity D3D11 pass plan, route and retirement verdict

The source audit starts at `1a87cbca`, after the [packet lease audit](rust-unity-d3d11-packet-leases.md).
The question was which decisions in the package D3D11 bridge's `execute11` could move into Rust
without changing the typed D3D11 and D3D12 calls, their order, the wait and signal sequence, the
packet leases, callback admission, the managed or native public ABI, or the Vulkan and Mono D3D12
routes.

## What moved

`games/unity-mono/src/d3d11_pass.rs` (boundary `d3d11_pass.h`) now owns:

- The route of a render event, from its offset in the reserved event range and whether the package
  runs in the Editor: reconstruction (offset 0), the window pass (offset 1), the Editor preview
  window (offset 1 in the Editor with a packet colour) and the HUD-less scene (offset 2).
- The assembled host render pass, which was the C++ `make_pass`, with the route's role (SR, WINDOW
  or FINAL_SCENE) and the preview's `RSF_UNITY_PREVIEW_SOURCE_COMMON` flag.
- The surface plan: the shared surface slot of each input (0 to 3 for reconstruction, 4 for every
  HUD-less input, 5 for the preview colour), the D3D12 state the pass uses each one in
  (NON_PIXEL_SHADER_RESOURCE for reconstruction inputs, COPY_DEST for its output, RENDER_TARGET for
  HUD-less), whether the HUD-less colour comes from the swapchain, and whether the output is copied
  back.
- The shared surface format for a slot and its source format (R32_FLOAT depth, R16G16_FLOAT
  motion, typeless and sRGB colour to the linear typed format). `rsf_unity_d3d11_prepare_surface`
  now takes only the source format and chooses the shared one itself.
- The policy change gate (`rsf_unity_d3d11_apply_change`): on a reconstruction, a pending change is
  read, the last completion value handed out is waited for through the record's wait leaf, the
  change is settled and the pass is marked `RSF_GAME_RENDER_RECONFIGURE` with an invalid camera.
- The retirement verdict (`rsf_unity_d3d11_retire`) of a submission whose queue work went out:
  retired, hand-back failed or completion signal failed, stopping the adapter on either failure.

`integrations/unity/native/bridge.cpp` keeps every typed call and its position: `GetSwapChain`, the
packet textures' `QueryInterface`, `GetBuffer`, `GetDesc`, the `rsf_shared_surface` calls,
`CopyResource`, the shared fence signals, flushes and waits, the barriers (now from the planned
states), allocator and list resets, `Close`, `ExecuteCommandLists`, the completion signal, the output
copy and its tail signal, and the render-pass calls. It fills the plan's pass with the shared
surfaces and the swapchain and logs the verdicts. `bridge.cpp` asserts the plan layout, the D3D12
state and DXGI format values and the preview flag against the native definitions.

Vulkan (`vulkan_bridge.cpp`) and the Mono D3D12 bridge (`games/unity-mono/src/bridge.cpp`) keep their
own `make_pass` and are unchanged.

## Trace

Packet leases. `render_event` enters the frame callback, admits ids in the reserved range and takes
the packet. A D3D11 packet's store entry holds one reference to each non-null colour, depth, motion
and output texture until `TakenPacket` ends the packet when `render_event` returns. Every path of
`execute11` runs inside that span: the `QueryInterface` copies take their own references, the
uploads and the output copy run on Unity's immediate context while the store still holds the
textures, and the context's own references cover the copies after return. The plan reads only the
packet copy; it neither retains nor releases.

Window. `ReScaleFrameRuntime.PresentCompletedFrame` issues the window packet
(`RSF_UNITY_PACKET_PROBE | RSF_UNITY_PACKET_WINDOW`) on offset 1 with every resource cleared, except
that in the Editor the colour is its composed Game-view copy (an R8G8B8A8_UNorm render texture).
Outside the Editor the plan is WINDOW: `pass.swapchain` is Unity's swapchain and
`render_pass(queue, pass, 1)` reaches package.cpp's `render`, which requests generation and calls
`rsf_fg12_window`. No surface, fence or ring slot is used. An Editor packet without a colour also
plans WINDOW, as the C++ fell through to the same call.

Editor preview. In the Editor with a packet colour the plan is PREVIEW. Under the surface lease the
colour is copied into surface 5, then the sequence is unchanged: `record_d3d11_copy`, `CopyResource`,
`shared_pair(uploaded, consumed)`, D3D11 `Signal(uploaded)` and `Flush`, `d3d11_submitted`, D3D12
`Wait(uploaded)`, `render_pass(queue, pass, 1)` (package.cpp presents it through
`rsf_unity_preview_present` with the COMMON state flag), D3D12 `Signal(consumed)` and D3D11
`Wait(consumed)` as the hand-back, then the completion signal for `rsf_unity_interop_completion()`
and the verdict. There is no begin-0 call for the preview.

Reconstruction (SR). Under the surface lease: ring slot, its retirement wait (logged on timeout),
allocator and list reset, the four `QueryInterface` and copy steps into surfaces 0 to 3 in packet
order, `shared_pair(upload, download)`, D3D11 `Signal(upload)` and `Flush`, D3D12 `Wait(upload)`,
barriers from COMMON to the planned states, the change gate, `render_pass(list, pass, 1)` (package
SR evaluation, preparing the Editor preview surface first in the Editor), barriers back to COMMON,
`Close`, `ExecuteCommandLists`, D3D12 `Signal(download)` and D3D11 `Wait(download)` as the hand-back,
the output copy from surface 3 into the packet output and its tail signal when the hand-back
succeeded and the plan copies back, the completion signal for `rsf_unity_interop_submit(slot)`, the
verdict, `render_pass(list, pass, 0)` (package.cpp reports `rsf_fg12_submitted`) and `advance`.

HUD-less (FG). The package renderer feature issues `RSF_UNITY_PACKET_PROBE | RSF_UNITY_PACKET_HUDLESS`
on offset 2 with depth, motion and output cleared and the colour from the render target, or null
for an imported back buffer. The plan takes the colour from swapchain buffer 0 when it is null, puts
every present input in surface 4 as a render target and sets FINAL_SCENE, which package.cpp routes
to `rsf_fg12_hudless`. The change gate does not apply and nothing is copied back. The rest of the
sequence is the reconstruction's. A probe packet's camera is invalid in both versions.

Completion and partial hand-back. Once `ExecuteCommandLists` ran, the completion fence is signalled
whether or not the hand-back succeeded, so release and the checked close still wait for the queue
work. A failed hand-back or a failed completion signal stops the adapter and skips the begin-0 call
and `advance`, as before. A refused output-copy tail signal stops the adapter in bridge.cpp, still
before the completion signal. Every return before `ExecuteCommandLists` leaves nothing submitted:
`Recording` closes the list and the slot is not advanced.

## Intentional differences

- `rsf_unity_package_is_editor()` is read for every D3D11 event instead of only window events. It
  is a plain getter of the package's mode.
- The change gate's wait uses the published record's view and wait leaf instead of the local
  `DeviceView`. Under the frame callback they are the same handles and the same native `wait`.
- `rsf_unity_frames_activate(0)` for a failed retirement runs in Rust before the native log, the
  C++ order.
- `rsf_unity_interop_submitted` in `bridge_interop.rs` is now `pub(crate)`. `d3d11_state.rs` already
  imported it at `1a87cbca` while it was private, which Rust refuses (E0603), so that tree likely did
  not compile; the export itself is unchanged. `rsf_unity_frames_activate`,
  `rsf_unity_frames_pending_change` and `rsf_unity_frames_settle_change` are `pub(crate)` for this
  module, also without export changes.

## Remaining native decisions

The ring slot's retirement wait and its log, the output-copy tail stop, the choice between the
Vulkan and D3D11 routes in `render_event` and the per-input `QueryInterface` loop stay in
bridge.cpp. The Vulkan and Mono D3D12 passes keep their own `make_pass`.

## Verification scope

Source comparison covered `execute11`, `make_pass`, `prepare_surface`, `retire_interop_submission`,
`render_event`, package.cpp's `render`, `rsf_unity_package_render`, the managed issue sites of the
three events (`ReScaleFrameUpscaler.cs`, `ReScaleFrameRuntime.cs`, `ReScaleFrameRendererFeature.cs`)
and the Vulkan and Mono D3D12 `make_pass`. `rsf_unity_*` exports, `rsf_unity_native_api`, the managed
package, `vulkan_bridge.cpp` and the Mono bridge sources are unchanged. New unit tests cover the
base pass, each route's plan, out-of-range offsets, the shared formats, the change gate's order and
refusal, and the retirement verdicts. Formatting used rustfmt 1.11.0, since the pinned 1.94.1
toolchain is not installed here. Locked offline Cargo metadata and Git whitespace checks also passed.
Native and Rust compilation and linking, Clippy, the unit tests, the D3D11 fixtures, and Unity Editor
and player runs on D3D11, Vulkan and Mono D3D12 need user verification.
