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

`tools/exposure_probe/src/lib.rs` owns the CLI inputs, adapter selection, frozen resources,
Streamline setup and teardown, 60 evaluations, colour transport, fidelity timing and raw dump.
`tools/exposure_probe/src/ngx.rs` owns the optional NGX hooks and forwards all original arguments.
`tools/exposure_probe/ngx.cpp` builds only with the NGX headers and retains the SDK's C++ virtual
parameter calls. The Rust probe entry supplies the CRT's C `main` symbol. The optional RenderDoc
1.6.0 API table and its typed calls stay in `tools/exposure_probe/renderdoc.c`. Capture starts before the sixtieth evaluation. Streamline
shuts down while the original and encoded resources remain alive. The probe is an explicit
hardware experiment outside CTest.

## Checks

- Rust 1.94.1 formatting passed for the changed packages. The workspace-wide check reports a
  pre-existing formatting difference in `games/fallout4/src/cpu_events.rs`.
- Bindgen 0.72.1 regenerated the scoped records. The first probe generation lacked the DLSS
  include root. Adding that root fixed generation. Unrelated generated import formatting was
  restored to the baseline.
- Locked offline Cargo metadata, C entry-point inventory, shader literal comparison, fixture
  wiring checks and `git diff --check` passed.

## Residual port on 3665aef0

The integrated base already contains the G03 typed D3D11 state, resource lifetime and fixture
owners, and the G05 sharpening owners. The G04 source-description and colour paths now call
the G03 typed SRV format helper directly. The exported C function delegates to that same match.
The colour transport fixture covers each alias and mapped format, typed passthrough, and
identical WARP encode results from typed and typeless FP32 allocations. The six CTest targets
retain their device selection, names, timeouts and skip codes. The eight public G04 headers
remain in use as C contracts.

The probe keeps its NVIDIA adapter selection and the original SDK feature gates. The NGX path
keeps the optional pre-exposure, exposure-scale and LDR overrides, the creation and evaluation
diagnostics, and the resource format report. The RenderDoc path keeps the wide library path
and the NVIDIA extension option before device creation. The linked DLSS and raw dump signatures
now come from their C headers through bindgen.

Rust 1.94.1 package formatting passed. Bindgen 0.72.1 reproduced the scoped output byte for
byte. Locked offline Cargo metadata resolved 111 packages using the existing user cache.
The alternate pinned-toolchain cache lacked the `ash` index entry. MSVC syntax-only checks
passed for the C11 launcher without SDKs, with RenderDoc, and with both SDK switches, and for
the optional NGX C++ leaf. Source comparison against `3665aef0` verified all six HLSL files,
eight C headers, all SRV format match arms and six CTest target declarations. Shader source,
SDK diagnostics and fixture wiring were reviewed. Linking, test execution, Clippy, and device
or game verification remain outside this check.

## Exposure probe seams on 19680cf1

The integrated base already includes the earlier probe port in `9f2e8500`. The follow-up
retires the C launcher and moves RenderDoc library selection, capture sequencing and status
reporting to Rust. The optional NGX leaf returns borrowed resources and reads and writes SDK
parameters. Rust owns the override decisions, D3D11 texture description and log fields.
Oversized NGX environment values are ignored because the Win32 buffer contents are undefined
when a value does not fit. The exposure log uses Rust's decimal formatting with the existing
field names and actual values. The previous binding allowlist named two cast macros that
bindgen omitted. The probe binding header now exposes both as C enum values derived from
the public DLSS header.

The CMake target remains a standalone native console executable with the CRT startup and the
same optional SDK header gates. The probe still selects the NVIDIA D3D11 adapter, uses the same
frozen input and resource setup, and starts capture before evaluation 60. The RenderDoc
library remains loaded until process exit. The SDK-free build links no-op capability seams.

Source comparison against `19680cf1` confirms byte-identical shader files, unchanged real
device/input construction and the original evaluation sequence. Rust 1.94.1 package
formatting, targeted bindgen 0.72.1 reproduction, locked offline Cargo metadata and MSVC
syntax-only checks for all four native feature combinations passed. Native linking, launch,
capture and evaluation remain unverified.
