# XeSS frame generation through Vulkan/DX12 interop

## Question and scope

Can the direct Godot Vulkan integration offer XeSS frame generation through an
orchestrator-owned presenter? This requires checking the actual Intel interface,
then testing resource sharing on the selected adapter. Source inspection alone
does not establish generated-image correctness or presentation ownership.

## SDK interface audit

The installed Intel XeSS repository is release `v3.0.2`, revision
`8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0`. The official latest release checked on
2026-10-25 is also [v3.0.2](https://github.com/intel/xess/releases/tag/v3.0.2),
published 2026-07-24. A duplicate SDK download was unnecessary.

The [Intel FG integration guide](https://github.com/intel/xess/blob/v3.0.2/doc/xess_fg_developer_guide_english.md)
requires DirectX 12. The public FG headers are `xefg_swapchain.h`,
`xefg_swapchain_d3d12.h` and `xefg_swapchain_debug.h`; XeLL has `xell.h` and
`xell_d3d12.h`. The current upstream header inventory agrees. XeSS SR's Vulkan
interface does not imply a Vulkan FG interface.

The installed `libxess_fg.dll` is version `1.3.1.78`, size 22,957,432 bytes,
SHA-256 `EC5E0C65E075570C6EDE72618BB666D0BE0C2E10B2EA9762C0FE8CB8E375AB27`.
MSVC `dumpbin /exports` reports 31 named exports and 26 ordinal-only exports.
Named context creation, resource tagging and swapchain initialization functions
are D3D12 functions. No public Vulkan FG creation/resource/WSI API exists in this
package. Ordinal-only functions do not establish a supported private Vulkan API.
`libxell.dll` SHA-256 is
`D2030DCD694FDA8F2EC7E044B13E6DB8F0B56D4BA9113A5EFAD334E3F3DED8C7`.

## Existing bridge and corrections

`runtime/vulkan/xess-fg/bridge.cpp` already owns a matching-LUID D3D12 device,
XeLL/FG contexts, DXGI proxy swapchain and shared resources. It is the appropriate
orchestrator-owned interop layer. Its legacy API remains available. The new
private `bridge_api.h` adds size/version checks, explicit absolute Intel runtime
directory, exact generated-count admission, reversed-depth initialization and
`BACKBUFFER_HUDLESS` composition. Godot supplies final color and completed
HUD-less color, so the service must not invent a UI-only alpha texture.

The new path accepts render-sized R32F depth and RG16F motion, and output-sized
BGRA8 final/HUD-less color. Intel documents current-to-previous pixel motion,
without jitter, and render-sized undilated motion with matching depth extent.
Matrices are row-major without jitter; jitter metadata is in render pixels.
Godot RGBA8 HUD-less needs a checked Vulkan blit to BGRA8, not an unchecked
reinterpretation. Actual XeSS dispatch with R32F depth remains to be tested.

`native_fg_xess_vulkan.cpp` provides the initial private import owner. It checks
the Vulkan LUID against the DX12 bridge adapter, queries D3D12-resource image
import capability, intersects native-handle and image memory-type masks, imports
dedicated optimal transfer images, and imports the DX12 fence permanently into a
timeline semaphore. NT handles close after import; COM references and Vulkan
objects remain owned until both queues drain. A failed drain must retain them.
See the [Khronos external synchronization contract](https://docs.vulkan.org/spec/latest/chapters/synchronization.html)
and [Win32 memory import lifetime](https://docs.vulkan.org/spec/latest/chapters/memory.html).

## Device experiment

The standalone fixture `tests/native_fg_xess_vulkan_interop.cpp` was compiled on
Windows using the installed LLVM-MinGW compiler with `-Wall -Wextra -Werror`.
It is separate from the main CMake build. This is a Windows device test using
LLVM-MinGW, not an MSVC verification run. Its default CPU-only checks reject a
short structure, mismatched ABI and relative SDK directory without loading a GPU
driver. Those checks passed.

`eng/test-xess-fg-vulkan-interop.ps1` acquired the common Godot GPU mutex, ran the
explicit device mode and released the lease. Process-only extension-point
mitigation was effective (`1`); no global RTSS setting changed. Khronos reported
core and synchronization validation enabled. Device vendor/device IDs were
`4318/11266` (NVIDIA). Intel context creation, permanent timeline import, BGRA8,
R32F and RG16F dedicated imports, Vulkan/DX12 drains and checked releases all
returned zero. No VUID or synchronization error was emitted; stderr was empty.

Compact evidence is `.local/godot/xess-fg-interop-20261025/receipt.json`,
`stdout.json` and `stderr.log`. Fixture SHA-256 is
`EC6D096D55488F9713172EAC7A545133F7B2295898BE08B79FE8245D944E769A`.
This run creates no swapchain and submits no texture transfers. It proves import
admission and destruction on this device, not frame generation or image quality.

## Remaining integration contract

The native presenter must own the HWND for its entire lifetime, including Off.
Its game-facing Vulkan images are ordinary images, not native WSI images. Their
final layout is shader-read. A separate options call supplies HWND and the
host's enabled timeline/Win32 interop features without extending generic ABI 1.
The engine owns HWND discovery, producer timing and input layout/usage promises.

Prepare and HUD-less copies bind to actual submitted command buffers. Present
consumes engine wait semaphores exactly once, queues the final copy and signals
the shared timeline's ready value. DX12 waits, tags resources `ONLY_NOW`, presents
through Intel, then signals done. Vulkan waits for done before resource reuse,
with explicit EXTERNAL queue-family ownership transfers. Fence values increase
globally and are not frame counters. First frames or missing producers need a
real-frame path with initialized private resources. XeLL CPU sleep/markers must
have one pacing owner and correct frame identity; a render callback alone is
insufficient.

## Private presenter device controls

`native_fg_xess_vulkan_present.cpp` implements a private provider behind the new
options call. It owns three ordinary game images and their shared final-color
copies, six guide/HUD-less slots, and initialized private fallback guides. It
does not send its opaque chain to native Vulkan WSI. The SDK receives
`BACKBUFFER_HUDLESS` composition and exact frame resources only after matching
producer command buffers were submitted. FG admission also requires matching
XeLL sleep and simulation/render CPU marker identity. Camera reset is a frame
constant, not presenter disablement. The actual-generated-image callback counter
remains zero because SDK present counts are not image callbacks.

The first 12-frame Off device control exposed a real allocation defect:
`VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL` requires sampled/input-attachment usage,
while the fake game-image allocation initially inherited only swapchain usages
plus transfer source. Khronos reported `VUID-VkImageMemoryBarrier-oldLayout-01211`.
The allocation now includes sampled usage. This failed control is retained in
`.local/godot/xess-fg-present-20261025`.

The corrected 12-frame Off run in
`.local/godot/xess-fg-present-sampled-20261025` passed core and synchronization
validation, presentation, both drains and checked release. A second control in
`.local/godot/xess-fg-present-guides-20261025` supplied synthetic static 640x360
R32F depth/RG16F motion and 1280x720 RGBA8 color, recorded actual guide/HUD-less
copies, submitted them, supplied XeLL CPU events, and enabled one generated frame.
It completed 12 real frames with 11 interpolated SDK statuses and zero
presentation/drain/release failures or validation errors. This also exercises
guide-slot reuse, format conversion, external ownership and bidirectional fence
ordering. Every ready value waits for the previous DX12 done value, preventing
one API from advancing the shared timeline past the other's unfinished work.

These controls establish resource transfer and SDK execution on this device.
They do not capture generated images and do not establish Godot image quality.
Generic WSI routing, engine CPU marker integration, actual generated-image
capture and existing geometry/UI gates remain required before any readiness
claim. Root owns those shared integration changes; the private provider files
were frozen after the passing controls.
