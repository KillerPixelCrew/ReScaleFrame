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

Five checklist paths had source cutovers before the continuation at `726c5e18`.

- `loader/census/src/decode.cpp`, replaced by Rust decoding and the guarded COM leaf.
- `loader/census/src/static_scan.cpp`, replaced by the Rust scan worker body.
- `loader/diagnostics/include/rescaleframe/frame_capture.h`, the retained C contract for the
  Rust capture state and optional RenderDoc connector.
- `tests/vendor_census_asi.cpp`, replaced by Rust fixture behavior and the native attach stub.
- `tests/vendor_census_fixture.cpp`, replaced by Rust fixture behavior and the native NGX and
  loader-lock leaves.

The continuation closes these five additional paths.

- `loader/census/src/wrap_vk.cpp`
- `tests/module_dump_v2.cpp`
- `tests/census_present_coexistence.cpp`
- `tests/vendor_census_ngx_params.h`
- `tests/loader/vendor_census_ngx_platform.cpp`

The census core, internal contract and module-dump internal contract are partially ported.
Fifteen paths from the continuation baseline remain open.

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
- `loader/census/src/decode_d3d_platform.cpp`
- `loader/diagnostics/include/rescaleframe/module_dump.h`
- `loader/diagnostics/src/module_dump_internal.h`
- `tests/vendor_census.cpp`
- `tests/vendor_census_layouts.cpp`

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

## Executable entropy and polling

Rust now implements `rsf_measure_module_code` and its bounded poll loop. The PE selection
still uses the largest executable section and the same evenly spaced 64 KiB windows. A single
window is copied at a time with the existing guarded zero-fill behavior. The reader checks
the PE header and live range before sampling. The poll uses the 64-bit tick count to support
the full 32-bit timeout range and preserves the consecutive-sample threshold and zero timeout.
The native writer still owns dump publication and updates only the existing timeout report
field. The public header retains all declarations and layout.

## Desktop coexistence fixture

`tests/census_present_coexistence.cpp` is replaced by Rust with a C entry stub for the census
callbacks. The fixture retains its opt-in command, debug-layer message 904 counter, hardware
adapter filter, visible triple-buffered flip-discard chain, per-buffer transitions, Present
and fence ordering, and summary fields. The COM calls use the generated Windows bindings.
Setup failures and a failed fence wait now produce a failure result instead of proceeding
with an unavailable resource. The build and desktop run remain user-owned.

## NGX fixture parameter object

Rust now supplies `NVSDK_NGX_D3D12_GetParameters` and the 17-slot parameter object. The
native `tests/vendor_census_ngx_params.h` retains the vendor declaration order for independent
call-site slot selection, while the storage and every virtual operation are in Rust.
`tests/loader/vendor_census_ngx_platform.cpp` is removed. The C++ slot probe now checks
all eight setters alongside the prior eight getters and reset, deriving each slot from the
native declarations. The DLL export name, pointer result and numeric versus pointer lookup
behavior are preserved.

## Module-dump name sidecar

The names section now reads and serializes the TSV in Rust, with the native writer receiving
byte spans for its existing stream. The port keeps the bounded line reads, decimal and prefixed
hex addresses, comments, UTF-8 BOM, skip count and UTF-16 JSON escaping. The schema fixture
adds a first-record BOM and a non-BMP character to exercise the ported parser. Native module
list, thread, function and RTTI sidecar sections still need further porting.

## Census hook binding ownership

The branch at `82b39a63` removes `loader/census/src/hooks.cpp`. Rust owns the export lookup
table, binding and conflict accounting, resolver and loader callbacks, module filtering, patch
snapshots and JSON hook summary. The MinHook callback table carries the unchanged native export
descriptors and the four graphics creation roots. The Rust binding path still preserves their
resolver results to avoid a cached-thunk cycle with an overlay. Last error is restored after
bookkeeping. Existing detour trampolines stay mapped on shutdown.

The native platform leaf contains the MSVC loader-list walk and MinHook calls. Each loader-list
read is guarded, and the walk has a traversal bound for calls outside DllMain. A mutex now
publishes whole hook records and names. A contended fatal-error summary may omit the hook and
module arrays for that sample instead of waiting on the hook record lock.

Source checks compare resolver entry names, data-file flags, root exclusions, summary keys,
patch length, and exact descriptor sharing. Builds and the existing DllMain, creation identity,
Vulkan and injected-overlay fixture runs remain user-owned.

## Record producer and writer boundary

The Rust queue keeps the 1,024 sequence-stamped slots, separate cache lines for producer and
consumer positions, the 6,144-byte argument budget, and 32 captured stack frames. Native
wrappers fill a reserved record and publish its ticket. Rust consumes published records under
the existing writer lock, serializes the same JSONL envelope and sends the resulting byte span
to the current file writer. The line scratch buffer belongs to the queue so fatal-error
draining does not put a full JSONL line on the crash thread's stack.

The SEH and executable-memory probes live in a separate native platform file. Rust owns
call-site admission, the sampling cadence, per-function and per-queue histograms, and
present indexing. The same summary entries keep their original array order, capacities,
and field names.

## Census session and file writer

`loader/census/src/core.cpp` is removed. Rust owns the public session behavior, call stack
records, worker scheduling, module and thread registries, vectored diagnostic callback,
summary assembly and buffered output. The public `vendor_census.h` ABI and numeric results
are preserved. The native entry provides the wrapper catalog and hook install callback.
`census_platform.cpp` also keeps the Vulkan wrapper table and CRT float formatting.

The writer and output buffer are serialized together. Module bases publish in one atomic
operation and thread names publish under their own tiny lock. A crash record uses a preallocated
slot, and the fatal handler tries the writer lock within the original attempt limit. Shutdown
retains worker handles and reports I/O failure if a join times out, so callers can retry.
Summary publication requires a complete temporary-file write. JSONL write failures retain
the prior admitted-byte accounting semantics.

Source checks compare the API validation, exported names and struct layout, JSON field inventory,
worker deadlines, file names and output caps. The remaining native wrapper and SEH leaves
continue to own borrowed SDK calls.

## NGX vendor wrapper cutover

`loader/census/src/wrap_ngx.cpp` is removed. Rust registers all 42 D3D12, D3D11 and Vulkan NGX
wrappers in the native catalog's order. The version-dependent entry points keep the existing
eight-word Windows x64 forwarding convention. Rust owns parameter selection, feature and quality
decoding, NGX Vulkan resource fields and the D3D12 notification sequence. Create and release
observers still require success. Evaluation observers still run before and after the original
regardless of record sampling.

Native leaves keep the guarded virtual getter calls, D3D resource description probes and CRT
float formatting. The D3D12 registration API keeps its public signature. Source checks compare
all catalog and decoder names, getter order and observer phases with the previous wrapper.

## DXGI and D3D12 wrapper cutover

`loader/census/src/wrap_d3d.cpp` is removed. The sixteen Rust wrappers use the native argument
widths, stack argument counts, HRESULT and handle returns, and void submission return. The
Windows bindings supply the DXGI and D3D12 description layouts. Factory, swapchain and queue
admission still use the same SDK slots and interface IDs. Rust reports a zero return for the
void ExecuteCommandLists method.

The Rust observers back the unchanged public `census_d3d12_observer.h` ABI. The shared present
depth still covers both APIs, filters DXGI test presents, and sends the before-present callback
only through the original Present path. Borrowed interface queries, debug-layer configuration
and debug-message registration remain isolated in `d3d_guarded_platform.cpp`.

## Vendor SDK layout fixture

`tests/vendor_census_layouts.cpp` is replaced by a Rust comparison fixture and native SDK
fact collector. All 188 expected values stay associated with the same types, fields and
SDK sets. The fact collector still uses the actual optional vendor headers to supply the
independent offsets and sizes, including C++ layouts from Streamline. CTest keeps all five
conditional targets and the original skip code.

## NVAPI, Streamline and AMD wrapper cutover

`loader/census/src/wrap_misc.cpp` is removed. The 36-entry Rust table retains NVAPI Reflex IDs,
the assumed-layout labels on NVAPI and NvLowLatencyVk records, Streamline decoder bounds,
and the AMD and AGS fields. NVAPI query and Streamline feature resolution preserve the same
one-binding behavior. The remaining version-dependent exports retain eight-word forwarding.
Streamline uses native guarded callbacks for its polymorphic token and writable output pointer.

## FidelityFX and XeSS wrapper cutover

`loader/census/src/wrap_fsr.cpp` and `loader/census/src/wrap_generic.h` are removed. Rust
registers all 48 FSR2, FidelityFX, XeSS, XeFG and XeLL entries. The four floating-point
signatures still use exact typed calls. Version-dependent exports keep the existing x64
word forwarding, including the first hidden result pointer for the FSR2 resource return.

The decoder keeps the 1,352-byte FSR2 record and all refusal reasons, the 12-element
FidelityFX chain bound, and the XeSS layout. The Rust decoder reads copied byte fields and
checks the raw boolean bytes before decoding FSR2. COM resource queries stay guarded in
the platform leaf. The source comparison covered every registration, decoder field,
descriptor kind and the original payload spans.

The last native wrapper support header is reduced to the vendor catalog and guarded platform
declarations. The DXGI and Vulkan platforms now share the Rust per-thread present scope.

## Module writer boundary

The PE relayout, import scan and final sidecar publication now belong to
`loader/diagnostics/src/module_dump_writer.c`. That translation unit depends on the private
image and module inventory helpers, the protected reader, and the sidecar sections. The
engine patch and console callbacks stay in their existing native translation unit. The
writer's algorithms and public dump declarations are unchanged.

## Census creation fixture

The creation identity mode in `tests/vendor_census.cpp` now runs in Rust. It loads the fixture
through the original LoadLibraryW path, checks the four early exports and verifies the four
resolver pointers after post-load. The shared C++ functional, budget, Vulkan, FidelityFX and
DllMain modes still need porting.

## Budget and DllMain fixture modes

The continuation at `031aa9ac` moves the budget and DllMain scenarios and their JSON
record assertions into Rust. The fixture still uses the native noinline import callers
for the 5,000 jitter calls and two FSR2 dispatches. The ASI remains a real DLL loaded via
LoadLibraryW with the FSR2 DLL already mapped by the executable's static import.
The assertions retain the byte and counter limits, dropped-record accounting, site totals,
accepted dispatch layout and caller module. The JSON parser dependency is scoped to the
Windows MSVC fixture crate.

## Inventory at the end of the 82b39a63 continuation

Eleven additional baseline paths have source cutovers. The two public header paths retain
their existing C declarations. The internal header retains the native catalog and platform
declarations.

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
- `tests/vendor_census_layouts.cpp`

Five baseline paths remain open.

- `loader/diagnostics/include/rescaleframe/module_dump.h` and
  `loader/diagnostics/src/module_dump_internal.h` still cover the native dump writer, RTTI and
  other sidecar sections.
- `loader/census/src/decode_d3d_platform.cpp` still owns the guarded COM resource queries.
- `tests/loader/module_dump_rtti_fixture.cpp` supplies the compiler's RTTI specimen.
- `tests/vendor_census.cpp` still owns the functional, budget, Vulkan, FidelityFX and DllMain
  modes.

The new native boundaries are `loader/census/src/census_platform.cpp`,
`loader/census/src/hook_platform.cpp`, `loader/census/src/guarded_platform.cpp`,
`loader/census/src/d3d_guarded_platform.cpp`, `loader/diagnostics/src/module_dump_writer.c`,
and `tests/loader/vendor_layout_facts.cpp`. The Windows builds, fixture execution, lint and
device checks remain user-owned.
