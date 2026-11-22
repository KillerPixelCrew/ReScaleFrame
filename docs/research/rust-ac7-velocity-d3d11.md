# AC7 D3D11 velocity replay and WARP fixture port

Base `fb27ab2f` contains the typed DXBC stages and shader-object cache. The
remaining native paths were traced from the carrier geometry observation to
capture, motion, history update, and final merge. The WARP fixture was traced
from device creation through its mapped readbacks. Earlier history contracts
and shader linkage findings are recorded separately.
[Shader and lease inventory](rust-ac7-velocity-a04-a05-residual.md)

| Source | Disposition |
| --- | --- |
| `games/ac7/src/velocity_pass.cpp` | Removed. `velocity_renderer/d3d11.rs` owns the D3D11 resources and commands. `velocity_renderer/replay.rs` supplies the carrier entries and serializes the same immediate-context draw, commit, frame retirement and release. |
| `velocity_device.h`, `velocity_device_abi.rs`, `velocity_commit.h`, `velocity_history.h`, `velocity_renderer.h`, `velocity_stream.h` | Removed with their C++ consumer. The shader device returns a typed COM variant lease and the existing history, stream and commit modules continue to supply decisions. Frame retirement returns Rust slot notifications after dropping the history lock. |
| `velocity_state_abi.rs` | Generated from the shared graphics header. The Rust guard borrows its storage through exactly one successful save and restore. Predication, SO cursor, UAV counters, all SRV ranges and shared nesting remain the shared owner's responsibility. |
| `tests/ac7_velocity_shader.cpp` | Keeps only the CTest executable entry and its Windows path conversion for an optional retained-DXBC directory. |
| `tests/ac7_velocity_shader_fixture.rs`, `tests/ac7_velocity_warp.rs` | Own the shader input bytes, WARP selection, resource creation, GPU replays, staging copies, Map and Unmap, and the assertions. The WARP fixture is enabled by CMake when `BUILD_TESTING` is on. |

Production capture preserves the direct, indexed, instanced, indexed-instanced,
DrawAuto, indexed-indirect and non-indexed-indirect routes. The indirect copy
uses the native argument offset and validates before the capture draw. The
history pass dispatches directly or from validated arguments at byte offset 36.
The native geometry and tessellation capture path clears history in b13 and
t127 before drawing. The motion draw restores ordinary hull evaluation and
binds the previous table, then records the view and queues the hash build.
Commit matches session, history, view, family, frame, depth identity, motion
extent/format and device. It clears all six 128-entry SRV ranges before copying
merged motion and depth back. The A02 camera and G02 production observer sources
are at the base revision.

The WARP replay retains the two opacity/depth material checks and 13-frame
producer sequence. This includes the rejected oversized arguments, indexed
and non-indexed indirect calls, native geometry deformation, 3- and 12-point
tessellation, camera resets, and reuse across identical material shader
objects. It also retains five sprite frames with the sparse indirect and
predication cases, two reordered mesh-particle frames with the previous
transforms at t127, and three frames for each beam, ribbon and animated-trail
route with both draw orders and the empty indirect allocation warmup. Shader,
layout and predication assertions query the actual context. Retained DXBC files
keep the previous artifact names, stage checks and history compiler count.

Source comparison against the base matched all 12 moved fixture shader
literals, the live HLSL and seven transform/codec sources, and D3D11 call order
for indirect validation, view depth initialization, capture and motion,
history dispatch, and merge. Pinned Rust formatting and syntax parsing,
MSVC fixture-entry syntax, targeted bindgen 0.72.1 regeneration, locked offline
Cargo metadata and whitespace checks passed. Native linking, emitted DXBC
comparison, WARP readback and game validation remain open.
