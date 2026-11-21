# Standalone VulkanStory NGX loader in Rust

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
