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
