# RDR2 capture ownership and diagnostics

R03 reviews the optional capture path at PORT.md's source revision `894a55f9` against the
renderer port recorded in `5c914b7f`. The question is whether the adapter preserves which
commands and resources a capture actually observed and whether it can publish or retire
the result without stalling Present. The changes use existing census callbacks and the
existing game guards. They require no new executable signatures, shader assumptions, or
native addresses.

## Capture path

The ASI registers census observations after the capture worker starts. D3D12/DXGI observation
provides the swapchain and backbuffers at Present, PSO and RTV facts at creation/bind, draw
identity at recording, resource states at barriers, and a recording generation at Reset.
NGX supplies the feature and evaluation list, temporal scalar values, and the color, depth,
motion, optional exposure and output resources. At queue submission, the Rust receipt claims
the exact recordings before the original call. A successful queue signal assigns the fence
that the poller needs to map the readback. The Rust worker detaches finished metadata and the
tail lease under the native mutex, writes artifacts after unlocking, and clears the dirty
ledger after publication for that sequence. The manifest is still the publication point.

The capture is a bounded probe for the observed RDR2 D3D12 path. The Windows SDK leaves keep
the typed COM detours, pipeline stream parsing, copies and readback mapping in
`games/rdr2/src/native_capture.cpp`. The ASI's trace shim supplies the immediate native caller
and stack to the Rust trace recorder. The WARP fixture still creates and presents a real
swapchain, drives graphics and NGX calls and checks raw bytes in `tests/rdr2_capture.cpp`.
Portable ownership regressions use the Rust recorder directly.

## R03 review findings

| Finding | Source evidence |
| --- | --- |
| Detach completed capture metadata before writing the RDR2 bundle. | `capture_state/{poll,bundle,shots}.rs` move draw, temporal and presentation data and the retained tail out of shared state, with matching-sequence finalization. |
| Keep the target identity of enabled RDR2 capture hooks. | `capture_state/hooks.rs` retains the installed target, logs a mismatch and publishes a process-observed role mask as `unobserved_hook_roles`. Bit positions follow `rsf_rdr2_capture_hook`. Multiple targets do not share an original trampoline. |
| Keep unavailable pipeline observations explicit in capture artifacts. | `capture_state/{recordings,artifacts,bundle,shots}.rs` and the native observation boundary preserve PSO identity and set `pipeline_known` in both JSON artifacts. CRC, blend and write-mask fields are omitted when unavailable. |
| Invalidate capture state after an aliasing barrier with no named destination. | `capture_state/recordings.rs` clears earlier local states and submitted facts at the wildcard boundary while preserving later explicit transitions. |
| Retire capture copies discarded by a command-list reset. | `capture_state/{recordings,shots}.rs` mark the abandoned unclaimed generation and release its readbacks on the poller. Claims, submitted work and untracked queue calls keep their leases. |
| Retire failed RDR2 readbacks when the fence reports device removal. | `native_capture.cpp` confirms removal on the readback's device. `capture_state/shots.rs` retires the shot without mapping. Both these retirements count toward `readbacks_unavailable`. |
| Apply one foreground F10 edge rule to both RDR2 capture workers. | `diagnostics.rs` and `capture_state.rs` apply the same rule to raw key samples. Request files remain independent. |
| Use the same bounded directory admission for RDR2 camera and GPU capture. | `diagnostics.rs`, `camera_probe.rs` and `capture_state/install.rs` share the reader, accepting 1–196 UTF-16 units. |
| Reuse the capture recorder's fallible map entry helper for swapchain metadata. | `capture_state.rs` supplies the helper for `capture_state/{metadata,recordings}.rs`, preserving updates to existing keys under allocation pressure. |
| Capture NGX scalar metadata as one observation. | `capture_state/metadata.rs` resolves the input once and uses a typed native reader for the scalar snapshot. The native reader preserves the signed fallback. |
| Report whether the frame-probe request worker actually started. | `diagnostics.rs` returns `-100` for a worker-start refusal after successful initialization. The existing ASI startup report reads that result. |

Source checks cover the private capture ABI, callback ownership and generated-binding inputs.
The checkout's Rust 1.94.1 formatter was used. Build, lint, unit tests, WARP runs and game
capture were left for the user. The previous Arc B370 report records timeouts in both
`rdr2_capture` fixtures with the existing five-second budget. This change keeps that budget.
State and pipeline observations are limited to the observed callbacks. A missing submission
receipt on a live device can still leave a lease pending when the adapter lacks a completion
proof.

## Follow-up from `806086f7`

- **RDR2 side of “Reuse one IEEE CRC-32 implementation for binary fingerprints.”**
  `capture_state/recordings.rs` uses the SDK checksum for shader bytes, preserving zero for
  an empty bytecode record and refusal for an invalid nonempty pointer or length. The ported
  `capture_state/pipeline.rs` uses the same hash for graphics descriptors and stream PSOs.
- `capture_state.rs`, `capture_state/recordings.rs`, `diagnostics.rs` and
  `native_platform.rs` use SDK `try_box` for nonzero-sized transferred
  owners. A failed allocation still returns null or refuses startup before publishing the
  owner. The ASI diagnostic wiring already present at the base is preserved.
- `capture_state/d3d12.rs` owns the D3D12/DXGI COM calls for queue fence creation and
  signalling, completion and device loss checks, swapchain buffers and Present admission,
  readback allocation, transition/copy/restore recording, and mapping/unmapping. The
  pinned `windows` bindings supply the SDK layouts. The copy uses subresource zero and
  retains the padded row pitch. A reused tail buffer must cover the new footprint.
  The Rust poller takes the retired queue and backbuffer owners by value and releases them
  after unlocking.
- `capture_state/pipeline.rs` owns the stream tags, pointer-aligned payload sizes,
  graphics descriptor facts and shader fingerprints. An unknown or truncated tag still
  leaves pipeline facts unavailable. The observation mutex protects publication of the
  copied facts.
- `capture_state/fixture.rs` checks manifest publication and the BGRA, RGB10, RG16F and
  depth bytes from the existing WARP runs. `tests/rdr2_capture.cpp` still drives real
  device creation, stream PSO construction, NGX fixture calls, draw/Present,
  teardown fence and debug layer validation.

The C++ capture observer and private headers remain for installation, native callback
registration and forwarding, raw NGX/camera observations, and immediate draw-caller and
stack capture. This slice does not close those files or the WARP driver as full ports.
Evidence is source comparison with `806086f7`, the locally pinned Windows bindings, and
Rust 1.94.1 formatting. The source audit matched 45 native capture entry names, all 25
existing pipeline stream tags, and the Windows SDK symbols used by the new leaves.
Locked offline Cargo metadata and Git whitespace checks passed. Build, lint, tests and
device captures were left for the user.

## Callback and fixture port from `82b39a63`

The remaining question was which native boundaries were required after the readback port.
The baseline `native_capture.cpp`, the loader census callback header, and the locally pinned
`windows` 0.62.2 SDK bindings supply the comparison. No executable or shader was captured
for this slice and the game addresses are the existing layout values.

| PORT.md path closed | Replacement and source evidence |
| --- | --- |
| `games/rdr2/src/native_capture.cpp` | `capture_state/{install,detours,ngx,poll}.rs` own registration, all 16 graphics detours, NGX and camera observations, queue receipts, and the worker. The owner is published before the worker and census callbacks can reach it. A failed thread start withdraws it before registration. |
| `games/rdr2/src/native_capture.h` | The private install table and fixture controls moved into Rust. The ASI's existing C entry name still binds capture to the loader census. |
| `games/rdr2/src/capture_state.h` | Observations now flow directly between the Rust owners. The detour definitions pin trampoline signatures and vtable offsets to the generated SDK records. The x64 draw entry gives the original return address to Rust, keeping the register and stack arguments used by both original draw methods. |
| `tests/rdr2_capture.cpp` | `capture_state/fixture/driver.rs` drives WARP and retains both CTest variants, the stream PSO, four NGX evaluations, 75 draws, post-submission Reset, the Present drain, artifact checks and D3D12 debug validation. It uses a separate `capture-fixture` archive so the shader compiler stays fixture-only. |

`capture_ngx.cpp` catches exceptions from the opaque NGX C++ getters before they reach
Rust and binds the existing census C functions. `tests/rdr2_capture_ngx.cpp` applies the
independent C++ NGX fixture declaration when setting test parameters and catches exceptions
there. `tests/rdr2_capture_main.c` supplies the CRT entry and linked service addresses.
The retained native functions contain no capture policy or device driver. The ASI settings
paths are unchanged. Bundle scalars remain finite JSON numbers and now use Rust's
round-trip decimal representation instead of the CRT's twelve significant digits.

Source checks matched the 16 role slots against the installed SDK vtables and compared the
fixture SDK names, draw and submission order, and CMake routing with the baseline. Rust 1.94.1
formatting, locked offline Cargo metadata with `capture-fixture`, and Git whitespace checks
passed. The fixture reuses the checked-in census options binding, so no generated declaration
or generator input changed. Compilation, lint, CTest, WARP and game validation remain
user-owned. Both capture variants previously exceeded the five-second capture budget on
the Arc B370, and the timeout budget is unchanged.

## Native NGX leaves moved to C11 (R03)

`games/rdr2/src/capture_ngx.c` and `tests/rdr2_capture_ngx.c` replace the two C++ files. The
ASI leaf dispatches the NGX `Get` virtual slot (8, 9, 11, 12 and 14, as before) through a
single function pointer type, because every overload takes a pointer to its value. It contains
the call with `__try/__except` instead of `catch (...)`, which also covers foreign C++
exceptions. The fixture object is still the C++ `NgxParam` table from
`tests/loader/src/windows/vendor_census_fixture/ngx_params.rs`. The C leaf reaches its `Set`
overloads through slots 0, 1, 3, 4 and 6, which are the MSVC reversed-overload positions of
`tests/vendor_census_ngx_params.h`. `vendor_census_native.cpp` still compiles that
declaration and so remains the compiler-derived check of the slot order. The fixture's
`DLSS.Feature.Create.Flags` value is an `int`, so it keeps the int slot.
