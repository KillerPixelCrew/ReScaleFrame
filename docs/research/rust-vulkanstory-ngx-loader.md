# Standalone VulkanStory NGX module in Rust

## Native caller port

The standalone library now builds as a Rust `cdylib`. Both packaging scripts
still publish the original filenames. `vulkanstory_ngx.h`, the managed imports,
and the Rust export declarations agree on all 29 symbol names. The private C
entry table is gone. Rust resolves the 12 typed vendor entries in one place and
keeps the successful module reference and a failed load diagnostic for the
process. The legacy Windows normal search and DriverStore order and Linux's
immediate, local symbol resolution remain.

The checked SDK source is Streamline 2.14.1 at
`2122257e0fce486f91b385aa63b9a09b0a34b363`. Its bundled
`nvsdk_ngx_defs.h` establishes the C convention and result values.
`nvsdk_ngx_params.h` shows eight virtual setters and eight virtual getters
without a virtual destructor. On Windows x64, `objdump -dr` of that checkout's
`nvsdk_ngx_d.lib` (SHA-256
`5b38e2de93f3bc4655ed722b352a64f33066689a3c6a366dcfa8bf0469c829ed`)
confirms the six used setter byte offsets `38, 30, 28, 20, 18, 00` and getter
offsets `78, 70, 68, 60, 58, 40`, in hexadecimal. These offsets match the
former shim's reverse overload order. Linux retains declaration order.
`nvsdk_ngx_vk.h` describes the SDK init wrapper and declares a one-argument
shutdown. The port retains the distinct driver init and two-argument shutdown
from the former shim at `abff6120`, including writable storage for the
remaining count observed on Linux driver 615.71.09.

Rust performs a volatile load of the SDK result after each result-producing
vendor dispatch. The setter calls return a separate result after their void
dispatches. This keeps a return address in the native module for NGX's caller
lookup. Verification still needs optimized binary export and call-site
inspection on Windows and Linux, followed by the actual driver run.

## Earlier loader-only port

Compared with `4ad9c5e9`, Rust now owns the shim's single load attempt, the
published symbol table and the terminal load diagnostic. The C leaves still
perform the OS calls. Windows tries `LoadLibraryA("nvngx.dll")` and then the
existing NVIDIA DriverStore enumeration. Linux clears `dlerror` before
`dlopen("libnvidia-ngx.so.1", RTLD_NOW | RTLD_LOCAL)`. The same 12 symbol names
are resolved in order. A missing symbol stays null. A failed load retains the
same bounded Windows numeric or Linux loader diagnostic. Reading the diagnostic
before the load finishes returns an empty static string.

The public header and managed imports match all 29 entry points at source
level. The native invocation and parameter bodies match after normalizing
their symbol table access. The C wrappers retain the Windows and Linux
parameter slots, argument checks, NGX caller frames and volatile returns.
Both scripts retain the GNU caller-frame flag. There is no link-time
dependency on NGX.

| Standalone route | Source-reviewed link |
| --- | --- |
| `runtime/vulkan/ngx/build.ps1` | Builds the locked release archive for `x86_64-pc-windows-gnu`. The existing GNU compiler route links it with the C shim into `VulkanStoryNgx.dll`. |
| `runtime/vulkan/ngx/build.sh` | Builds the locked release archive for native Linux and links it with the C shim and `libdl` into `libVulkanStoryNgx.so`. The GNU linker hides archive symbols and rejects unresolved references. The shell's Windows selection uses the Win64 GNU archive and DLL name. |

Both routes reuse a target strictly newer than the C source and header, Rust
source and manifest, workspace manifest and lockfile, toolchain pin and
its build script. Each builds in a temporary directory under the destination
and publishes only after a successful link. PowerShell reports failure by
throwing. Bash keeps its optional-build exit status and reports that any
previous library was retained.

The ordered platform table and wrapper comparison, PowerShell and Bash syntax
parsing, pinned Rust formatting and locked offline Cargo metadata passed.
Compile and link on both routes, binary export inspection, timestamp and
failed-build checks against real outputs, and NGX runtime verification remain
pending.
