# C05 native fixture port

The baseline is `d8415c47`. The CTest targets retain their names, timeouts
and availability gates. Fixture code links the existing Runtime owners.

| Fixture | Source evidence | Native calls retained |
| --- | --- | --- |
| `native_fg_cpu_contract` | The Rust fixture copies the existing C04 platform table and overrides the same operations exercised by the C++ fixture. Checks cover independent Present progress, actual submission timing, receipt rebinding and deduplication, prospective cancellation and session or generation retirement. | Win32 thread duplication, sampling and address description use the existing Runtime callback exports. The executable has a C entry trampoline. |
| `native_antilag_vulkan_contract` | The typed fake Vulkan proc resolver checks the actual records passed to the driver boundary. It retains extension and chain validation, 64-bit source pairing, overlapping Present, cancellation and device retirement. The newer AMD extension structures use the Vulkan-Headers 1.4.357 field order and x64 offsets. | The executable links the existing C ABI and its optional Anti-Lag gate. The fixture opens no Vulkan loader or device. |
| `native_latency_vulkan_contract` | The fixture implements the six-slot synthetic Streamline token ring and fake XeLL endpoints in Rust. It retains the real Win32 message queue cases, provider-specific status and concurrent sleep/Present and shutdown cases. A refused checked start writes a nonzero handle and verifies the Runtime refuses to lease it. | The executable still links the existing native consumer and its provider callback exception boundary. The mock endpoints use the baseline's test-only symbol mappings. |
| `xell_sleep_concurrency_contract` | The Rust scenario checks that an earlier Present marker completes during a later sleep, and that configuration recovery restores the SDK's last accepted limit. The old fixture included `xess_generation.cpp` and referred to the removed `native_ops()` helper. The fixture now supplies the current 24-entry `rsf_xeg12_ops` table and the five-entry check table, including resolver and settings-drain event hooks. | The C++ shim supplies fake `IUnknown` objects, module references obtained from the Windows system library, and the typed callback table. It loads no vendor module. |

The Runtime presentation fixture header now declares the C entry points.
The C05 fixture returns borrows via Rust `Drop`. The existing C++ Vulkan
provider dispatch fixture includes the relocated test-only wrapper.

The D3D Anti-Lag leaf stays in C++. In the pinned SDK headers,
`ffx_antilag2_dx11.h` implements initialization, update and cleanup inline
over `IAmdDxExtAntiLagApi`. `ffx_antilag2_dx12.h` does the same over
`IAmdExtAntiLagApi` and also implements `MarkEndOfFrameRendering` inline.
The Runtime leaf calls these C++ functions and supplies the DXGI private
data record with the original SDK context address. The headers expose no
equivalent callable C ABI for replacing the leaf. The C04 Rust callback
facade and the SDK-facing Runtime implementation were preserved.

Verification used pinned Rust formatting, scoped bindgen regeneration,
locked offline Cargo metadata and source comparisons. Build, fixture
execution and device verification remain pending.

## Terminal replay

Base `9716068b` tracks three open C05 paths.

| Original path | Suggested PORT disposition | Source evidence |
| --- | --- | --- |
| `tests/xell_sleep_concurrency.cpp` | Ported and removed | `tests/game-sdk/src/xell.rs` builds the mock COM objects and calls the production XeFG provider through bindgen declarations from `generation12_platform.h`. The CTest name, SDK-header gate and timeout stay in place. The fixture checks real callback delivery while a later mock sleep is pending, configuration recovery, native queue identity on settings drain and balanced COM references at teardown. It takes only Windows system module references and creates an unnamed event. |
| `runtime/orchestrator/src/native_latency_vulkan.cpp` | Ported and removed, with explicit native ABI and exception leaves | `native_latency_vulkan.c` wires the existing public exports and optional providers. `vulkan_fg_wsi.rs` validates and forwards the original native queue and Present info, using Streamline synchronization when linked and the typed Vulkan proc otherwise. The instance resolver still accepts a null instance and stays callable after SDK shutdown. The separate C++ leaf catches exceptions at SDK startup and when invoking the borrowed host callback. |
| `runtime/orchestrator/src/latency_antilag.cpp` | Retain as a pinned SDK leaf | The Rust owner supplies the unavailable callback table. The C++ leaf keeps the inline SDK initialization, updates, end marker and teardown, plus the FSR swapchain private-data record. Its DX12 context address remains valid for the process because the record embeds that pointer. |

The Vulkan table still binds `rsf_vk_sl_begin_frame_checked`,
`VulkanStorySlTokenMatches` and `VulkanStorySlMarkerForToken`. The existing
Rust owner leases tokens only after a successful checked start with a
nonzero handle. It uses the marker callback result before recording success.
The FG endpoint table also retains its provider marker result and status
callbacks. The changes leave CPU fact admission and presentation ownership
with the existing Rust owners.

The installed Anti-Lag headers match both SHA-256 entries in
[the pinned SDK record](../dependencies.md#amd-antilag-2). The matching
DX11 and DX12 headers define the called operations inline over vendor
COM interfaces. The leaf uses the header-defined context types and retains
the existing FSR private-data GUID and payload. The source review did not
exercise an AMD device.

Verification used Rust 1.94.1 formatting, scoped bindgen 0.72.1
regeneration, locked offline Cargo metadata and source comparisons.
Build and fixture execution were outside this replay.

## Native leaf replay from fb27ab2f

The D3D Anti-Lag owner now reserves the Rust handle before allocating the
typed SDK storage. Rust selects the DX11 or DX12 leaf, interprets the raw
HRESULT, and writes the FSR presentation record to the borrowed swap chain.
The record keeps the pinned GUID, the original DX12 SDK context address,
the enabled byte at offset 8 and the 16-byte x64 size. The
`FrameInterpolationSwapchainDX12.cpp` consumer reads those fields before
Present. Rust calls the SDK end marker after a successful private-data
write and only when the original attachment condition holds. The update,
marker and rebind locks retain their previous order. The C++ leaf keeps
the typed allocation, inline vendor calls and typed cleanup, including
symbol resolution internal to those vendor headers.

Streamline initialization now uses the neutral
`rsf_vk_sl_initialize_features` export directly. The bridge enters the
Rust lifecycle, and its native `lifecycleInit` already catches exceptions
at `slInit`. The remaining C++ body in
`native_latency_streamline_exceptions.cpp` is necessary for borrowed
host callbacks that may enter throwing C++ code. Rust validates and
resolves the callback, user, SDK module and checked frame token within
the existing lease. The catch boundary invokes that resolved callback
and returns the existing feature-failure result if it throws.

Scoped bindgen regeneration and the source comparison covered the private
tables. MSVC syntax-only checks covered both optional-SDK variants and
the callback exception leaf. No link, fixture execution or device result
is claimed.
