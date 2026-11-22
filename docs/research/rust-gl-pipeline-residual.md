# D08 OpenGL platform and hardware fixture source port

The comparison starts at `3665aef0`, which already contains the Rust WGL and D3D12
sharing owner, the exact-backbuffer overlay handoff and the resize rollback. The
question was whether the remaining C++ dispatch and device fixture could move to
Rust without changing those ownership contracts.

The Rust pipeline now opens the flip-discard chain through the same explicit
presentation bridge, using the same D3D12 queue and the child HWND created on
the event thread. It acquires the current backbuffer once, holds its COM
reference through composition and Present, and gives the overlay the same
resource plus the separate parent input HWND. The public entry points and
struct layouts stay stable. The native leaf supplies only header-inline
contract queries and constant assertions. The existing presentation bridge,
overlay binding and vendor leaves still own their respective operations.

The D08 dispatch calls the local GL, SR12 and FG12 owners directly. The
replay on C02 commit `e993c6ad` uses its shared SR12 host constructor and FG12
native binding, including for the explicit OpenGL installation. The D08
context, texture and pipeline entry points
all reserve a handle before GPU initialization. A failed pipeline open returns
a cleanup handle when retirement refuses. A rejected resize retains the last
published surfaces and restores or tracks the child window extent through the
existing transaction.

The hardware fixture retains the `gl_shared_surface` CTest target, its 60
second timeout, skip result 77 and hybrid GPU selection exports. Rust owns
the hidden WGL window, two-way readback and fence checks, threaded event
probe and optional vendor, resize, settings, overlay and capture scenarios.
The C launcher holds the two GPU selection exports and calls the Rust entry.

Source checks traced the 53 dispatch slots and compared the hardware
scenario matrix and entry points to the base. Bindgen 0.72.1 reproduced
the D08 outputs. The full generator also removed C02's SDK module
declaration and rewrote a C02 fixture import. Those two files were restored
to the replay base before running the pinned Rust 1.94.1 workspace format,
locked offline metadata and Git whitespace checks, which passed. Windows
linkage, exports and device results remain unverified for these commits.

C02 handles the render host and callback host portions of the shared-owner
dispatch finding. The native window, overlay host and overlay service
portions remain open. Existing SR and FG native leaves for their other
consumers, and allocation changes outside the three D08 entry points, were
outside this source port.
