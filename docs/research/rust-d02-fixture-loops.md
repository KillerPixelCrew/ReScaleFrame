# D02 linked presentation fixture loops in Rust

This continuation starts from `b5a7434c`. That base already includes
`20995953`, the replay of `ac31d685`. The existing linked executables and
CTest registrations still drive the production presentation owners.

The shared FG runner now owns the 40- or 130-source-frame loop,
provider preparation and retirement callbacks, input refusals, provider
status and switches, stable buffer and latency-handle checks, foreground
lease, concurrent engine queries, cached DX11 view and the readback
comparison. It submits the source list, publishes source submission,
joins the window callback, draws the overlay when selected, and submits
the finish transition to PRESENT before the application's actual
Present. Frame activity still comes from the vendor and MFG checks still
count effective 2x, 3x, 4x and 6x presents. A failed drain exits through the
launcher's early-failure path, with callback resources held through
process exit. The native file retains argument and window setup, backend
and device startup, the optional native DLSS SR attachment, resource
leases and final cleanup.

The AC7 runner now owns the 16-, 120- or 240-frame loop, flight, hangar
and briefing changes, early and late SimulationEnd, controller marker
deduplication, SDK sleep and token assertions, provider switches and
multiplier clamping. It checks the source Present count, sampled and
direct activity, the PCL ping, and the abandoned-window pacing join.
The native file retains cold startup and fallback, COM identity and
resize checks, the exception-guarded AC7 command-list execution and
scene/window checks, and the production scope, SR, RTV and Present calls
inside that per-frame seam. Rust consumes the seam's actual SR,
Present and pass results. Both launchers retain their original SDK and
device paths, including selected hardware and the AC7 proxy smoke's
hardware-then-WARP behavior.

The private C/Rust contracts have size and offset checks on both sides.
Verification used Rust 1.94.1 formatting, targeted repeatable bindgen
0.72.1 output, locked offline Windows-target Cargo metadata, MSVC
`/Zs /W4 /WX` for both launchers and source comparisons of frame and
callback ordering. The first D3D11 syntax check hit an MSVC internal
compiler error on aggregate initialization of a list with a self
pointer. Direct list construction and an explicit reset preserve the
native layout and passed the subsequent syntax check. Linking, test
execution, lint, device measurements and game validation are pending.

## Startup and teardown continuation

Base `cb8a0e0c` includes `16033561`, the replay of the live loops.
`d3d11_fixture_startup.rs` owns argument modes, the cold status refusals,
Win32 window and device setup, observer admission, the original fallback,
and the canonical IUnknown and source device checks. It keeps the 12
startup Presents, held-buffer resize refusals, the 1024×576 buffer and
DXGI TEST Present check, and exact owned Release after dropping the
alternate interfaces. The proxy smoke still tries hardware before WARP.

`fg_fixture_startup.rs` owns selected hardware creation, native hook or
explicit HWND creation, deferred provider startup, cached DX11 RTV and
application latency handle acquisition, the Auto and FSR4 capability
inputs, and optional native DLSS SR attachment. Teardown still closes the
wait handles and destroys SR and overlay only after a completed loop.
The provider callback state survives to CRT process teardown, including
when completion is unknown. Both execute the same live loops and link
the production providers.

`tests/d3d11_present_bridge.cpp` keeps the native command list layout,
execution exception guard, AC7 scope and command callbacks, native
exception report, and reentrant marker callback table. Its typed
startup callback table keeps AC7-specific symbols within that executable.
`tests/fg_present_bridge.cpp` retains the CRT entry and stdout setup.
The CTest registrations and both executable paths stay in place.

Source checks compared the guarded AC7 command and exception bodies with
the base, all mode names, and startup and final Release order. Pinned
Rust formatting, repeatable targeted bindgen, locked offline Windows
metadata and MSVC syntax checks passed. Build, link, fixture execution
and device validation remain pending.
