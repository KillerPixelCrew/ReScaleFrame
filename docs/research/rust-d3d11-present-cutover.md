# Shared D3D11 presentation bridge Rust cutover

The baseline is `f693b02` on `rust/orchestrator-anomaly`. The question was whether the shared presentation bridge in `runtime/presentation/src/d3d11_present_bridge.cpp` could hand its orchestration to one Rust owner while the `IDXGISwapChain4` facade, the published `d3d11_present_bridge.h` contract, its export names and ordinals, and every DXGI, D3D12, D3D11, Win32, MinHook and SDK call stay native. This was a source port with synthetic and ABI checks. No device, window or game ran, and the bridge device fixture was built but not run.

| Boundary | Ownership after the port |
| --- | --- |
| Exports | `d3d11_present_bridge.cpp` keeps its 33 C entry points (`rsf_d3d11_present_*` and `rsf_d3d11_chain_dispatch_owned`). Each forwards to an unexported `rsf_core_d3d11p_*` function with the native leaf table. |
| Admission and publication | `d3d11_present.rs` validates and copies the setup (strings owned by Rust), admits creation requests (window callback, nested creation, single owner, sample count, format, buffer count), owns the scoped creation transaction (tokens, captured inner facade, outer alias, refusal), and publishes and unpublishes the owner, host, interop queue, graphics, session and pacing route. |
| Backend selection | Auto resolution against the allowed choices, the choice mask, request validation and the one backend-to-provider-family mapping (`rsf_core_fg_provider_family`). `rsf_fg_get_provider` in the orchestrator now uses the same mapping and table instead of its own copy, which the old bridge duplicated because it could not link the orchestrator. |
| Provider switch | Admission, the try-lock and deferred retry, drain, probe, teardown, host reload, creation, restore of the previous provider, plain fallback, fault, generation and switch result. |
| Frame and slot sequencing | The fence serial, three-slot ring, slot fences, application render index, application frame-latency fences and event, present count, post-switch receipts and the debug pacing trace. |
| Present, resize, retirement | Callback order around Present, sync interval normalization, tearing translation, failure reporting and fault, lazy physical creation and Off fallback, fullscreen, resize normalization and recreation, the retire lease and the facade's destruction (unpublish, drain, release or retain). |
| Status and latency | The copied provider status snapshot, the provider lease (`RwLock`, try-read for callers, try-write for replacement, blocking write for retirement), and begin, pace, marker, acquire, receipt, input and abort routing to the shared Streamline host or the provider. |
| Native leaves | `d3d11_present_platform.h` lists 77 leaves. Each is one existing native step on the facade: COM identity and reference counting, DXGI factory and chain calls, D3D12 queue, fence, list, resource and copy work, D3D11 fence and context work, shared surface and fence calls, Streamline host calls, `IsWindow`, `GetClientRect`, provider tables, the observer and the D3D12 info-queue report. Leaves never call back into the owner. |

Public layouts are hand-mirrored in `d3d11_present_abi.rs`; `d3d11_present_bridge.cpp` asserts the same sizes and offsets with `static_assert` and the Rust module asserts them in `const` blocks. The provider status and setup mirrors already existed in `native_fg_cpu_abi.rs` and `fg12_abi.rs` and are reused. No game or engine policy was added; the bridge still receives every engine fact from its setup.

## Linkage

The bridge object now names Rust symbols, so `rsf_presentation` links `rsf_orchestrator_rust` publicly. The Rust archive never names a presentation symbol: the leaves arrive as a table argument on every call. The dependency therefore runs in one direction, needs no link group on single-pass linkers and creates no archive cycle. Stand-alone Presentation consumers such as `rsf_shared_surface` still link; they pull no bridge object and therefore no Rust code. Every existing bridge consumer already linked the orchestrator core and the Rust archive.

## Defects found and fixed

Both are visible in the C++ source and were demonstrated with synthetic leaf tests that failed against the faithful port before the fix. Neither was observed in a game.

- Lazy physical creation published the provider session, pacing route and status only after entering fullscreen. When `SetFullscreenState(TRUE)` was refused for a non-windowed application, the call failed, the chain remained and later calls presented through it with no published session and a `NOT_READY` status until the next provider switch. FG tagging and provider-paced latency calls saw no session. Publication now happens before fullscreen entry. Test: `lazy_creation_publishes_before_fullscreen_entry`.
- Three early returns in Present (the engine-to-provider queue join, the provider-to-engine join after list submission, and a D3D11 frame without a provider queue) returned `DXGI_ERROR_DEVICE_REMOVED` after the application Present notification without its completion and without faulting the facade, so later Presents kept running. They now complete the notification, log the native step, fault the facade, and the post-submission join failure still signals the slot fence that covers the submitted list. Test: `early_present_failures_complete_the_notification_and_fault`.

## Other changed behavior

- The provider's `configure(Off)` now runs before its chain is queried for `IDXGISwapChain4` and published. The C++ owner queried first. The order only differs when that query fails, which then destroys the provider in both versions.
- A provider table with a missing entry returns `NOT_SUPPORTED` instead of calling a null pointer.
- Present's tearing check and the switch's fullscreen query use the locked `current_physical()` copy instead of reading the physical pointer unlocked.
- Sequencing fields are atomics. The C++ `faulted` flag was a plain `bool` written by `creation_end` refusal and read by Present, and `serial` was a plain counter also advanced by `drain()` from `SetFullscreenState` and `ResizeBuffers`. These are source-level data races; no failure was observed.
- A repeated prepare keeps the earlier string copies alive instead of overwriting their storage.
- Log lines are no longer cut at fixed buffer sizes. Pointer fields keep the 16-digit uppercase `%p` form.
- The copied status cache contract (invalidation, generation, concurrent readers, retired publisher) moved from `tests/d3d11_present_bridge.cpp` to `copied_status_contract` with its owner. The fixture keeps the public `status_get` refusals.

## Verification

- Worktree-local VS2026 configure (`build/wk18`, local `FETCHCONTENT_BASE_DIR`) with the pinned MinHook source and the vendor and reference headers of the main checkout read only. Debug and Release built every target, including the Runtime, the AC7, shared and thin `dinput8.dll` carriers, Minecraft, the C++ Anomaly adapter and the production-linked `rsf_d3d11_present_bridge_test` and `rsf_fg_present_bridge_test` fixtures, with no compiler or linker warning.
- Dumpbin export names and ordinals of the Runtime (381), AC7 `dinput8.dll` (256), Loader `dinput8.dll` / `ReScaleFrame.Loader.dll` (273) and `ReScaleFrame.Minecraft.dll` (267) match `build/rust-migration/baseline-exports` in both configurations. No `rsf_core_*` symbol is exported.
- Debug and Release passed 35 focused CPU and ABI CTest cases: the D3D11 present, FG, overlay and window DLL contracts (C++ and C11), `d3d11_host_contract`, `fg12_abi`, `sr12_abi`, `native_translucency_abi`, `render_frontend` and `callback_host_abi` with their embedded forms, `fg_choice`, `fg_session`, `fg_leases_faults`, `generation_state_concurrency`, `native_cpu_reservation_contract`, `native_fg_cpu_contract`, `input_receipts_contract`, `xell_sleep_concurrency_contract`, `gpu_sr_policy` and its embedded form, `orchestrator_domain_contract`, `plugin_session_lifetime`, `plugin_carrier_contract`, `shared_shim_exports`, `latency_input_event_contract` and `anomaly_fg_policy_contract`.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` passed (158 orchestrator tests, 17 new). This machine has no rustup, so the system Rust 1.100 ran instead of the 1.94.1 pin. The new tests drive the owner through a fake leaf table and provider: setup refusal, creation admission, publication and retirement, failed initialization, callback order and slot rotation, interval and tearing policy, native D3D12 frame sequencing and switching with restore, probe refusal and plain fallback, lazy Off fallback, resize, the retire lease, latency routing and lease refusal, scoped creation, retained retirement and the copied status cache under concurrent readers.
## Assumptions and remaining gaps

- The merge holds the provider read lease while `rsf_d3d11_present_queue` and `rsf_d3d11_present_command_list` look up the facade and invoke the native leaf. Both now return null while a replacement or retirement holds the write lease. A regression reproduced the unguarded lookup. The returned pointers retain their existing borrowed lifetime.
- The native12 path and the D3D11 path are exercised only through the fake leaf table. Queue joins, fence values, Streamline proxy unwrapping, info-queue reports and DXGI fullscreen behavior are unverified on a device in this branch.
- The scoped creation transaction is exercised synthetically. AC7, Fallout 4 and Skyrim scoped creation, Unity and RDR2 native D3D12 switching, and Anomaly D3D11 presentation were not run.
- `tests/d3d11_present_bridge.cpp`, `tests/fg_present_bridge.cpp`, `fg_switching` and `fg_provider` are device or SDK fixtures and were not run.



## D02 follow-up

Starting from `846e3a45`, including the staged application buffers from
`25744aba`, D02 traced the Runtime import table, host dispatch, the
Presentation static archive and the OpenGL resize caller. The application
buffers still stage before physical ResizeBuffers and publish only after
success. The OpenGL compositor retains its own preview and composition route.

| Source finding | Change and fixture |
| --- | --- |
| Reject nonfinite view-space scales consistently at presentation setup. | Host, legacy SR and presentation use the same finite-positive check before forwarding meters per view-space unit. The Rust setup fixture and the real DLL table fixture refuse positive infinity. |
| Preserve pending physical-chain creation across a host-load refusal. | A lazy owner claims creation before calling the Streamline host. A refusal restores pending, and later calls can create and publish the owner. Terminal physical creation failures fault the facade. The synthetic fixture retries with the same facade after the host becomes ready. |
| Read one provider status snapshot when publishing a presentation owner. | Initial publication, lazy creation and provider switching share one cached provider read for pacing and status. Initial logging uses that same record. Per-frame status refreshes remain in the Present path. The synthetic fixtures count provider reads at eager and lazy publication. |
| Keep a retryable physical Present result distinct from device loss. | D3D11 and native D3D12 return WAS_STILL_DRAWING without faulting when the slot fence was signaled. A fence signal failure remains terminal. A refused attempt completes its notification, and the native D3D12 logical render index advances only on success. The synthetic fixture covers both paths and the failed fence case. |

The shared surface production symbols now use the `windows` 0.62.2
D3D11, DXGI and D3D12 bindings. The vtable calls preserve HRESULTs and
raw outputs so the existing adoption code releases partial objects.
Texture usage, resource access flags, interface queries and the fence
initial value match the former native implementation. The shared-surface
device fixture still links the public C entry points, checks the opened resource's
description and observes the D3D11-to-D3D12 fence signal. Compile-time C++
checks retain the public record and result layout assertions.

Chain dispatch now admits the exact facade and maintains its live
reference in Rust. The guarded C++ calls address the private owned
methods directly because the public COM vtable may be patched by a
linked consumer. Release still consumes the caller's reference. The
synthetic dispatch fixture checks that reference and scoped refusal.
The Runtime query uses Rust for size, version and output copy. The
C++ export thunk supplies the original typed callback table from the
Runtime DLL. This keeps the Presentation dependency directed toward
the Rust archive and keeps the export spelling and caller-side types.

The `IDXGISwapChain4` facade, MinHook installation, native graphics and
provider callback leaves, and their platform declaration remain in C++.
The facade's owned methods are shared by the public virtual methods
and the linked Steam and host detours. A full move must preserve that
entry-point identity and the native exception guards. The shared FG
and D3D11 bridge device harnesses still cover real swapchain, provider,
overlay and AC7 proxy paths. The public API harness still checks the
separately linked Runtime DLL, typed slots and guard-page refusal.

Checked source signatures against the pinned Windows crate and generated
Windows x64 records with bindgen 0.72.1. Pinned Rust 1.94.1 formatting
and Git whitespace checks passed. The user owns native and Rust build,
test and lint execution, binary exports, real DXGI and vendor checks,
and the Minecraft GL preview and resize run.
