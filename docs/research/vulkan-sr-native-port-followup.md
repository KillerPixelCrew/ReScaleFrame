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
