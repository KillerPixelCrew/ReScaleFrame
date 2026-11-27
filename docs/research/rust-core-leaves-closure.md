# C05, G02 and G03 native leaf closure

Four first-party C++ files remained after the C05, G02 and G03 ports. Each now
has no C++: one moved to Rust and three became C11 leaves.

| File | Disposition |
| --- | --- |
| `runtime/orchestrator/src/latency_antilag.cpp` | Removed. Its driver calls are `latency_antilag_sdk.rs`. |
| `runtime/orchestrator/src/native_latency_streamline_exceptions.cpp` | Now `.c`, SEH containment of the borrowed Streamline callback. |
| `runtime/orchestrator/src/frame_tap_callbacks.cpp` | Now `.c`, SEH containment of the frame-tap callbacks. |
| `runtime/orchestrator/src/native_composition_unload.cpp` | Now `.c`, a CRT initializer that registers the unload release. |

## Anti-Lag 2 driver calls in Rust

Question: can the D3D Anti-Lag leaf leave C++? The earlier note kept it because
`ffx_antilag2_dx11.h` and `ffx_antilag2_dx12.h` are inline C++ with no C ABI, so
a C leaf could not include them. The headers (MIT, revision `390aa4a8`, SHA-256
values in [dependencies](../dependencies.md#amd-anti-lag-2)) are small and only
call driver COM interfaces, so the same calls can be made from Rust.

How it was checked. A C++ probe compiled against the pinned headers with fake
driver interfaces and MSVC 14.52 recorded what the SDK sends:

- `Context` is 16 bytes (pointer, bool at 8, unsigned at 12). `APIData_v1` is 32
  bytes with the mode at 8, control string pointer at 16, its length at 24 and
  the frame limit at 28. `APIData_v2` is 176 bytes with the flags word at 8.
- The DX12 `IAmdExtAntiLagApi` calls `UpdateAntiLagState` through vtable slot 3
  and `Release` through slot 2. The DX11 `IAmdDxExtAntiLagApi` calls `Release`
  through slot 1 and `UpdateAntiLagStateDx11` through slot 3, after a virtual
  destructor in slot 2. The probe called those slots directly.
- `Update` pushes state only when the enabled flag or limit changes (mode 1 or
  2), then always makes a null update that inserts the delay and maps S_FALSE
  to S_OK. The DX11 push carries the control string
  `delag_next_osd_supported_in_dxxp = 1` (36 characters), the DX12 push none.
- `MarkEndOfFrameRendering` sends `APIData_v2` with version 2 and flags `0x20`.
  `SetFrameGenFrameType` (flags `0x0c` for an interpolated frame) is not used by
  the Runtime and was not ported.
- Initialization returns E_INVALIDARG for a missing context or device,
  E_HANDLE when the vendor module is not loaded and E_INVALIDARG when the module
  lacks the entry. The initial update disables Anti-Lag 2.

The Rust module keeps those values and the `Context` layout. The pinned FSR DX12
swap chain reads the DX12 context through DXGI private data, so the storage is
still leaked and its address still reaches the private record. Two failure
paths differ from the headers, both on error handling: a null interface after a
successful create call is a failure, and a failed DX11 create call clears the
request identifier the SDK leaves in the context, since cleanup would otherwise
call `Release` through it. A native build no longer needs `RSF_ANTILAG2_INCLUDE`
and the unavailable-SDK build variant is gone. A machine without the AMD module
reports the same unavailable result at bind time. The generated
`latency_antilag_native_abi.rs`, the `AntiLagNativeOps` table and the
`rsf_core_antilag_create_sdk` export are removed, and the platform table uses
`create_sdk_context` directly.

Rust tests run in an isolated crate because the tip does not build. They use
fake driver interfaces and assert the byte layouts above, update elision, the
end-of-frame marker, every refusal and cleanup path, release counts and zeroing
on shutdown. No AMD driver was loaded, so initialization against real AMD
hardware is untested.

## C11 exception leaves

Foreign C++ callbacks can throw, and an exception that unwinds into a Rust frame
is undefined. MSVC raises a C++ throw as structured exception `0xE06D7363`. A C
translation unit can contain it with `__try` and `__except`, the pattern the
census and RDR2 leaves already use. The filter handles only that code, as
`catch (...)` did, and leaves access violations and other faults alone.

- `native_latency_streamline_exceptions.c` returns the feature-failed result.
- `frame_tap_callbacks.c` contains the nine callbacks of the frame tap platform
  record and the constant observer. Reports are skipped, the gate and override
  report zero and the verdict reports leave. `build.rs` compiles it as C11 beside
  the remaining C++ callbacks of another chunk.

On a toolchain without MSVC structured exception handling, the C leaves call the
callback directly. The repository's other C leaves make the same choice, and the
only supported native toolchain is MSVC.

An MSVC C++ probe confirmed containment: throwing a `std::runtime_error`, an
`int` and a `std::logic_error` through the frame-tap gate, verdict, override,
report and constant-observer entries returned the decline values, and a normal
callback result passed through unchanged.

## CRT unload hook

The `.cpp` file held a static object whose destructor released the composition,
reinsert, exposure, regions and translucency owners at DLL or carrier unload.
The C file registers `atexit` from a `.CRT$XCU` initializer, which is what the
compiler emits for a C++ static destructor, so the CRT runs it at process exit
and at DLL detach. A probe linked against the object showed the five releases
running after `main`.

## Checks

- MSVC 14.52, `/W4 /WX /std:c11`, compiled the three C files with the runtime
  include paths.
- `cargo fmt --all -- --check` passes. This included reformatting one line of
  `d3d11_present.rs` that already failed on the tip.
- The Anti-Lag module's tests and `clippy -D warnings` pass in the isolated
  crate with `latency_antilag.rs`.
- The tip fails `cargo check -p rsf-orchestrator` with 48 errors outside these
  files and fails CMake configure at `tests/CMakeLists.txt:448`, so no target
  build or CTest ran. `libclang` is not installed, so the binding generator did
  not run. The removed generator entry only produced the deleted file, and the
  other Anti-Lag binding does not mention the removed type.
