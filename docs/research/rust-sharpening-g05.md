# G05 sharpening port review

Baseline `68388daf`. The changes cover host contracts and control flow. The D3D11,
D3D12 and Vulkan shader kernels and their constant mapping retain their original
source and compiled payloads. The shader notices record the input for each
embedded payload and the shared depth include.

## Rust review findings

- **Pass the admitted sharpening device and slot through recording.** The Vulkan
  owner borrows one device and one slot after admission. Preparation, view
  creation, failure cleanup and recording share the same borrows. Device bind
  latching, slot limits, retirement and allocation order are preserved.
- **Report optional Vulkan sharpening failure from one owner.** DLSS, FFX, XeSS
  and FSR4 use the facade's reporting path. The facade owns the latch across
  clear and retries, including allocation failure before a Rust owner exists.
  The FFX and XeSS providers continue through their vendor output paths on
  refusal.
- **Keep sharpening activation policy out of the Vulkan owner's fake platform.**
  The fake explicitly supplies its callback result. The CPU fixture exercises
  the actual callback for absent settings, Off, Vendor, unknown mode, zero
  strength, fallback from NaN and signed MAS strength with zero base strength.
- **Generate the Vulkan sharpening callback and pass records from their native
  header.** bindgen generates all five Vulkan records with size, alignment and
  field offset assertions. Shared Vulkan image and SR records are imported. The
  D3D11 and D3D12 sharpening records also have generated bindings.

## Port file disposition

| Path | Source review |
| --- | --- |
| `runtime/orchestrator/src/sharpening_vulkan_core.h` | C declarations for the Rust owner and typed leaves, covered by generated bindings. |
| `runtime/graphics/src/sharpening/d3d12_platform.h` | C declarations for the Rust owner and typed leaves, covered by generated bindings. |
| `runtime/orchestrator/include/rescaleframe/sharpening_settings.h` | C record, ABI version, constants and header defaults. The C++ helpers forward validation, sanitization and vendor selection to Rust. Their in-tree consumers link the static Runtime through the core, the native Vulkan target or the Runtime test dependency. The loader and bridge use the header's C defaults. |
| `runtime/graphics/src/sharpening/rcas.h` | RCAS HLSL payload in a C++ string. |
| `runtime/graphics/src/sharpening/da_rcas.h` | Depth RCAS HLSL payload in C++ strings. |
| `runtime/graphics/src/sharpening/da_das.h` | Depth DAS HLSL payload in C++ strings. |
| `runtime/graphics/src/sharpening/depth_color.inc` | HLSL shared by the two depth-aware variants. |
| `runtime/graphics/src/sharpening/rcas_spirv.h` | Generated RCAS shader words. |
| `runtime/graphics/src/sharpening/da_rcas_spirv.h` | Generated depth RCAS shader words. |
| `runtime/graphics/src/sharpening/da_das_spirv.h` | Generated depth DAS shader words. |
| `runtime/graphics/src/sharpening/options.h` | Remaining GPL-3.0-only code. Native leaves use it to sanitize pass inputs and pack the 128-byte shader constant layout. A Rust replacement would require an independent implementation and shader parity validation. |
| `runtime/graphics/src/sharpening/d3d11.cpp` | Remaining native leaves for D3D11 shader compilation, pixel resources and immediate-context recording. Callers are the D3D11 pipeline and the WARP fixture. |
| `runtime/graphics/src/sharpening/d3d11.h` | Remaining C++ lifetime wrapper shared by `dlss_pipeline.cpp` and the WARP fixture. |
| `runtime/graphics/src/sharpening/d3d12.cpp` | Remaining native leaves for the root signature, PSOs, resource descriptions, barriers, dispatch and copy. Callers are `sr12_platform.cpp` and the D3D12 fixture. |
| `runtime/graphics/src/sharpening/d3d12.h` | Remaining C++ lifetime wrapper shared by `sr12_platform.cpp` and the D3D12 fixture. |
| `runtime/orchestrator/src/sharpening_vulkan.h` | Remaining native Vulkan descriptor, pipeline and barrier leaves, shader constant packing and facade. All four native SR providers and both Vulkan fixtures use it. |
| `runtime/orchestrator/src/sharpening_pass_options.h` | Remaining C++ field adapter from the public options record to the GPL native shader options, used by `dlss_pipeline.cpp`, `sr12_platform.cpp` and the Vulkan leaf. |
| `runtime/backends/common/sharpening.h` | Remaining native FFX dispatch-field adapter, used by `fsr_backend.cpp`, `native_sr_vulkan.cpp`, the FSR4 Vulkan bridge and the settings fixture. |

The D3D cutovers and Vulkan cutover had already moved the main resource owners
into Rust at the baseline. Native graphics leaves, the option adapters and the
fixtures still need work to close the G05 C++ port.

## Fixture disposition

| Path | Coverage and remaining work |
| --- | --- |
| `tests/engine_sharpening.cpp` | C++ dynamic-loading and settings contract across the Godot and Unity package DLL exports. Port still open. |
| `tests/sharpening.cpp` | C++ D3D11 WARP dispatch, readback, native options and GPL constant mapping. Port still open. |
| `tests/sharpening_d3d12.cpp` | C++ D3D12 command queue, descriptor and readback fixture, including concurrent slots. Port still open. |
| `tests/sharpening_settings.cpp` | C++ ABI, settings store, overlay and actual FFX descriptor fields when vendor headers are present. Port still open. |
| `tests/sharpening_vulkan.cpp` | C++ offscreen Vulkan recording, retirement and pixel readback. Port still open. |
| `tests/sharpening_vulkan_cpu.cpp` | Updated C++ fake Vulkan contract for the reporting facade and production option admission. Rust owner tests exercise slot cleanup and admission separately. Port still open. |

Verification was limited to source checks, formatting with Rust 1.94.1 and
bindings regenerated with bindgen 0.72.1. Comparing the GPL files to the baseline
showed the original shader and host constant-mapping content was preserved.
Build, test, Clippy and device validation are user-owned.

## Follow-up from `726c5e18`

- `tests/engine_sharpening.cpp` was replaced by `tests/engine_sharpening.rs`.
  CTest retains the Godot and Unity target gates, DLL paths and test name.
  Bindgen supplies the two engine settings layouts and the Vulkan SR frame
  assertions. The fixture still round-trips all five modes through the actual
  DLL exports and verifies that rejected requests preserve the settings.
- `runtime/graphics/src/sharpening/d3d12.cpp` was removed. The Rust host uses
  typed D3D12 interfaces to create the root signature, pipelines, per-slot
  descriptors and scratch texture, and to record and copy the result. The
  output transitions from UAV to shader-read to copy destination and back to
  UAV, and scratch returns to UAV. The engine still owns command submission
  and slot retirement. Shader compilation calls the Rust G06 compiler with
  the same source, entry, profile and flags. The GPL payload bridge only
  returns the original HLSL text and packs the original constant layout.
- `runtime/graphics/src/sharpening/d3d12_platform.h` is now a single-call C
  contract with generated Rust layout assertions. The C++ lifetime facade in
  `runtime/graphics/src/sharpening/d3d12.h` still serves the D3D12 host and
  fixture.

These conclusions come from source comparison, regenerated bindings and
pinned formatting. The C++ shader payloads and GPL options header were
compared against the follow-up baseline without differences. Compilation and
fixture execution remain pending.
