# D3D12 Streamline host startup in Rust

Compared with `8f5d0b49`, the existing Rust host owner now sequences the work
formerly in `dlss_generation.cpp`'s `admit` and `open`. The private startup
table exposes typed COM, Win32, Streamline, MFG and diagnostic operations.
The public `streamline_host.h` ABI remains 2. The public wrappers, the shared
DLSS SR and D3D11 presentation consumers, the `rsf_dlss` Rust archive link,
and the Streamline-disabled stubs match the baseline sources.

## Admission and preferences

The owner lock still covers admission through host publication. The engine
value must be below the compiled SDK's `sl::EngineType::eCount` before
`IDXGIAdapter::GetDesc` is attempted. Failed interface or description queries
return `INVALID_ARGUMENT` (`-1`). A `VendorId` other than `0x10de` emits the
same refusal while the adapter reference is live and returns `NOT_SUPPORTED`
(`-7`). An existing `sl.interposer.dll` returns `NEEDS_RESTART` (`-11`). The
setup size and ABI checks still return `INVALID_ARGUMENT` (`-1`) and
`ABI_MISMATCH` (`-2`), respectively. Output reentrancy and owner lock
contention still return `NOT_READY` (`-8`). Byte-for-byte copies into the
128-byte engine version and 256-byte project ID buffers and the absolute
UTF-16 directory validation still run at their original points. The
interposer load still uses the signature gate and restricted DLL search.
A failed load returns `LOAD_FAILED` (`-4`).

`SdkModule::symbol` does the required lookups against that loader reference
in this order. `slInit`, `slShutdown`, `slUpgradeInterface`,
`slGetNativeInterface`, `slSetD3DDevice`, `slIsFeatureSupported`,
`slGetFeatureFunction`, `slGetFeatureVersion`, `slSetFeatureLoaded`,
`slGetNewFrameToken`, `slSetConstants`, `slSetTagForFrame`. The first missing
entry returns `MISSING_ENTRY_POINT` (`-5`). The addresses now bind to the
typed native members after all 12 lookups succeed. The old code bound each
member as it resolved and disposed the unpublished state on failure.

The D3D12 preference record retains the engine type, copied version and
project ID, one plugin path and the existing log callback. The ordered
feature list is DLSS, PCL, Reflex, DLSS-G for the FG profile, DLSS, PCL,
Reflex for Reflex, and PCL for the PCL profile with DLSS prepended only if
shared SR asks for it. The Rust host retains the directory, strings and
feature array through shutdown. Native constructs `sl::Preferences` and
passes `sl::kSDKVersion` from the compiled headers. Both modes replace the
SDK default flags with manual hooking and frame-based resource tagging.
Retail adds disable-debug-text, uses default logging and hides the console.
Development uses verbose logging and shows the console. SDK log registration
still precedes `slInit` with the same callback and verbosity. The moved
feature IDs, preference values and direct-queue type were checked against
the local Streamline 2.14.1 and Windows SDK 10.0.26100.0 headers and are
asserted in C++.

## Staging and rollback

After `slInit` succeeds, startup retains the supplied device when both
graphics inputs are present or creates a device at feature level 11.0 on the
admitted adapter. FG startup primes MFG on that device using the original
signature policy. It then upgrades the device, retains the native alias,
starts the Reflex audit, registers the proxy device with `slSetD3DDevice`,
creates the DXGI factory with flags zero and upgrades it. A supplied queue
must be direct and its `GetDevice` must return the same `ID3D12Device*` as
the native alias. Failure at either check returns `WRONG_API` (`-12`). The
host always creates its own direct queue on the proxy device and retains the
native queue alias. The presentation bridge keeps ownership of the supplied
queue and the inter-queue handoff.

Native allocation, `slInit`, device and factory creation and upgrades, device
registration, proxy queue creation and native alias failures still return
`INIT_FAILED` (`-6`). Failed upgrades release the original object before
disposal. Disposal still stops the audit, releases the proxy and native
queues, factory, proxy and native devices in that order, conditionally calls
`slShutdown` after successful initialization, releases MFG, clears the log
registration and deletes the native state. A temporary `GetDevice` reference
on a queue refusal is released after this disposal, before the interposer
lease unloads. On acceptance it is released before creating the proxy queue.

Optional lookups still run after graphics setup. DLSS-G looks up GetState
then SetOptions and runs MFG preparation even when an entry is absent.
Reflex looks up SetOptions, GetState, then Sleep. PCL looks up SetMarker,
GetState, then SetOptions. A successful PCL SetOptions lookup triggers the
typed default options call, and only a successful state query can publish a
nonzero message ID. Missing optional functions leave their capability bits
clear and startup continues. The host pointer is published last. Frame identity,
PCL routing, Reflex pacing, diagnostics and generation load after startup
match their baseline sources.

Source comparison matched the 12 required slots, eight optional bindings,
the startup and retained callback tables, preference fields, result codes,
and the rollback and consumer paths. Pinned Rust 1.94.1 formatting, locked
offline Cargo metadata and whitespace checks passed. Compilation and linking
with and without Streamline, export comparison, Clippy, tests and device and
game verification are pending user verification.
