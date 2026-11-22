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
