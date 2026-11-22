# Rust colour, depth, motion and fullscreen passes

G04 moves the first-party D3D11 passes and their six fixture bodies to Rust. The comparison is
against `7e3f11a8`, the G06 shader compiler baseline. This is source and binding evidence.
Native linking, fixture execution, GPU readback and game behaviour require user verification.

## Passes and contracts

| Original implementation in `runtime/graphics/src/` | Rust owner in `runtime/orchestrator/src/` | Compared behaviour |
| --- | --- | --- |
| `colour_fidelity.cpp`, `colour_transport.cpp`, `colour_d3d11_platform.h` | `colour_d3d11.rs`, `graphics_d3d11.rs` | FP16 transport and typed target views, sampler filters and LOD, exposure and highlight inputs, resource ownership, compute binding restoration |
| `motion_decode.cpp`, `motion_resolve.cpp`, `motion_d3d11_platform.h`, `motion_decode_platform.h`, `motion_resolve_platform.h` | `motion_decode.rs`, `motion_resolve.rs`, `graphics_d3d11.rs` | Legacy parameter prefixes, validity and sentinel order, UAV formats, leases, binding order, and SDK input units |
| `depth_replay.cpp`, `depth_platform.h` | `depth_replay.rs`, `depth_replay/d3d11.rs` | Same-frame source, layer and context identity, sticky refusal reasons, four admitted draw kinds and the narrow depth state scope |
| `depth_scale.cpp`, `depth_scale_private.h` | `depth_scale.rs`, `depth_scale/d3d11.rs` | Packed D24/S8 snapshot, depth and eight stencil draws, allocation-only reserve, restore/discard, thread affinity, view restoration before resource retirement |
| `fullscreen_pass.cpp`, `present_blit.cpp`, `fullscreen_platform.h` | `fullscreen_pass.rs`, `fullscreen_pass/d3d11.rs` | Blend and transfer modes, point/linear filters, RTV mip dimensions, independent snapshot, swapchain buffer/view lifetimes |

The eight corresponding public headers under `runtime/graphics/include/rescaleframe/` retain
their C declarations. Source comparison found all 36 entry points in the Rust owners. Bindgen
0.72.1 generated the setup and draw records, the depth replay detail and the auxiliary SR ABI.
Existing generated records supply motion resolve parameters and depth scaling setup and frame.
The shared D3D11 state leaf still implements the same context calls. Cargo now includes it in the
orchestrator archive so consumers resolve one copy without a reverse graphics archive reference.

The six extracted HLSL files under `runtime/graphics/shaders/` match the original literals
byte for byte. The shader names, entry points and profiles still go through the G06 System32
compiler. Colour and depth scaling retain strictness. Motion and fullscreen retain flags zero.

## Review findings

- **Distinguish a missing optional depth layer from a rejected supplied layer.** A null layer
  still requests scene-depth-only reprojection. A supplied layer must pass device, format,
  extent, single-surface and SRV checks before dispatch. The D3D11 DLSS pipeline returns its
  existing resource failure and the legacy SR bridge returns its existing feature failure.
  Shared D3D12 evaluation still clears the D3D11-only pointer after normalization.
- **Move Unreal motion and jitter conventions out of the runtime backend crate.** Removed the
  unused `rsf-upscaler` crate after tracing workspace, native and fixture references. The live
  game adapter still provides its motion facts through the existing API. Historical notes retain
  their references to the earlier model. The duplicate `motion_convention.h` fixture now
  checks the production Rust DLSS sign conversion for raw, resolved and unspecified inputs.
- **Use the same rectangle and jitter predicates for auxiliary color and depth scaling.** Both
  owners use the same integer bounds and finite inclusive 16,384-pixel jitter limit. Auxiliary
  colour keeps its fractional source checks.
- **Auxiliary SR reaches the Rust depth-scale owner through two C layers.** Auxiliary SR holds
  its depth transaction directly. The intermediate checkpoint used one native platform pointer.
  The finished Rust leaf removes that pointer as well. Public standalone calls still enforce
  record size, ABI and thread ownership.
- **Centralize adoption of partially populated D3D11 resource arrays.** `ComRef::adopt_all`
  takes ownership of every populated slot and drops them all on incomplete output. A fake COM
  fixture covers a missing middle slot. Each constructor keeps its own result codes.

## Fixtures and probe

`tests/{colour_transport,depth_scale,exposure_normalization,fullscreen_pass,motion_decode,motion_resolve}.cpp`
were replaced by corresponding Rust files. CTest keeps the existing names, timeouts and skip
codes. Thin C launchers call the fixtures. The exposure fixture uses the real SR procedures
through a C callback table and retains the RGBA32F to R32F GPU readback, WARP fence and input
values. Motion resolve now exercises absent, wrong-format, undersized, foreign and unreadable
layers. The depth fixture keeps the packed 256-value stencil, alias, append counter, predicate,
viewport, wrong-thread and rollback checks. The fullscreen and decode fixtures keep their
hardware-first and WARP fallback behavior.

`tools/exposure-probe.cpp` now contains the optional NGX and RenderDoc SDK leaves, the
Streamline setup and the texture dump call. `tools/exposure_probe/src/lib.rs` owns the CLI
inputs, adapter selection, frozen resources, 60 evaluations, colour transport, fidelity timing
and output sequencing. The optional SDK leaves still use their real headers. Capture starts
before the sixtieth evaluation, and Streamline shuts down while its tagged resources remain alive.
The probe remains an explicit hardware experiment outside CTest.

## Checks

- Rust 1.94.1 formatting passed for the changed packages. The workspace-wide check reports a
  pre-existing formatting difference in `games/fallout4/src/cpu_events.rs`.
- Bindgen 0.72.1 regenerated the scoped records. The first probe generation lacked the DLSS
  include root. Adding that root fixed generation. Unrelated generated import formatting was
  restored to the baseline.
- Locked offline Cargo metadata, C entry-point inventory, shader literal comparison, fixture
  wiring checks and `git diff --check` passed.
