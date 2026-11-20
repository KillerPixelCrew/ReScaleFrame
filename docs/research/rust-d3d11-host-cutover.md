# Generic D3D11 host Rust cutover

The baseline is `d8f4e6a` on `rust/orchestrator-anomaly`. The question was whether the generic D3D11 callback host in `d3d11_host.cpp` could move its session and reduced-frame admission, callback leases, recovery, presentation binding and lifecycle into one Rust owner while the published `d3d11_host.h` contract, its export names and ordinals, and every native D3D11, DXGI, SDK, overlay and capture call stay where they were. This was a source port with synthetic and ABI checks. No device, window or game ran.

| Boundary | Ownership after the port |
| --- | --- |
| Exports | `d3d11_host.cpp` keeps all 15 `rsf_d3d11_host_*` entry points as one-line forwards to `rsf_core_d3d11_host_*`. The host is DLL-only; the AC7 carrier and shared loader do not link it. The Rust functions are not exported. |
| Session and lifecycle | `d3d11_host.rs` validates setup and services, copies strings, owns the single process host, activation and its settings generation, installs the presentation bridge through a native leaf and publishes the host services. The upstream callbacks are Rust functions. |
| Callback leases | The former `presentation_guard` is a Rust mutex taken with `try_lock`, so a contended or reentrant callback refuses (`BUSY`, `NOT_READY` or no-op as before) instead of waiting. `creation_end` still waits because it must close its transaction. Settings, plan and overlay report sit behind a second lock that is never held across a native call. |
| Presentation binding | First scene lease or explicit binding configures the overlay panel, Present observation, capture output and latency window; the owner transfer before scene admission and the refusal after it are unchanged. Capability-derived panel choices are computed in Rust. |
| SR lifecycle and recovery | Pipeline start, resize, backend, quality, preset and sharpening decisions, history reset, evaluation and reporting run in Rust. The reduced-frame ticket (reserve, seal, temporal attempt, finish, retire, cancel and idempotent acknowledgement) runs in `d3d11_host_reduced.rs`, which drives the existing Rust auxiliary SR owner directly. Sampler bias calls the existing Rust sampler owner. |
| Native leaves | `d3d11_host_platform.h` lists 60 calls plus the auxiliary table. Each is one existing call: COM identity, device and chain queries, `d3d11_output` helpers, bridge install and continuation, native scene, window, FG and CPU owners, DLSS pipeline, frame dump, overlay, INI and the two capture budgets. Leaves contain C++ exceptions and return a failure value. |
| GPL helper | `failure_capture_policy.h` is GPL-3.0-only. It stays in C++ and is reached only through `failure_claim`, `sequence_request` and `sequence_claim`. It was not translated. |

The recorded capture names, descriptions and log lines are unchanged. Public layouts are hand-mirrored in `d3d11_host_abi.rs` because bindgen-cli was not installed; `d3d11_host.cpp` asserts the same sizes and offsets with `static_assert` and the Rust module asserts them in `const` blocks.

## Changed behavior

- A refused `RSF_D3D11_REDUCED_RESERVE` no longer changes the previous ticket. The C++ host copied the new owner, count and surfaces into the retired ticket before validating surfaces or reserving products. After such a refusal, a repeated `RETIRE` or `CANCEL` for the retired token was compared against the refused pass and returned `NOT_READY`. The new reservation is built aside and committed only on success. The `refused_reserve_keeps_previous_acknowledgement` test fails with the old ordering emulated and passes with the fix.
- Exceptions no longer unwind to a per-export `catch (...)`. Each leaf contains its own exception and returns its failure value, so the Rust owner records an ordinary refusal; for example an evaluation exception becomes `NOT_READY`, counts as refused and resets history. The reduced-frame entry formerly turned any exception into `BUSY`.
- A failed overlay configuration no longer writes partial settings into the host.
- A null settings pointer from the overlay and a null option array are ignored instead of dereferenced.
- CPU fact log lines are no longer cut at 320 bytes.

## Verification

- Baseline at `d8f4e6a`, VS2026 Release with the vendor headers from the main checkout: Runtime 354, AC7 `dinput8.dll` 256 and `ReScaleFrame.Loader.dll` 273 exports. After the port all three lists match by name and ordinal. The Runtime count is lower than the 381 in the tracker because this configuration lacks the optional Vulkan headers.
- Release and Debug built the Runtime, both carriers, the C++ Anomaly adapter, the D3D11 host, overlay, Present, FG and window contract clients, the auxiliary and sampler fixtures, and the shared-SR and rendered-FG D3D11 host device fixtures.
- Release and Debug passed `d3d11_host_contract`, `anomaly_input_events_standard_contract`, `anomaly_input_events_avx_contract`, `overlay_dll_api_contract`, `overlay_dll_api_c11_contract`, `overlay_dll_api_no_sr_contract`, `d3d11_present_api_contract`, `d3d11_fg_api_contract` and `native_window_contract`, as did the baseline.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` (128 tests, 10 new) passed. The new tests use fake COM identities, a fake leaf table and fake auxiliary products. They cover first-lease configuration, suspended and contended leases, the full reduced ticket, recovery admission of only the sealed owner, the RESERVE regression, owner transfer, refused early planning, failed evaluation reporting, overlay action bounds and report truncation.

Device execution, the D3D11 device fixtures, Skyrim, Fallout 4, Steam and Anomaly game runs remain pending.
