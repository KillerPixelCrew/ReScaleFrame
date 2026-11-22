# Unity D3D bridge submission and retirement

This updates the U03 baseline at `739be614`, including the U02 and U04
recording retirement producers and the C02 versioned callback host.
Source review followed the managed packet through both native bridge
entry points, Unity's D3D12 recording and flush events, the D3D11 upload
and copy-back, and Vulkan's upload and fenced hand-back. The final
D3D12 fence value and the D3D11 shared copy value are independent
observations. A successful D3D12 signal alone cannot retire a D3D11
output copy.

## Ported source

- `games/unity-mono/include/rescaleframe/unity_bridge.h` documents native
  texture identities and leases. The managed packet and callback
  table layouts and versions are unchanged.
- `games/unity-mono/src/bridge.cpp` and `bridge.h` use the Rust-owned
  packet lease, dispatch and receipt store. C++ retains typed Unity
  calls, allocator and command-list operations, the frame fence, the
  COM references that span GPU execution, and Win32 input observation.
- `games/unity-mono/src/bridge_state.h` and `bridge_state.rs` own the
  pending generation receipt, the D3D12 event scope, packet lifetimes,
  CPU stage facts and the queue-flush receipt delivery. The
  `rsf_unity_mono_drained_generation` symbol keeps the C02 signature and
  is now implemented in Rust. The C02 host and its versioned table
  are unchanged.
- `games/unity-mono/src/bridge_interop.h` borrows per-route Runtime handles
  and `bridge_interop.rs` supplies separate D3D11 and Vulkan instances.
  `runtime/graphics/submission` owns their fence sequence, slot rotation
  and retirement checks. The private C interface in
  `runtime/common/include/rescaleframe/submission_ring.h` exchanges
  opaque handles and scalars within the linked binary.
- `games/unity-mono/src/d3d11_pass.h` and `d3d11_pass.rs` describe and
  implement the pass and surface plan. `d3d11_state.h` and `d3d11_state.rs`
  describe and enforce checked replacement. `integrations/unity/native/bridge.cpp`
  resolves every compatible source before issuing any copy. COM,
  Direct3D, DXGI and Unity SDK calls remain typed native leaves.
- `integrations/unity/native/vulkan_bridge.cpp`, `vulkan_state.rs` and
  `preview.rs` consume the same Runtime mechanism. The U02 slot reset and
  destruction notifications and the U04 Vulkan recording serial, epoch
  and hand-back retirement proof remain at their existing producers.

## Fixed review findings

| Exact finding | Source and fixture evidence |
| --- | --- |
| Move the Unity interop submission ring and retirement mechanism into the Runtime. | `runtime/graphics/submission/src/lib.rs` provides acquisition, submission and drain status through the private C interface. Its Rust fixture checks that independent handles have independent counters. |
| Commit a Unity D3D12 reconfiguration only after the bridge delivers it. | `bridge_state.rs` returns a pending generation from admission. After native list setup, Rust checks the generation, exposes it during the host callback and settles only that generation after the callback returns. The fixture leaves admission unacknowledged and replaces its generation before retry. |
| Preserve each Unity view submission receipt until the queue flush. | `bridge_state.rs` stores session, source frame and view identities in recording order, deduplicates exact repeats and refuses work before recording when capacity is exhausted. Fixtures cover two views, frames eight apart and delayed or cross-session flushes. |
| Build the common Unity render-pass fields once across all three graphics routes. | `d3d11_pass.rs` assembles scalar identity, frame, camera, rects and motion fields once. Direct D3D12 binds packet resources and preserves the inputs-only FG probe behavior, covered by a Rust fixture. |
| Validate source compatibility before copying Unity motion or depth into a shared texture. | `d3d11_pass.rs` accepts only the RG16 float family for motion and R32 float family for depth. `integrations/unity/native/bridge.cpp` checks sample count and quality, mips and array size against the shared allocation. The fixture refuses RGBA16 float motion and a different depth family. |
| Require both graphics timelines to retire before replacing a Unity shared surface. | `d3d11_state.rs` reuses checked close's predicate for replacement. The fixture leaves the old surface owned with an unfenced or incomplete D3D11 copy. |
| Make the shared Unity completion check reject device removal. | `runtime/graphics/submission/src/lib.rs` distinguishes removed, pending and complete. The D3D11, Vulkan and Editor preview paths use the shared predicate. The fixture checks that removal cannot retire either timeline. |

## Verification

Targeted bindgen 0.72.1 regeneration reproduced `bridge_abi.rs`.
Rust 1.94.1 formatting, locked offline Cargo metadata and Git whitespace
checks passed. A source comparison checked both routes' recording reset
and retirement notifications, the C02 generation callback signature,
populate-before-copy ordering, and the unchanged managed ABI. Rust fixtures
cover the migrated state decisions. The existing native `unity_sr_d3d12`
fixture still exercises the Windows D3D12 device and provider calls.

Builds, tests, lint and device work are user-owned and pending. Live Unity
validation still needs managed reconfiguration, delayed window flushes,
D3D11 size changes and copy-back, Vulkan hand-back and device removal.
