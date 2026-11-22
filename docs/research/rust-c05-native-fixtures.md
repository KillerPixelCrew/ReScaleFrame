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
