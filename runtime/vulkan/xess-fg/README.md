# XeSS frame-generation bridge

`VulkanStoryXessFg.dll`: drives XeSS-FG and XeLL through a DX12 presenter fed from the Vulkan renderer. Diagnostic environment keys use the `VULKANSTORY_XESS` prefix.
Moved from Optimum's `native/optimum-xess-fg` (revision `386e0d0`).

Build on Windows with x64 MinGW `gcc`, the repository's Rust toolchain with the
`x86_64-pc-windows-gnu` target, and `VULKAN_SDK` set.

```powershell
./runtime/vulkan/xess-fg/build.ps1 -SdkRoot <host>/sdk/xess -Output <dir>/VulkanStoryXessFg.dll
```

At runtime it loads `libxess_fg.dll` and `libxell.dll` from the same directory.

The integrated Runtime and standalone DLL use the same Rust session for
startup and checked retirement. The host stops presentation and drains Vulkan
before preparing retirement. Preparation drains DX12, flushes capture, and
destroys the SDK consumers before the host releases the imported resources.
Checked destruction then releases the native objects and SDK libraries. A
failed drain or teardown keeps the session and its remaining leases alive.

The session also owns the opt-in diagnostic capture (`capture.rs`): source
records, admission, budgets, duplicate refusal, the generated-callback counter
and the pending readbacks, published only after the drained DX12 fence.
`diagnostic-capture.h` declares the private SDK callback table and the Runtime
capture calls. `bridge.c` binds the SDK exports and keeps the Win32, COM and
DX12 readback, copy and map leaves.
