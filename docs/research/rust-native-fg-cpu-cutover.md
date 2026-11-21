# Rust Direct3D frame-generation CPU owner

The comparison starts at `d8f4e6a69b2b92edc06f13a1ed968965297ee154` on
`rust/orchestrator-anomaly`, after the D3D11 graphics cutover in `c9fec1f`.
The existing adapter events, D3D11 graphics transfers, D3D12 SR and shader inputs
provide the same boundary for this shared Direct3D CPU owner.

| Mechanism | Owner after the change |
| --- | --- |
| Actual CPU stages, input facts, duplicate sequence markers, reservations and actual predecessor Present outcomes | `native_cpu.rs` and its reservation ledger |
| Copied session/source/generation identity, RHI and submission spans, pacing attempts, routes and markers | `native_fg_cpu.rs` and its diagnostic module |
| FG eligibility, graphics leases, requested settings and published FG status | Existing Rust graphics and status owners |
| Process-wide Direct3D claim and pacer precedence | Existing `latency_lease.rs` |
| Anti-Lag 2 state and dispatch | Existing `latency_antilag.rs` with separate update serialization and shared driver leases |
| Win32 message pump, completion event, thread sampling, DXGI adapter identity and SDK calls | Native leaves in the CPU platform and presentation bridges |
| Receipt replay and unique candidate selection | Existing GPL helpers and their C++ bridges |

The source audit followed the CPU sink after the dispatcher released its lock.
Ordered legacy pacing still joins the previous Present with a bounded wait
that dispatches sent messages. Threaded pacing retains its overlap. A
prospective source waits for the actual predecessor result and claims one
pacing attempt before the engine enters its poll. Cancellation during the
wait or sleep leaves the in-flight callback responsible for retiring the
token. The later adoption and binding still come from the dispatcher.
Rendered-source mode supplies render timing and declines input pacing and
markers. Source times are QPC ticks with an explicit frequency. Source
intervals use consecutive native frame identities in one session.

The former marker call sampled the current provider generation even for a
source admitted by an earlier generation. The CPU owner now carries the
admitted generation into pacing and marker calls. The presentation bridge
rechecks it under its shared provider lease before calling the SDK. Short
render-pass structures refuse before an identity is read. RHI accounting also
checks the copied session. A receipt completed after a window binding was
replaced still reports to the selected provider and can no longer increment
the replacement binding's route totals.

The consumed AMD DX12 header shows that Update ends in the blocking
`UpdateAntiLagState(nullptr)` call. The older render-end marker reads the
initialized API pointer and sends an independent SDK operation. The native CPU
owner formerly serialized both behind one mutex. The Rust owner releases its
state and selection locks before Update and retains shared driver leases for
both calls. Initialization and shutdown take the exclusive lease. Reflex
already separates sleep from marker bookkeeping. Its status read now uses
published capability values and current counters while the sleeper owns its
options lock, so a Present-side status reader can continue. The status read
still refreshes capability values when it can take that lock.
Contended Reflex option changes return `NOT_READY` and retry through the
existing configuration paths. The presentation bridge similarly defers a
provider switch when a pacing lease is active. A queued newer switch request
keeps priority. Neither operation waits on the thread that may need to
finish the prior frame.

## Focused validation

Windows x64 MSVC 19.52.37029.0, Windows SDK 10.0.26100.0, CMake 4.4.4 and
locally installed Rust 1.100.0. The repository pins Rust 1.94.1. The baseline
and cutover used the same locally supplied Streamline, XeSS, FidelityFX,
Anti-Lag 2 and Vulkan headers. The corrected Vulkan Streamline runtime was
absent from the isolated workspace. The initial CMake download returned a
different yyjson source hash. The three pinned files were copied from the
adjacent `rust-native-fg11` workspace and CMake verified their hashes.

    $rsfPaths = @(
        '-DRSF_ANTILAG2_INCLUDE=D:/Coding/ReScaleFrame/vendor/antilag2'
        '-DRSF_FFX_INCLUDE=D:/Coding/ReScaleFrame/vendor/fidelityfx/Kits/FidelityFX/api/include'
        '-DRSF_FFX_VK_INCLUDE=D:/Coding/ReScaleFrame/references/godot/fidelityfx-vulkan/ffx-api/include'
        '-DRSF_RENDERDOC_INCLUDE=D:/Coding/ReScaleFrame/vendor/renderdoc'
        '-DRSF_STREAMLINE_INCLUDE=D:/Coding/ReScaleFrame/vendor/streamline/include'
        '-DRSF_VULKAN_INCLUDE=D:/Coding/ReScaleFrame/references/godot/engine/thirdparty/vulkan/include'
        '-DRSF_XESS_INCLUDE=D:/Coding/ReScaleFrame/vendor/xess/inc'
        '-DRSF_XESS_VK_INCLUDE=D:/Coding/ReScaleFrame/vendor/xess/inc'
        '-DRSF_UNITY_NATIVE_INCLUDE=D:/Coding/ReScaleFrame/vendor/unity-native/include'
    )
    cmake --preset windows-x64-vs18 @rsfPaths

    $baseline = @(
        'rsf_orchestrator', 'rsf_proxy_ac7', 'rsf_proxy_dinput8', 'rsf_game_anomaly'
        'rsf_native_cpu_reservation_test', 'rsf_input_receipts_test', 'rsf_latency_event_contract_test'
        'rsf_latency_antilag_test', 'rsf_anomaly_input_hooks_test', 'rsf_anomaly_fg_device_test'
        'rsf_fg12_abi_embedded_test'
    )
    cmake --build --preset windows-release-vs18 --parallel 6 --target @baseline rsf_native_latency_vulkan_test rsf_xell_sleep_concurrency
    ctest --preset windows-release-vs18 -R '^(native_cpu_reservation_contract|input_receipts_contract|latency_input_event_contract|latency_antilag2_contract|anomaly_input_(events|hooks)_(standard|avx)_contract|fg12_abi_embedded)$'
    cmake --build --preset windows-release-vs18 --parallel 4 --target rsf_xell_sleep_concurrency
    ctest --preset windows-release-vs18 -R '^xell_sleep_concurrency_contract$'

    $targets = $baseline + @(
        'rsf_native_fg_cpu_test', 'rsf_fg12_abi_test', 'rsf_xell_sleep_concurrency'
        'rsf_d3d11_present_api_test', 'rsf_d3d11_present_api_c11'
        'rsf_d3d11_fg_api_test', 'rsf_d3d11_fg_api_c11'
    )
    cmake --build --preset windows-release-vs18 --parallel 6 --target @targets
    cmake --build --preset windows-debug-vs18 --parallel 8 --target @targets
    $pattern = '^(native_fg_cpu_contract|native_cpu_reservation_contract|input_receipts_contract|latency_input_event_contract|latency_antilag2_contract|anomaly_input_(events|hooks)_(standard|avx)_contract|fg12_abi(_embedded)?|xell_sleep_concurrency_contract|d3d11_(present|fg)_api(_c11)?_contract)$'
    ctest --preset windows-release-vs18 -R $pattern
    ctest --preset windows-debug-vs18 -R $pattern
    ctest --preset windows-release-vs18 --repeat until-fail:20 -R '^(native_fg_cpu_contract|latency_antilag2_contract|xell_sleep_concurrency_contract)$'

    cargo fmt --all --check
    cargo test --workspace --locked
    cargo clippy --workspace --all-targets --locked -- -D warnings
    $env:BINDGEN = 'D:/Coding/ReScaleFrame/.local/rust-abi-tools/bin/bindgen.exe'
    $env:LIBCLANG_PATH = 'D:/Coding/ReScaleFrame/.local/rust-abi-tools/python/clang/native'
    python tools/generate-rust-bindings.py
    git diff --check

Ten baseline Release contracts passed. All 16 cutover contracts passed in
Debug and Release, along with all 118 Rust tests. The three Release concurrency
contracts each passed 20 runs. Both Anomaly profiles exercised the input-hook
and input-event fixture branches. The new CPU owner contract used the actual
dispatcher and receipt helpers with controlled provider callbacks. An older
Present delivered its marker while a newer source slept. The fixture also
checked duplicate pacing, actual predecessor results, cancellation during
sleep, stale generation refusal, receipt replay and rebinding, rendered-only
provenance, and session-scoped process claim release. The Anti-Lag fixture
passed with a driver double whose Update waits for an older render-end marker.
The XeLL fixture exercised the compiled provider with its SDK double.

The Release Vulkan latency fixture failed to link in both trees.
`cmake --build --preset windows-release-vs18 --parallel 4 --target rsf_native_latency_vulkan_test`
reported five duplicate `rsf_fgvk_*` symbols shared by its test stubs and a
Rust archive object. The D3D work leaves the fixture for Vulkan reconciliation.

The Release export comparison matched every ordinal/name pair in order.
It covered 381 runtime, 256 AC7 carrier and 273 loader exports. The import
check found a runtime DLL import in the DLL-linked D3D12 ABI fixture. Both
carriers and the embedded D3D12 ABI fixture link the core directly.

    $dumpbin = 'C:/Program Files/Microsoft Visual Studio/18/Community/VC/Tools/MSVC/14.52.37025/bin/Hostx64/x64/dumpbin.exe'
    $binaries = @(
        'build/windows-x64/bin/Release/ReScaleFrame.Runtime.dll'
        'build/windows-x64/ac7/bin/Release/dinput8.dll'
        'build/windows-x64/bin/Release/ReScaleFrame.Loader.dll'
    )
    foreach ($binary in $binaries) {
        & $dumpbin /nologo /exports $binary | ForEach-Object {
            if ($_ -match '^\s+(\d+)\s+[0-9A-F]+\s+[0-9A-F]{8}\s+(\S+)') {
                "$($Matches[1]) $($Matches[2])"
            }
        }
        & $dumpbin /nologo /imports $binary
    }
    & $dumpbin /nologo /imports build/windows-x64/bin/Release/rsf_fg12_abi_test.exe
    & $dumpbin /nologo /imports build/windows-x64/bin/Release/rsf_fg12_abi_embedded_test.exe

Bindgen 0.72.1 regenerated the two changed ABI files byte for byte.

These runs cover synthetic callbacks and ABI admission. The Streamline status
and configuration changes and switch deferral were compiled and reviewed
against their lock sites. Physical driver
sleep behavior, real backend switching, generated presentation, game
acceptance and measured input-to-display latency remain unverified.

## Render hitch diagnostic

The diagnostic now owns the watched interval, sample schedule, thread
handle and log formatting in Rust. One lock publishes the session,
source, submission and start time together. Only the matching close
clears that interval. A later render thread receives its own handle,
and the native latency session drain releases the matching handle.
The existing three-second screen gate remains at the submission
observer. The sampler keeps the 2 ms polling interval, the 15, 30 and
45 ms thresholds, and the 36-attempt process limit.

The C++ leaves duplicate a current-thread handle, take a bounded
snapshot of that thread and format one address from a loaded module.
The snapshot checks the committed readable stack range and resumes
the thread before the Rust owner looks up modules or logs anything.
If the watched interval changes during the snapshot, the owner drops
the stale sample. A failed thread launch can be retried on the next
eligible submission.

The Rust tests cover stale closes and both sampling limits. The
native CPU contract duplicates and samples its own test thread,
resolves the captured instruction address and joins the resumed
thread. It creates no graphics device. The seven focused MSVC
contracts passed in both Debug and Release. The CPU owner contract
passed 20 consecutive Release runs. All 295 locked Rust tests,
formatting and Clippy passed. Both configurations matched the recorded
Runtime, AC7, Loader and Minecraft export names and ordinals.
Game hitch traces require a separate run with diagnostics enabled.
