# Vulkan SR native port follow-up

The follow-up starts at `b9e0a480`. The integrated E03 Runtime SR session
still calls the public Vulkan SR functions.

## Split SR ABI and fixture

`runtime/orchestrator/src/native_sr_split_vulkan.cpp` was removed. Rust maps
the private FSR4 provider info into the public ABI and retains the checked
setup, frame, continuation and result handling. The C bridge contains only
the optional provider table and export forwarding. CMake keeps that table
disabled when the FSR4 graphics leaf is absent. The Rust archive remains
usable by hosts without the FSR4 leaf.

`tests/native_sr_split_vulkan.cpp` was removed. Rust exercises the actual
dispatcher with absent and stub providers, including both setup versions,
short records, output clearing, partial cleanup ownership and a failed
dispatch with a committed continuation. The separate DLL export fixture
now also calls the real split plan and destroy exports with null owners.
The C header fixture remains. The old fake threw C++ exceptions directly
from substitute public provider functions. Production provider entries now
use C/Rust linkage and cannot unwind across it. Exception containment for SDK
callbacks and split recording stays in `native_sr_fsr4_vulkan.cpp`, where
the actual C++ calls occur.

The existing generated private FSR4 and public Vulkan SR structures supply
the record types. The transfer is field by field and no public structure
or size changed. MSYS2 Clang C11 syntax checks passed for the thin export
boundary with FSR4 disabled and enabled. Pinned Rust formatting and
`git diff --check` passed. Builds, tests, lint and devices remain user-owned.

## Vulkan timing lifetime

`runtime/orchestrator/src/native_vulkan_timing.h` was removed. Rust opens the
timing file once on first use and registers the close callback with the
linking image's CRT before it publishes the handle. Normal unload releases the
file after the host drains callers. An unsuccessful registration drops the
file immediately and disables timing for that image. The [MSVC CRT
contract](https://learn.microsoft.com/en-us/cpp/c-runtime-library/reference/atexit)
documents DLL detach behavior. The GNU image still needs a link and unload check.

The existing FG and latency leaves use C ABI begin/end calls. They keep the
same tags and frame IDs. The outer WSI spans still include the borrow release,
and the inner spans still measure the actual Vulkan call. Failed Reflex
options still suppress sleep. The timing file format, QPC clock and 2,048
sample limit stay in Rust. Pinned formatting and source review passed.

## Spatial recovery

Rust now resolves and calls the three typed Vulkan procedures for spatial
recovery. The repository's Ash lock is `0.38.0+1.3.281`. Its system-ABI
function signatures were compared to the Godot Vulkan headers at
`ed1daf0bf001b61586d9930840f2f1394092c079` (header revision 335).
The route keeps the native checked application resolver, the optimal-tiling
linear-blit feature requirement, the source and target transfer barriers,
the active extents, and restoration of the source's shader-read layout.
The Rust fixture supplies typed Vulkan procedure fakes and checks recording
order, extents, alias refusal and a device without the needed features.

The public ABI is unchanged. The private SR table now has 33 pointers
(264 bytes on Windows x64) on both sides. Existing E03 session and XeSS
callers still use the same public entry points. The V06 branch was inspected
for overlap. Its XeSS FG capture and session changes use different paths.
G07's shared readback owner and Vulkan readback leaf contracts were left
untouched.

## Open inventory

- `runtime/orchestrator/src/native_sr_vulkan.cpp` still holds the typed Vulkan
  provider table, shared capture and sharpening construction and FidelityFX
  effect calls. A Bindgen 0.72.1 probe on FidelityFX
  `c6efa6bf7f2027b3ec94f28578bb5965eabb9e55` found the 40-byte
  `ffxCreateBackendVKDesc`, 432-byte `ffxDispatchDescUpscale` and the four SDK
  C function pointer types. The experiment generated an allowlisted ABI
  outside the repository. Moving those calls requires a stable bridge to the
  shared G07 readback and G05 sharpening leaves plus preservation of the
  SDK exception guards. V06's XeSS provider table remains an optional caller.
- `tests/native_sr_vulkan.cpp` still covers real Vulkan allocation and queue
  work, packed output pixel checks and fake-driver capture against the G07
  native readback leaf. Its device scenarios and CPU retirement assertions
  remain pending a port with those direct dependencies.

## V01 residual replay

The residual replay starts at `3665aef0`. That base already contains the typed
spatial recovery replay `552b1480`. FidelityFX Vulkan SR now creates its
graphics owner in Rust. The owner binds the shared image allocator, queries
physical-device memory properties with Ash, and records barriers and copies
through the integrated V02 functions. Zero-mask clearing uses the same typed
Rust command buffer and color range. FidelityFX's memory fallback remains in
the shared resource transaction. On initialization failure, the new graphics
owner releases any image-dispatch or capture state already acquired.

The graphics owner calls the G05 sharpening host and G07 capture owner
directly. SR still records raw provider output before sharpening. Readback
publication still requires the exact command retirement or a full drain, and
SDK destruction refusal still retains its dependencies for another attempt.
The V06 XeSS typed SDK bridge and the optional DLSS and XeSS platform entries
stay in place.

The compiler-selected C++ leaf still compiles against the configured
FidelityFX Vulkan headers. It retains version discovery, create and dispatch
records, provider resource interpretation, and the exception guards around the
four SDK procedures and host resolver. The public exports and their
feature-disabled forwarding remain in that translation unit.

The former `tests/native_sr_vulkan.cpp` body now uses Rust and Ash. Its C
entry provides the public SR, FG, and Streamline callbacks and the configured
feature bit. The original CTest names, arguments, timeouts, and skip codes
remain. The device routes still create a Vulkan 1.2 device, submit commands
and wait on a fence. They preserve two gradient recovery passes with
full-output comparison, FSR2 and FSR3 dispatch, DLSS with a shared PCL
session and default/K presets, the 12-frame reset and optional-input
sequence, and packed FP16 comparisons. The CPU path still records three
fake-driver readbacks through the production capture owner and separately
checks submitted, abandoned, and drained recordings. Its typed fake Vulkan
entry points preserve the event-completion and no-map assertions.

### Source evidence

- Rust 1.94.1 workspace formatting and locked offline Cargo metadata passed.
- Bindgen 0.72.1 reproduced the checked-in Vulkan SR, shared resources, and
  sharpening records byte-for-byte after newline normalization. A Windows
  x64 probe returned 128 bytes and the expected 16 pointer offsets for the
  fixture table. The pinned FidelityFX headers
  (`c6efa6bf7f2027b3ec94f28578bb5965eabb9e55`) produced 40-byte backend and
  432-byte dispatch descriptors with the Godot Vulkan headers
  (`ed1daf0bf001b61586d9930840f2f1394092c079`). The SDK probe required C++
  parsing because the Vulkan SDK header uses unqualified struct names.
- Clang source syntax checks with warnings as errors passed for the production
  C++ leaf and the C fixture entry, each with the Vulkan SR feature enabled
  and disabled. Source comparison covered the seven public SR exports,
  optional provider routes, SDK call sequence, and the fixture assertions.
- Build, binary linkage, fixture execution, and hardware results remain
  pending. The two V01 residual PORT entries can cite this section, with
  the production entry retaining the optional C++ SDK leaf.
