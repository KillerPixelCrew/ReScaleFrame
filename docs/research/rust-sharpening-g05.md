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
- `runtime/backends/common/sharpening.h` now declares the Rust FidelityFX
  dispatch normalizer. Both Vulkan dispatch adapters write directly into the
  SDK descriptor fields. Native D3D12 FFX calls the same Rust logic and the
  settings fixture still inspects the real SDK descriptor when available.

These conclusions come from source comparison, regenerated bindings and
pinned formatting. The C++ shader payloads and GPL options header were
compared against the follow-up baseline without differences. Compilation and
fixture execution remain pending.

## D3D11 and WARP follow-up from `4a531cda`

- `runtime/graphics/src/sharpening/d3d11.cpp` was removed. The Rust pass
  reuses the G04 compiler, constant buffer, view and compute dispatch helpers.
  It preserves immediate-context admission, full state save and restoration,
  explicit OM and CS UAV unbinding before SRV binding, and the scratch-to-output
  copy. The scratch texture and UAV still enter the cache together. The
  synthetic scratch regression retains refusal cleanup and same-key retry.
- `runtime/graphics/src/sharpening/d3d11.h` is now a private C contract. The
  DLSS pipeline supplies the same depth, near/far and motion facts and keeps
  the pass handle with the pipeline. Bindgen generates the frame-facts record.
  The derived GPL activation and constants remain in the native payload bridge.
- `tests/sharpening.cpp` was replaced by `tests/sharpening.rs` and the shared
  G04 C test entry. The existing CTest name, timeout and WARP skip code are
  preserved. The Rust fixture exercises the actual pass and the original GPL
  constant calculations, including the MAS ramp, all filter modes, the
  missing-depth fallback, disabled dispatches, HDR and SDR color, clamp
  behavior and alpha. CPU assertion failures are reported even when WARP is
  unavailable.

The independent commits replay in this order on `4a531cda`: `84aa8607`
(`d1580012`), `bce8a510` (`797fd9a2`), and `098bc0a3` (`e5e7b151`).
This keeps the engine fixture, D3D12 host and FidelityFX normalization
independently reviewable before the D3D11 slice.

Remaining G05 paths with implementation code are
`runtime/graphics/src/sharpening/d3d12.h`,
`runtime/graphics/src/sharpening/options.h` (GPL calculator),
`runtime/orchestrator/src/sharpening_pass_options.h`,
`runtime/orchestrator/src/sharpening_vulkan.h`,
`tests/sharpening_d3d12.cpp`, `tests/sharpening_settings.cpp`,
`tests/sharpening_vulkan.cpp` and `tests/sharpening_vulkan_cpu.cpp`.

## Follow-up from `82b39a63`

- `runtime/graphics/src/sharpening/d3d12.h` was removed. SR12's existing
  callback slots now call the Rust D3D12 host directly through
  `runtime/graphics/src/sharpening/d3d12_platform.h`. The GPL payload still
  owns both activation checks and the constant layout. The production
  options, command list, resource order and retirement contract are retained.
- `tests/sharpening_d3d12.cpp` was replaced by `tests/sharpening_d3d12.rs`.
  The existing CTest name and WARP skip behavior remain. It retains disabled
  dispatch checks, HDR and SDR output checks, depth fallback, three command
  slots recorded before submission, and D3D12 debug-layer message checks.
- `runtime/orchestrator/src/sharpening_vulkan.h` now declares C-only host
  state and functions. Rust uses typed Vulkan calls for all four providers
  and owns one fallback log per host, including initial allocation failure.
  The device binds through the same 31 entry points, writes the six
  descriptors and records the six barriers, compute dispatch and copy.
  The generated callback ABI also covers the new C host record.
- `runtime/orchestrator/src/sharpening_pass_options.h` was removed. The
  existing payload bridge marshals the C settings record into the licensed
  calculator. It exposes HLSL and SPIR-V from their original arrays and
  preserves the original activation and packing logic.
- `tests/sharpening_vulkan_cpu.cpp` was replaced by a Rust fixture that
  supplies the 31 typed Vulkan functions to the production host. It
  checks shader mode and entry point, view formats, descriptor and
  barrier order, constants, failure latching and resource retirement.
  The fake now keeps separate host memory per allocation.
- `tests/sharpening_vulkan.cpp` was replaced by an offscreen Rust
  Vulkan fixture. It loads the same System32 loader, selects a
  graphics/compute queue, preserves the upload, dispatch and readback
  sequence, and tests the same shader responses and three pre-submit
  slots. It retains the skip behavior for missing loader or devices.
- `tests/sharpening_settings.cpp` was replaced by a Rust fixture body.
  It retains the D3D11 setter, INI round-trip, owned and foreign keys,
  CRT decimal agreement, FFX descriptor and overlay intent checks.
  `tests/sharpening_settings_native.cpp` exposes the actual C++ overlay
  wrappers, SDK descriptor, appended setter and CRT formatter through a
  fixture-only table. The assertions and test data live in Rust.

The GPL HLSL, SPIR-V and `options.h` inputs match `82b39a63` byte for byte.
The calculator and adaptive ramp in `options.h` are upstream-derived
implementations. Moving them into MIT Rust would require an independently
specified replacement and parity evidence. The host ports call these
calculations through the existing GPL-3.0-only compilation boundary.

The HLSL headers `rcas.h`, `da_rcas.h`, `da_das.h` and `depth_color.inc`
and the three corresponding SPIR-V headers contain payload data and are
accounted for as such. `sharpening_settings.h` keeps the C ABI and its
header-only fallback defaults for targets without Runtime linkage. The
Runtime's validation and normalization are already in Rust.
`sharpening_vulkan_core.h` remains a C declaration file for generated
records and private Rust symbols.

Source verification used pinned Rust 1.94.1 formatting, reproducible
bindgen 0.72.1 regeneration of the Vulkan record, and
`cargo metadata --locked --offline`. The Vulkan comparison matched all
31 device lookups plus the instance memory-properties lookup with the
Rust fixture fakes. The CTest names and entry points were checked against
the source tree. The worktree retained the existing Cargo dependency
graph and the native Vulkan capability gates. No builds, test runs,
Clippy runs or device execution were performed for this follow-up.

The thin settings launcher now compiles as C11. It keeps the independent
panel, Runtime and SDK layout probes, the FidelityFX descriptor fields and
the CRT decimal comparison. Strict MinGW C11 syntax passed with and without
the pinned FidelityFX header. CTest execution and native linking are pending.
## Retained payload boundary at `fb27ab2f`

`PORT.md` has two open G05 implementation entries at this base. Both belong
to the existing GPL-3.0-only payload in `runtime/graphics/src/sharpening/`.

| Path | Current ownership and lineage |
| --- | --- |
| `options.h` | Licensed option activation and sanitization, motion-adaptive ramp and 128-byte constant mapping. The Git blob `2308f85eed1659d60991cff4ec2d8d302faa54f8` is identical to the blob introduced by `dee20e5869eca9598d154376613606156ef3fa10`. The header records OptiScaler `2fff542e78e257a1e4ff5a1e21a69a166c78c335` as its source. |
| `pass_payload.cpp` | The sole translation unit that includes the calculator and embedded shader arrays. Its blob `887202ba15ab227d693ade1a95f59c19f64485e9` was introduced in its current form at `094a8fc936ebe0828d45adec23ad52d7dde9a0c9`. The bridge began at `489369bffe6443a39ad1a485c0f238aa91e57da7` for D3D12, added D3D11 at `0a40bb30c2aa4612ea029e112bad8aefd81d1cb2`, and shared D3D marshalling at `296f75425adf8b5b7fae4929b10a74ee5f64dd7e` before adding SPIR-V access. |

The bridge returns borrowed HLSL strings and SPIR-V word ranges by filter
mode, marshals the public settings and frame facts, and invokes the original
calculator for admission, constants and the fixture's adaptive ramp. It
contains no graphics API calls. D3D11 owns its compute shader, bindings and
copy in `runtime/orchestrator/src/d3d11_sharpening.rs`. D3D12 owns its
descriptors, slot resources and commands in `d3d12_sharpening.rs`. The SR12
platform table now refers directly to that Rust module's private callbacks.
Vulkan's shared owner and typed API calls are in `vulkan_sharpening.rs` and
`vulkan_sharpening/native.rs`. The licensed functions they call provide
shader and calculation inputs only. Moving the existing calculator or
shader code into MIT sources would translate upstream-derived code.

The Windows branch of `runtime/orchestrator/build.rs` compiles just
`pass_payload.cpp` as C++17 into `rsf_sharpening_payload` through `cc`.
CMake consumes the resulting `rsf-orchestrator` static library through
`rsf_orchestrator_rust`. This is a source and compilation boundary within
the existing static link. The redistribution terms for linked binaries
remain those recorded in [the dependency notices](../dependencies.md).

The Git objects for `rcas.h` and `rcas_spirv.h` match `dee20e58`.
`da_rcas.h`, `da_das.h`, `depth_color.inc` and their two SPIR-V headers
match `d44a68463fc12df99b28579fbcb9dce3d1269b39`. The license text
matches `8fec1933638f22ed6b55e50d436678c828ca6f59`.
`eng/sharpening/spirv.py` is the recorded payload generation path.

The retained translation unit passed MSVC `/Zs /std:c++17 /W4 /WX` syntax
checking. Bindgen 0.72.1 reproduced the existing D3D11 and Vulkan private
records and the two SDK sharpening files byte for byte. Pinned Rust 1.94.1
formatting, locked offline Cargo metadata and the source caller and blob
checks also passed. No object files, binaries or runtime observations
were produced.
