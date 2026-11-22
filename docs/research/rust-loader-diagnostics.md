# Loader census and diagnostics Rust port

## Source comparison

The L02 port starts from `ab173d96`. The census remains attached through the existing native
install API. Its scan worker still records findings and the completion event through the census
writer. Rust reads one PE section at a time with `ReadProcessMemory`. An unreadable section
ends the scan. The signatures, RIP-relative instruction forms, exception-directory attribution,
leaf heuristic and reporting caps come from `loader/census/src/static_scan.cpp` at that revision.
The leaf heuristic stays within the copied code section. Findings still identify leads for
inspection.

The census JSON buffer and resource fields now come from Rust. The COM leaves retain the Windows
SDK calls and SEH. The native leaf copies resource descriptions before the Rust writer formats
them, including an available D3D11 dimension when the texture call faults. The platform adapter
still formats finite floating-point fields with the CRT's `%.9g`.

RenderDoc capture stays optional. The C adapter still acquires API 1.6.0 through the supplied
vendor header and configures the output template, overlay, keys and vendor extension setting.
Rust owns loading, publication, trigger selection and count. Failed connections release the
library, and concurrent or reentrant initialization returns the existing not-ready status while
the table is being connected.

Module dump entropy and guarded copies are implemented in Rust. The dump keeps its existing
region protections and uses `ReadProcessMemory` for the copy, so losing readability after
`VirtualQuery` cannot fault inside the copy routine. Exact copies stage into scratch space before
publishing. Guarded span copies keep the readable bytes and zero the rest.

The ASI fixture still calls installation from its native `DllMain` and exports the same four
symbols. Rust reads the bounded environment value, builds the v1 options and stores the result.
The existing functional census fixture still provides the static xref case.

## Verification scope

The port was checked by source comparison, generated x64 layout assertions, pinned Bindgen
0.72.1, pinned Rust 1.94.1 formatting, locked offline Cargo metadata, and Git whitespace checks.
Builds, lint, fixture execution and game or device verification are user-owned.
