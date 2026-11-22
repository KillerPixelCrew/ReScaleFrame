# AC7 velocity shader and replay ownership

The residual port starts at `031aa9ac`. The earlier A04 stage linkage and A05
history and variant admission are present in that tree. This pass traced every
remaining shader output from its DXBC writer to its D3D11 consumer, and the
replay lease from the scoped draw through commit and retirement. It used source
evidence from the existing AC7 velocity implementation and its fixtures.
[A04 stage inventory](rust-ac7-velocity-a04.md)
[A05 history and admission inventory](rust-ac7-velocity-pass-a05.md)

| Source | Disposition |
| --- | --- |
| `games/ac7/src/shader_bytes.h` | Removed. Velocity writes to Rust vectors. Contact-shadow correction builds the complete Rust-owned replacement, preserving the RHI prefix and engine trailer, and lends it to the synchronous guarded factory in `native_renderer.cpp`. |
| `games/ac7/src/velocity_container.h` | Already absent at the base. The encoder and borrowed semantics stay in `velocity_container.rs`. |
| `games/ac7/src/velocity_shader.cpp`, `velocity_shader.h` | Removed. Typed Rust stage transforms pass borrowed signatures and owned bytecode directly to the Rust device owner. |
| `tests/ac7_velocity_shader.cpp` | Keeps the WARP device and context setup, pipeline replay, resource bindings and readbacks. `tests/ac7_velocity_shader_fixture.rs` takes geometry and tessellation shader creation checks, pixel linkage for the GPU fixture, optional artifact output and retained DXBC validation, and the history GS compiler cache assertion. |
| `games/ac7/include/rescaleframe/ac7_velocity_pass.h` | Keeps the existing carrier and WARP declarations. Scope, log attachment and release resolve to Rust. Geometry, commit and frame end enter the Rust replay lease through native wrappers. |
| `games/ac7/src/velocity_commit.h` | Keeps native target and clock-sample inputs. Reporting and HLSL ownership are in `velocity_commit.rs`. |
| `games/ac7/src/velocity_history.h` | Keeps the history decision and retirement seams for D3D11 model and view targets. Model counts stay in Rust for reporting. |
| `games/ac7/src/velocity_pass.cpp` | Keeps native resource allocation, context and target identity checks, QPC timing, and D3D11 capture, motion, history and merge commands. Rust owns device shader objects, bytecode-keyed reuse, variant publication, scope facts, replay serialization, merge reentry suppression and reporting. |
| `games/ac7/src/velocity_pass_cache.h` | Removed. The existing Rust admission table hands the device owner typed slices into the retained shader and layout lease after dropping its cache lock. |
| `games/ac7/src/velocity_renderer.h` | Keeps the native create, destroy and synchronous submission seams. The existing Rust renderer owns publication, frame and release serialization in `velocity_renderer/replay.rs`. |
| `games/ac7/src/velocity_stream.h` | Keeps route, stream sizing, argument extent and cbuffer output declarations for native submission. Draw admission and indirect validation HLSL stay inside Rust. |
| `games/ac7/src/velocity_device.h`, `velocity_device_abi.rs` | Private bindings lend shader interfaces to the replay while its device owner is live. Shader COM references and DXBC cache keys are owned by `velocity_device.rs`. |

The writer replaces the same chunk indices in original DXBC table order, with
the same token and signature ordering and checksum inputs. The shared System32
compiler receives the same HLSL, names, entry point, profiles and strictness.
The A02 pass provides the exact view, family, history and frame keys to the
replay snapshot. The A03 capture ABI and the carrier's bytecode observation
fan-out remain at the base revision. Native replay retains the G02 borrower
scope, including saved predication and the high SRV slots restored after merge.

Source comparison matched all six standalone velocity HLSL files, all three
inline kernel payloads, the six moved fixture shader payloads, and the ordered
hexadecimal opcode and profile literals in the seven stage and codec files.
It also matched the ordered D3D11 context calls in the replay and merge bodies.
Pinned Rust 1.94.1 formatting, bindgen 0.72.1 regeneration, locked offline Cargo
metadata (111 packages) and `git diff --check` passed. Builds, test execution,
emitted DXBC comparison, WARP and game validation remain open.
