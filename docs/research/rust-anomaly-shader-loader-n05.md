# Anomaly shader loading port

N05 follows the shader loader on `d12e6884` and the Rust compiler and Runtime
pack owner already on that base. It reuses the existing engine and shader
evidence. [Shader loader evidence](rust-anomaly-shader-loader.md),
[AVX profile](anomaly-avx-profile.md),
[Runtime patching gate](runtime-shader-patching-gate.md)

| Area | Source evidence and result |
| --- | --- |
| Family IDs | `shader_family.rs` assigns explicit IDs to the same eight families, preserving their C ABI values. Admission matches name and stage and returns the declared ID. Draws import those constants. Counters index by the ID and are sized from the table. The zero bucket prints `other` in reports and retains `none` in the family-name ABI. |
| Reflection | The loaded system `d3dcompiler_47` export returns an `ID3D11ShaderReflection`. Rust reads the version, all input/output/patch signature rows, resource bindings, and each buffer and variable in native order. One callback copies the complete description to current C++ consumers while the COM reflector and row storage are live. A failed descriptor read returns failure before publication. Compatibility and linkage continue to consume copied or borrowed snapshots under the existing synchronous contract. Both sides assert the private x64 layout. |
| Deployed-tree compile | The Rust tree owner still reproduces the stock DXBC before admitting a fresh variant. The fixture compiler now calls the existing `rsf-shader-compiler` owner with the selected export, empty source name, original flags and ordered macros. The Rust include vtable preserves `r3` before shared lookup, and the native path leaf still applies the same Windows narrow-path conversion and append rules. |
| Live compile | Anomaly still opens the already-loaded Runtime service and passes the engine's loaded compiler export. Rust checks the RIP-relative LEA at setup RVA plus six (`0x83b9ce` Standard and `0x832cce` AVX are the setup RVAs), signed address arithmetic, and both guarded includer entry prefixes before opening the pack. Native SEH wrappers still guard reads and the engine's include Open and Close calls. Runtime owns proof, cache, and variant creation. |
| Shader asset | `tools/anomaly_shader_constants.hlsli` retains identical HLSL bytes. Both the staging tool and its fixture use the new source filename. |

The standalone temporal fixture now lives in the Anomaly Cargo example
`anomaly_temporal_shader_device`. The Python verifier builds the locked
example under its existing local evidence directory and retains the TSV
case list and JSON report fields. The example uses the G06 compiler for
fixture VS/CS HLSL and reads the same FXC-built stock and patched DXBC.
The port keeps hardware with the debug layer, WARP fallback, VS stream
output, reflected b13 constants, the native MRT formats, per-sample MSAA
readback, and the current/prior clip, invalid history, alpha, stationary
motion, and jitter checks. The verifier now checks the pixel count against
the same alpha modes used to generate the case list. This is a source
comparison. The built binary and device results remain unverified.

This checkpoint has source and private ABI review, pinned 1.94.1 rustfmt,
and whitespace checks. A Rust regression fixture covers refusal on an
unreadable signature row. Builds, execution of that fixture, lint, device
checks, and Standard/AVX gameplay remain user-owned. No shaders, game
fingerprints, executable sites, or graphics-stage policy were changed.

## Live loader follow-up

The live Rust stage owns the pack box, checks the Runtime result and both
shader reflections, derives foliage constant offsets from stock reflection,
then creates and retains the variant. On success the published variant owns
only its reflection and shader/device leases. The Runtime's compile bytes are
copied within the prepare lease and released after shader creation. If create
returns an object with a failure result, the local lease releases it. Registry
linkage uses the retained Rust snapshots and still keys its cache by all bound
stock stages. Pack reload still prepares each replacement before publishing
the next generation.

Each snapshot owns the copied descriptor tables and reflected names while
preserving enumeration order. It can outlive both the COM reflector and the
pack whose compiler generated the variant. Both the live and native fixture
paths refuse an unreadable row using the same reflection owner.

The reconciled private motion platform changed from N04 ABI 10 to 11
and from 352 to 280 bytes. C++ and Rust assert the table size and the remaining shader callback
offsets. The adapter's existing Runtime shader-patch ABI stays at version 1.
The Standard/AVX profile still supplies the includer setup RVA and the
`d3dcompiler_43.dll` name. `engine_shader_includes.cpp` retains just that
profile handoff and the two SEH-protected calls into the engine vtable.

The live game target and SR hooks fixture no longer link
`shader_d3d.cpp`, `shader_loader.cpp`, or `motion_foliage.cpp`. The C++
motion contract and three deployed-tree device fixtures still use their
fixture facades for fake reflectors, preparation callbacks, and native
fixture inputs. These uses remain in `shader_d3d.cpp/.h`,
`shader_loader.cpp/.h`, and `shader_loader_platform.h`. The live caller-frame
layout declaration is now in
`shader_frame_layout.h`. Converting the remaining fixture facades would
also require changing those C++ motion fixtures.
