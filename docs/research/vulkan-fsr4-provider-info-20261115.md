# Effective FSR4 provider query, 15 November 2026

## Question and source evidence

The coordinator's fullscreen native-resolution TotK FSR4 run after the allocation
generation correction completed 1536 presents, exited zero and reported zero
validation errors, shader failures or skipped draws. Its first successful temporal
evaluation was logged without an SR refusal or spatial fallback. Provider creation
included roughly 23.5 seconds of first SDK compilation. Those are coordinator-owned
runtime observations, not worker device tests, and selecting backend 4 alone is
not a sufficient statement of effective provider identity.

`runtime/vulkan/fsr4/bridge.cpp`, `CreateContext` already forces a 4.x provider via
the existing override, verifies the effective provider, and refuses mismatches.
The signed provider's adapter LUID, effective opaque provider version identifier,
PCI vendor ID and active INT8 compatibility lease are stored in the bridge owner.
`RsfFsr4BridgeInfo` exposes those recorded facts through the private bridge ABI;
`native_sr_fsr4_vulkan.cpp`, `rsf_fsr4vk_info` already forwards that query. No vendor
policy, provider selection, PCL delivery or DLSS capture implementation changed.

## Additive public contract

`native_sr_split_vulkan.h` appends the optional `rsf_srvk_fsr4_info` query and a
separate `rsf_srvk_fsr4_provider_info` structure. The info ABI starts at version 1:
size/version at offsets 0/4, adapter_luid/provider_version as uint64_t at 8/16,
vendor_id/int8_compatibility as uint32_t at 24/28, total size 32 on Windows x64.
The six existing split APIs, their structures and their ABI versions are unchanged.

`native_sr_split_vulkan.cpp`, `rsf_srvk_fsr4_info` validates size/version and owner,
queries a correctly versioned private bridge-info local through the existing
`rsf_fsr4vk_info`, then copies actual returned facts on success. Native/private
error mapping follows the other split entry points. Short buffers are not written;
a full known output prefix has zero facts on errors, including disabled support,
query refusal and exceptions. Caller size/version and unknown appended bytes are
preserved. The opaque provider version is reported without inventing a semantic
version string or deriving a version from the backend selection.

Older hosts need not call this additive export. Hosts compiled against the new
SDK should resolve it optionally, so an older runtime can still provide its six
working SR exports. Query failure is honest unknown information; it is not an
alternative to provider admission, successful dispatch or device validation.

## TotK consumption

The separately authored TotK `runtime/rsf_sr.c`, `split_info_log` resolves the new
query only after successful split creation and planning, once per new owner,
under the existing TOTK_TRACE=gfx bit. Success logs the exact opaque provider
version, adapter LUID, vendor ID and INT8 compatibility flag. A missing export or
nonzero result logs unknown with its actual error. The info query does not affect
SR creation/evaluation admission and cannot trigger a different SR provider.

Compilation against older pinned SDK headers remains supported by a feature-macro
guard, which reports the query absent from those headers. After publishing the new
RSF paths and updating the external/ReScaleFrame pin, the same TotK source enables
the query automatically. No pinned checkout was edited by this worker.

## CPU verification

No GPU, game, UI, device tests, shared build writes, stage-DLL replacement, staging,
commits or remote actions were performed. The isolated build is
`build/worker-fsr4-sr-20261115` and was already configured for Visual Studio 18 2026:

```powershell
cmake --build build/worker-fsr4-sr-20261115 --config Debug --target rsf_native_split_stub_test rsf_native_split_disabled_test rsf_native_split_c_header rsf_native_split_test rsf_native_vulkan_exports_test -j 4
ctest --test-dir build/worker-fsr4-sr-20261115 -C Debug -R '^(native_vulkan_dll_exports|native_sr_split_vulkan_contract|native_sr_split_vulkan_(stub|disabled))$' --output-on-failure
```

The complete real Vulkan service/archive, runtime DLL and focused fixtures build
successfully with MSVC. Four focused CPU tests pass: real DLL export availability,
production split ABI validation, disabled support and stub-backed private query
forwarding. The C header fixture compiles with layout/offset assertions. Checks
cover null owner/output, short-before-version validation without writes, version
mismatch, exact 64-bit facts/flags, appended-byte canary preservation, disabled
unknown output, native error preservation, failure output clearing and exception
containment. Stub values are synthetic fixtures, not reported hardware identities.
The existing private info glue was reused without modification.

From the TotK root:

```powershell
python tests/rsf_fsr4_info_check.py
python tools/check_content.py --staged
```

Both commands exit zero. The first writes only `build/checks/fsr4-info-worker`,
compiles and executes the actual logger with new sibling and older pinned SDK
headers, compiles the complete adapter twice with UCRT64 Clang -Wall -Werror,
and runs its actual lifetime fixture against both header sets. It proves exact
fact logging, missing export/query-error unknown results, no tracing-side calls
when gfx tracing is off, and continued ordinary/split owner lifecycle behavior.
The lifetime fixture gained only an rt_trace definition, off by default, to satisfy
the adapter's existing host tracing dependency. Scoped whitespace checks pass in
both repositories. No unrelated native_sr.cpp workaround was needed for this
isolated DLL build; concurrent dirty work was preserved.

## Remaining acceptance and ownership

The coordinator publishes only the owned RSF paths, updates the TotK gitlink,
rebuilds the combined runtime and repeats its own bounded fullscreen focused FSR4
acceptance with TOTK_TRACE=gfx. That run must record genuine effective identity;
this worker cannot supply hardware/provider values from CPU fixtures. The full
combined native gate remains coordinator-owned. This query does not establish
moving-scene SR quality, measured latency or an SR fallback.

Owned RSF paths: runtime/orchestrator/include/rescaleframe/native_sr_split_vulkan.h;
runtime/orchestrator/src/native_sr_split_vulkan.cpp; tests/native_sr_split_vulkan.cpp;
tests/native_sr_split_vulkan.c; tests/native_vulkan_exports.cpp; this note (new).
Owned TotK paths: runtime/rsf_sr.c; tests/rsf_sr_lifetime_check.c (one tracing global);
tests/rsf_fsr4_info_check.py (new). No other files were edited in this dispatch.

Publishing caution: native_sr_split_vulkan.cpp and its existing C++ split fixture
already contained unrelated camera-ABI migration hunks at dispatch start. This
worker preserved those bytes; they are not part of the info-query change. Stage
only the new query/struct/tests/export hunks for this task unless the coordinator
intends to publish the wider camera migration too. Likewise, the isolated DLL
includes the current shared dirty native tree and proves the query/export builds,
not that every unrelated dirty file should be included in this task's commit.
