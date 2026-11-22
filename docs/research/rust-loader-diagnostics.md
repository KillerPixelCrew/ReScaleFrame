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

The eight fake vendor DLLs keep their original export sets and creation aliases through module
definition files. Rust supplies their argument receipts, typed return values and dynamic symbol
dispatches. DirectX creation still resolves its exports under the loader lock before post-load
hooks. The NGX leaf keeps the native compiler's parameter vtable and the fixture still disables
identical-code folding and incremental linker thunks.

## Verification scope

The port was checked by source comparison, generated x64 layout assertions, pinned Bindgen
0.72.1, pinned Rust 1.94.1 formatting, locked offline Cargo metadata, and Git whitespace checks.
Builds, lint, fixture execution and game or device verification are user-owned.

## L02 inventory

Five checklist paths have source cutovers.

- `loader/census/src/decode.cpp`, replaced by Rust decoding and the guarded COM leaf.
- `loader/census/src/static_scan.cpp`, replaced by the Rust scan worker body.
- `loader/diagnostics/include/rescaleframe/frame_capture.h`, the retained C contract for the
  Rust capture state and optional RenderDoc connector.
- `tests/vendor_census_asi.cpp`, replaced by Rust fixture behavior and the native attach stub.
- `tests/vendor_census_fixture.cpp`, replaced by Rust fixture behavior and the native NGX and
  loader-lock leaves.

The census core, internal contract and module-dump internal contract are partially ported.
Eighteen checklist paths remain open.

- `loader/census/include/rescaleframe/census_d3d12_observer.h`
- `loader/census/include/rescaleframe/vendor_census.h`
- `loader/census/src/census_internal.h`
- `loader/census/src/core.cpp`
- `loader/census/src/hooks.cpp`
- `loader/census/src/wrap_d3d.cpp`
- `loader/census/src/wrap_fsr.cpp`
- `loader/census/src/wrap_generic.h`
- `loader/census/src/wrap_misc.cpp`
- `loader/census/src/wrap_ngx.cpp`
- `loader/census/src/wrap_vk.cpp`
- `loader/diagnostics/include/rescaleframe/module_dump.h`
- `loader/diagnostics/src/module_dump_internal.h`
- `tests/census_present_coexistence.cpp`
- `tests/module_dump_v2.cpp`
- `tests/vendor_census.cpp`
- `tests/vendor_census_layouts.cpp`
- `tests/vendor_census_ngx_params.h`

L02 has no numbered Rust findings.

## Typed Vulkan entry points

The follow-up branch at `726c5e18` replaces `loader/census/src/wrap_vk.cpp`. The eleven
wrappers use the Windows x64 Vulkan entry signatures. The Rust bodies forward each argument,
read the same bounded input layouts through the census guarded reader, and publish the same
fields in the same order. The two void queue lookups report a zero result. The native core
still owns record admission, first-binding wins, and the shared present nesting counter.
The native callback table checks the size and alignment of the stack record before constructing
it in the caller's storage. Its storage stays at one address through record completion.

The entry wrappers ask Windows for their caller PC before invoking helpers. Caller attribution
now depends on Windows unwinding reaching the caller. Source checks cover table slot order,
field names, layout spans, bounds and the existing functional fixture's paths. Binary checks
still need to exercise caller attribution, thunk conflicts, Vulkan result forwarding and
cross-API nested presents.

## Module-dump schema fixture

`tests/module_dump_v2.cpp` is replaced by the Rust fixture and a small native RTTI specimen.
The executable keeps the native vtable and inheritance records under test. Rust starts and names
the worker, checks its reported RVA, validates the exception, names and RTTI sections, and
compares section digests with BCrypt across the bytes in the file. The short v1 report has a
sentinel immediately after its ABI prefix to detect writes into the extension. CTest retains
the same fixture target, output directory and timeout. The CLI expects the UTF-8 directory
already required by the dump API.
