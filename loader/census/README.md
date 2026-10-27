# Vendor call census

`rsf_vendor_census` (alias `ReScaleFrame::VendorCensus`) is a diagnostic probe. Installed inside a game
process, it records every call the game makes into vendor upscaler, frame generation, latency and
presentation entry points, with the caller's module and RVA, the thread, the frame and the decoded
arguments. One game run answers where the DLSS, FSR, XeSS, Streamline and Reflex call sites are, how
often each runs per presented frame, on which threads, and with which resources and settings. It
replaces hand-tracing those call sites in a disassembler.

It uses the same discovery idea as OptiScaler (intercept the vendor exports) and was written
separately. `references/OptiScaler` was read for module names, export names and what identifies each
vendor path; no code was copied.

The census never changes behaviour. Every wrapper forwards the original arguments and returns the
original result, does no GPU work, and is `noexcept`. Reads of game memory run under SEH, so a bad
pointer in a description costs one record, not the game.

## Use

```c
#include "rescaleframe/vendor_census.h"

rsf_census_options options = {0};
options.struct_size = sizeof(options);
options.abi_version = RSF_CENSUS_ABI_VERSION;
options.output_dir_utf8 = "C:\\captures\\rdr2-census";
rsf_census_result r = rsf_census_install(&options); /* safe from DllMain */
```

Link `ReScaleFrame::VendorCensus` (static, C++20, MSVC only) and call `rsf_census_install` as early as
the process allows. For RDR2 that is the ASI plugin's `DLL_PROCESS_ATTACH`, which runs during CRT
startup after the image is decrypted.

| Call | Purpose |
| --- | --- |
| `rsf_census_install(const rsf_census_options*)` | Start recording. One install per process. |
| `rsf_census_flush()` | Drain the queue and rewrite `census-summary.json`. Blocks on disk I/O. |
| `rsf_census_get_counters(rsf_census_counters*)` | Counters for a UI or a test. |
| `rsf_census_shutdown()` | Flush, stop the writer, disable the detours. Optional. |

`rsf_census_options` carries `struct_size`, `abi_version` and, with zero meaning default:

| Field | Default | Meaning |
| --- | --- | --- |
| `output_dir_utf8` | current directory | Created when missing. |
| `byte_budget` | 64 MiB | Hard limit for `census.jsonl`. |
| `full_record_cap` | 32 | Full records per call site, then sampling. |
| `sample_interval` | 600 | After the cap, one full record in this many calls at that site. |
| `summary_period_ms` | 2000 | How often `census-summary.json` is rewritten. |
| `flags` | 0 | `RSF_CENSUS_FLAG_NO_STATIC_SCAN`, `_NO_DXGI`, `_NO_VULKAN`. |

`rsf_census_install` rejects a short `struct_size` (`INVALID_ARGUMENT`), a wrong `abi_version`
(`ABI_MISMATCH`) and a second call (`ALREADY_INSTALLED`). Extend the structures by appending fields and
bumping `RSF_CENSUS_ABI_VERSION`, as the other rescaleframe headers do.

### Install under the loader lock

Install does no `LoadLibrary` and waits on no other thread. It allocates, opens the output file,
starts the writer thread (which waits for the loader lock to be released before it runs), initialises
MinHook, detours `kernelbase!GetProcAddress`, `GetProcAddressForCaller` and the `LoadLibrary` family,
and detours the vendor exports of modules that are already mapped. MinHook enables detours by
suspending the other threads of the process, which does not wait on them but does mean a thread that
holds a lock another needs is paused for the duration. The static signature scan runs on its own
thread so it never delays the writer.

## What is intercepted

Two mechanisms bind a recording wrapper to a function, and the first binding of a name wins.

1. **Detour.** When a module with a vendor basename loads (or is already loaded at install), its
   known exports are detoured with MinHook. That covers static imports, where the game never calls
   `GetProcAddress`.
2. **GetProcAddress thunk.** A `GetProcAddress` for a known name returns a recording thunk when the
   function is not already detoured. `nvapi_QueryInterface`, the Vulkan `vkGet*ProcAddr` pair and
   Streamline's `slGetFeatureFunction` hand out thunks for the functions they resolve. Because
   `kernel32!GetProcAddress` forwards to `kernelbase!GetProcAddressForCaller`, both are hooked.

Graphics creation exports (`CreateDXGIFactory`/`1`/`2`, `D3D12CreateDevice`) retain the exact pointer
returned by the resolver, including during DLL initialization before the module-load callback.
They use entry detours instead of resolver thunks. Otherwise an overlay can cache a census thunk
as its original while the census forwards into that overlay, forming a recursive hook chain.
The GTA V Enhanced startup failure and its correction are recorded in
[renderer research](../../docs/research/gta5-enhanced-renderer.md). A stand-in DLL resolves these
exports during its own DllMain to verify pointer identity; the WARP fixture checks that native
graphics creation calls are still recorded.

DXGI/D3D12 COM methods have no export. They are detoured at the address a vtable slot holds
when the first factory, device, queue or swapchain appears.

| Group | Functions | Decoded arguments |
| --- | --- | --- |
| DXGI / D3D12 | `CreateDXGIFactory`/`1`/`2`, `D3D12CreateDevice`, `IDXGIFactory::CreateSwapChain`, `IDXGIFactory2::CreateSwapChainForHwnd`/`ForCoreWindow`, `IDXGISwapChain::Present`, `Present1`, `ResizeBuffers`, `ResizeBuffers1`, `SetColorSpace1`, `SetFullscreenState`, `GetFrameLatencyWaitableObject`, `ID3D12Device::CreateCommandQueue`, `ID3D12CommandQueue::ExecuteCommandLists` | swapchain description, backbuffer state, present flags and sync interval, colour space, queue type, priority and flags, command lists per queue |
| Vulkan | `vkGetInstanceProcAddr`, `vkGetDeviceProcAddr`, `vkCreateInstance`, `vkCreateDevice`, `vkGetDeviceQueue`/`2`, `vkQueueSubmit`, `vkQueueSubmit2`/`KHR`, `vkQueuePresentKHR`, `vkCreateSwapchainKHR` | application and engine, layers, extensions, queue families, batches and command buffers per submit, swapchain format, extent, usage, present mode |
| NGX | D3D12, D3D11 and VULKAN `Init`, `Init_Ext`, `Init_ProjectID`, `Init_with_ProjectID`, `Shutdown`, `Shutdown1`, `GetParameters`, `AllocateParameters`, `GetCapabilityParameters`, `DestroyParameters`, `CreateFeature`, `CreateFeature1`, `EvaluateFeature`, `ReleaseFeature` | feature id, `Width`/`Height`/`OutWidth`/`OutHeight`, quality, `DLSS.Feature.Create.Flags` by name; on evaluate the Color, Output, Depth, MotionVectors, TransparencyMask, ExposureTexture and bias mask resources with D3D12 or D3D11 descriptions (Vulkan image view data), jitter, MV scale, reset, sharpness, render subrect, pre-exposure, frame time and subrect bases |
| FSR 2.x | `ffxFsr2ContextCreate`/`Dispatch`/`GenerateReactiveMask`/`Destroy`, `GetJitterOffset`, `GetJitterPhaseCount`, `GetRenderResolutionFromQualityMode`, `GetUpscaleRatioFromQualityMode`, `ffxFsr2GetInterfaceDX12`, `GetScratchMemorySizeDX12`, `ffxGetResourceDX12`, `ffxGetCommandListDX12`, `ffxGetDeviceDX12` | all seven `FfxResource` entries (pointer, name, description, state, depth flag, D3D12 description) and every scalar of the 2.1.2 dispatch description |
| FidelityFX API (FSR 3.1, 4, frame generation) | `ffxCreateContext`, `ffxDestroyContext`, `ffxConfigure`, `ffxQuery`, `ffxDispatch` | the whole `pNext` chain by type; upscale create and dispatch, frame generation create, configure, dispatch and prepare V2, swapchain wrap, new and for-HWND descriptors |
| XeSS | `xessD3D12CreateContext`/`Init`/`Execute`, `xessSetJitterScale`, `xessSetVelocityScale`, `xessSetExposureMultiplier`, `xessDestroyContext`, the D3D11 and Vulkan create/init/execute | init parameters, execute resources and scalars |
| XeFG, XeLL | `xefgSwapChain*` creation, tagging and control; `xellD3D12CreateContext`, `SetSleepMode`, `GetSleepMode`, `Sleep`, `AddMarkerData`, `DestroyContext` | raw arguments; XeLL frame id and marker |
| Streamline | `slInit`, `slShutdown`, `slSetTag`, `slSetTagForFrame`, `slSetConstants`, `slEvaluateFeature`, `slGetNewFrameToken`, `slGetFeatureFunction`, `slIsFeatureSupported`, `slSetD3DDevice`, `slUpgradeInterface`, `slAllocateResources`, `slFreeResources`; feature functions `slDLSSGSetOptions`, `slDLSSGGetState`, `slDLSSSetOptions`, `slReflexSleep`, `slReflexSetOptions`, `slPCLSetMarker`, `slPCLSetOptions` | preferences, tags by buffer type with resource descriptions, constants including five matrices and camera axes, DLSS-G and DLSS options, Reflex options, PCL marker and frame index |
| NVAPI Reflex | `nvapi_QueryInterface`, then ids `0xAC1CA9E0` SetSleepMode, `0x852CD1D2` Sleep, `0xD9984C05` SetLatencyMarker, `0x1A587F9C` GetLatency, `0x13C98F73` D3D12 SetAsyncFrameMarker | sleep mode flags and interval, marker type and frame id, whether a latency report came back |
| NvLowLatencyVk | `NvLL_VK_Initialize`, `InitLowLatencyDevice`, `DestroyLowLatencyDevice`, `SetSleepMode`, `Sleep`, `SetLatencyMarker`, `GetLatency` | marker frame id and type (layout assumed) |
| AMD | `AmdExtD3DCreateInterface` (Anti-Lag 2 path), `agsInitialize`, `agsDriverExtensionsDX12_CreateDevice` | interface id, raw arguments |
| Static FSR / NGX / vendor code | string and xref scan of the main image | see "Static findings" |

Vendor module basenames that trigger detours: `ffx_fsr2_api_x64.dll`, `ffx_fsr2_api_dx12_x64.dll`,
`amd_fidelityfx_dx12.dll` and its loader, upscaler and frame generation variants,
`amd_fidelityfx_vk.dll`, `libxess.dll`, `libxess_dx11.dll`, `libxess_fg.dll`, `libxell.dll`,
`sl.interposer.dll`, `nvapi64.dll`, `nvlowlatencyvk.dll`, `amdxc64.dll`, `amd_ags_x64.dll`,
`nvngx.dll`, `_nvngx.dll`, `dxgi.dll`, `d3d12.dll`, `vulkan-1.dll`. A renamed or proxied vendor DLL is
still caught by name through the `GetProcAddress` thunk.

## Output

### census.jsonl

One JSON object per line, written by a separate thread from a bounded queue. Hooked calls only format
into a stack buffer and push to the queue; a full queue drops the record and counts it. The first line
is `{"type":"census_start",...}`. Every other line:

| Field | Meaning |
| --- | --- |
| `seq` | Dense line number, starting at 0 after the start record. |
| `t_us` | QPC microseconds since install. |
| `present` | Present index: Present, Present1 and `vkQueuePresentKHR` calls begun so far (test presents and nested presents excluded). A call made while frame N is built carries N-1. |
| `api`, `fn` | Vendor group and function. `fn` is the export name, or an internal name for vtable methods. |
| `ord`, `site_ord` | Call ordinal for the function, and for this call site. |
| `module`, `rva` | Basename of the module holding the return address, and the RVA inside it. |
| `tid`, `thread` | Thread id and `GetThreadDescription` text. |
| `ret` | Raw return register as hex. |
| `args` | Decoded arguments, see the table above. |

Per call site (function, caller module, RVA) the first `full_record_cap` calls are written in full,
then one in `sample_interval`. The budget is a hard limit: a record that would exceed it is dropped and
`budget.exhausted` is set. Counters and the summary keep counting.

Pointers are hex strings. Resources carry a `format_name` from the DXGI format table. Floats are
printed with `%.9g`.

### census-summary.json

Rewritten every `summary_period_ms` and on flush, through a temporary file and a rename.

| Key | Content |
| --- | --- |
| `process`, `pid`, `elapsed_ms`, `present_index` | Identity and progress. |
| `budget`, `counters` | Byte budget and use; calls seen, records queued, written and dropped, sites. |
| `sites` | `{api, function, module, rva, threads, count, first_present, last_present}` per call site. `threads` lists up to six thread ids. |
| `histograms` | Per function: `calls` and `calls_per_present`, a map from calls in one present interval to the number of intervals (`65+` collects larger). |
| `threads` | Every thread that made a recorded call: id, last description, call count. |
| `queues` | Per D3D12 or Vulkan queue: type or family and index, submit calls, command lists or batches, first and last present, calls-per-present histogram. |
| `hooks` | Every binding: function, API, `detour` or `getprocaddress_thunk`, module, MinHook status and `entry_modified`. |
| `modules_hooked`, `hook_conflicts` | Vendor modules detoured; second implementations left alone. |
| `static_findings` | See below. |

### Static findings

The census scans the main image for identifying strings (FSR2 and FSR3 resource names, NGX entry and
parameter names, vendor DLL names), finds the code that references each through a RIP-relative `lea`
or `mov`, and maps it to a function with the exception directory. Leaf functions have no directory
entry; their start is guessed from alignment and the preceding `ret` or `int3`, and the finding is
marked `[no unwind]`. Functions that reference several FSR2 resource names are reported as
`fsr_static` candidates for the context creation code. The NVAPI Reflex interface ids are searched as
32-bit immediates in code. A finding is a lead for a disassembler, not proof of an entry point; the
dispatch function of a statically linked FSR2 is found from the creation function, not directly.

## FSR 2.1.2 layout check

The dispatch description size is not passed to `ffxFsr2ContextDispatch`, so the decoder reads 1352
bytes and refuses to decode when any of these checks fails, writing `layout_check: "failed"`, the
reason and `raw_bytes_len`:

- at least five of the seven resources have a printable ASCII name that ends within 64 characters, a
  type of at most 3, a format below 64, extents of at most 16384 and a state below `1 << 16`;
- the color and output resource pointers are non-null;
- jitter is finite and within 16, motion vector scale finite and within 1e6;
- the render size is 1 to 16384 in both dimensions;
- both bool bytes are 0 or 1; sharpness, frame time, pre-exposure, near, far and field of view are
  finite and in generous ranges.

FSR 2.2 starts with the same fields and should pass the check; that is not tested. FSR 2.0 is not
covered.

## Limits and known behaviour

- **First binding wins.** Each wrapper has one slot for the function it forwards to. A second
  implementation of the same name (a device-level Vulkan pointer after an instance-level one, two
  NGX DLLs in one process) is passed through unrecorded and counted in `hook_conflicts`.
- **Overlays can displace a detour.** RTSS, Steam, Discord and similar hook the same DXGI methods. When
  one is installed in front of the census, the caller module of a record is the overlay's hook, not the
  game. When one overwrites the census detour at the function entry, calls bypass the census. The
  summary compares each entry with the bytes written at install and reports `entry_modified`. The
  census does not fight back. This happened on the development machine with RTSS: the Present detour
  was replaced after the first frame.
- **Tail calls hide the call site.** A caller that jumps to a vendor function instead of calling it
  makes the return address the caller's caller. This is what the CPU does; the record is accurate for
  that return address.
- **Present index is a count, not a frame id.** It counts presents begun on any swapchain or queue.
  Frame generation proxies that present internally are counted once per outermost call on a thread.
- **Per-present histograms race at the boundary.** A call on another thread near a present can land in
  either interval.
- **Memory.** About 11 MB is committed at install: the record queue (6.4 MB), the site table, the
  summary buffer (4 MB) and the file buffer. The site table holds 8192 sites; calls from further sites
  are counted (`site_overflow`) but not listed.
- **Hooking pauses other threads** while MinHook enables detours. On unusual processes that can
  deadlock if a paused thread holds a lock the hooking thread needs.
- `rsf_census_shutdown` disables the detours but leaves trampolines mapped; unload the census module
  only after the process no longer runs inside them.
- Only the main image is scanned for static findings, and only on x64.

## What was verified, and how

Build: MSVC, `/W4 /WX`, Release and Debug, Visual Studio 2026 generator. The tests are synthetic:
fake vendor DLLs named like the real ones, and one real DXGI and D3D12 path on the WARP adapter.
Nothing here has run inside a game.

`ctest -R vendor_census`:

| Test | Covers |
| --- | --- |
| `vendor_census` | FSR2 through a direct import, `GetProcAddress` and from two threads (pass-through of arguments, return values and a float return; decoded resources and scalars; layout check refusal), NGX through `LoadLibrary` (CreateFeature flags, EvaluateFeature resources and scalars, the parameter vtable slots), `nvapi_QueryInterface` thunks for Reflex ids, XeSS float arguments and execute parameters, real DXGI and D3D12 on WARP (factory, device, queue, `CreateSwapChainForHwnd`, Present, Present1, test present, `SetColorSpace1`, `ResizeBuffers`, `ExecuteCommandLists`), the vtable slot numbers against the Windows SDK headers through a fake vtable, caller module and RVA of the test executable, call and site counts, calls-per-present histograms, sampling (4 full records then one in 10), thread names, dense sequence numbers, the static scan finding a function that references `FSR2_InputColor` |
| `vendor_census_budget` | A 40000 byte budget under 5000 calls: the file stays under the budget, drops are counted, the summary still counts every call |
| `vendor_census_vulkan_streamline` | Vulkan through `vkGetInstanceProcAddr` and `vkGetDeviceProcAddr` (thunks, create instance, device, queues, submits, present, swapchain, per-queue per-present histograms), Streamline `slInit`, and `slGetFeatureFunction` handing out thunks with a fake frame token |
| `vendor_census_fidelityfx_api` | `ffxCreateContext` chains (upscale, DX12 backend, swapchain for HWND), upscale and frame generation dispatch, configure, query, destroy |
| `vendor_census_dllmain` | Install from `DllMain` of a plugin DLL with the vendor DLL already mapped |
| `vendor_census_layouts_{sl,ffx,xess,fsr2,vk}` | The offsets the decoders use against the real Streamline, FidelityFX API, XeSS, FSR 2.1.2 (OptiScaler checkout) and Vulkan headers. Built only where the untracked headers exist; otherwise skipped |

Where the test environment disagrees with the idealised run: RTSS was injected into the test process
on the development machine. The test accepts a missing Present record only when the summary flags the
entry as modified, and prints a note. The strict path (three Present records, exact histograms) has
not been observed on this machine.

### Implemented but not exercised by a fixture

NGX D3D11 and Vulkan wrappers, the Vulkan NGX resource decode (offsets assumed from `nvsdk_ngx_vk.h`
and flagged `layout: assumed`), `NvLL_VK_*`, XeFG, XeLL, `AmdExtD3DCreateInterface`, AGS,
`ffxGetResourceDX12` and the FSR2 create and interface functions, the NVAPI struct layouts (assumed
from `nvapi.h`, which is not in the tree, and flagged), Streamline constants, tags and DLSS options
(layout checked against the headers, behaviour not run), and `vkGetDeviceProcAddr` against a real Vulkan
loader. Anti-Lag 2 beyond the `AmdExtD3DCreateInterface` call and the interface id is
not decoded.

## Reading a census for a new game

1. `modules_hooked` and `hooks` say which vendor DLLs and exports were seen. A vendor missing there
   may be linked statically; look at `static_findings`.
2. `sites` lists every call site with module, RVA and threads. Sort by `count` for the frame loop.
3. `histograms` show calls per present. A function that runs twice per present on one thread and once
   on another has two sites.
4. `queues` show which queue carries the work and how many submits each present needs.
5. Open `census.jsonl` for the decoded arguments of the first calls at each site.
