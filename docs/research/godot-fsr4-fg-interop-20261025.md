# Godot FSR4 frame-generation interop, 25 October 2026

The question is whether public FG backend 4 can use a real FSR4 provider with a
Vulkan engine. The existing Vulkan dispatcher rejected backend 4 unconditionally.
FSR4 upscaling through an INT8 compatibility lease is separate from ML frame
generation and does not answer that question. The adapter must discover exact
family 4 before retiring a working presentation owner; it must never label FSR3
fallback as FSR4.

## SDK source and admission

The inspected FidelityFX SDK is 2.3.0, revision
`60f4ea81909200d8542eca14dccb2628b763a9a3`. Its README lists ML frame generation
4.0.1. `Kits/FidelityFX/docs/techniques/frame-interpolation-ml.md` specifies
Windows 11, DirectX 12 Agility SDK 1.4.9 or later, and an AMD 9000-series GPU or
later. It requires `ffxConfigure`, `ffxDispatch` with PrepareV2, then frame
generation dispatch, in that order. PrepareV2 must contain world camera position
and normalized up, right and forward vectors. The proxy swapchain API is 3.1.7.

The public frame-generation header reports API version 4.0.1. This does not prove
that the loaded DLL exposes an algorithm provider of family 4. The existing
`runtime/backends/fsr/src/fsr_generation.cpp` already separates these concepts:
query versions for the actual DX12 device, select a provider by its paired
version name, force its opaque version ID, and verify the created context's ID.
That existing code and the SDK call-order documentation informed this adapter.

The new private bridge repeats this admission sequence. IDs are opaque and no
bit mask guesses their family. Both enumeration and post-create verification
must succeed for family 4. The HWND start function also requires the verified
admission state. There is no vendor-name blacklist and no use of the upscaler
compatibility hook. An absent family returns `RSF_FSR4FG_NOT_SUPPORTED` (`-13003`).

`RsfFsr4FgProbe` creates a matching DX12 device and queries providers, without
SDK context creation, command queues, rendering or an HWND. The native
`rsf_fsr4gvk_probe` obtains the Vulkan physical device's Windows LUID and passes
it to that probe. It uses `setup.runtime_directory_utf8` for FidelityFX, never
the host's Intel-directory field. The host can call it while the current owner
remains live and reject an unsupported request before retirement. Generic Godot
routing and this integration's final build are coordinated separately.

## Private owners and resource flow

New code lives in:

- `runtime/vulkan/fsr4-fg/bridge_api.h`, `bridge_contract.h` and `bridge.cpp`.
- `runtime/orchestrator/src/native_fg_fsr4_vulkan.h` and `.cpp`.
- `runtime/orchestrator/src/native_dx12_vulkan_interop.h` and `.cpp`.

The bridge owns the matching DX12 device, exact SDK context, SDK DXGI presenter,
command storage and completion fences. Calls use size/ABI-checked C structures;
SDK structures, STL allocations and ownership-ambiguous strings do not cross
the boundary. Create descriptors remain in the owner through context destruction.

The native broker exposes three ordinary Vulkan images through its private WSI
handle. This handle is never sent to native Vulkan WSI. It copies submitted
engine depth and motion into per-source shared resources before SR, records the
output-sized HUD-less image after postprocessing, and copies the completed
presentation image at Present. The initial path accepts SDR RGBA8/BGRA8 output,
R32F normalized depth, RG16F motion, exclusive queue-family ownership, and one
generated image. Unsupported formats, count changes or layouts refuse explicitly.

Every shared resource is a dedicated `D3D12_RESOURCE` NT-handle import. Actual
Vulkan and DX12 adapter LUIDs must match. One permanently imported D3D12 fence is
used as a bidirectional Vulkan timeline. Vulkan waits for previous `done`, copies
and releases ownership to `EXTERNAL`, then signals `ready`. DX12 waits for
`ready`, consumes the inputs, restores shared resources to COMMON and signals
`done`. Caller submission callbacks retain the engine's queue serialization.
Frame/view identity does not substitute for successful command submission.

The FSR4 presenter configures every source before PrepareV2. It forwards jitter
in render pixels, the supplied motion-to-pixel scale, frame duration in
milliseconds, depth conventions, near/far planes and vertical FOV in radians.
Camera vectors follow the existing D3D12 adapter: position comes from
`view_to_world[12..14]`; Godot's right-handed perspective uses negative view Z,
so its forward vector negates row 2 according to the projection W/Z term.
Reset remains an SDK history reset and does not disable generation by itself.

HUD-less resources remain in compute-read state until the SDK's generation and
presentation work retires. The initial path conservatively calls the SDK's
wait-for-presents operation and waits for local queue completion before restoring
COMMON and returning shared ownership. This can serialize presentation; no
performance or lower-latency acceptance is claimed. AMD timing and image proof
are still required before relaxing this lifecycle.

An Off configuration clears stale generated-dispatch error state only after
successful SDK configuration. Unknown GPU completion is terminal. The bridge
publishes that condition through present status; the Vulkan broker then refuses
acquire/drain/destruction and retains all owners. It never signals fabricated
completion or frees potentially live imports to make teardown appear successful.

## Shared importer factoring

The former XeSS-specific import implementation is now the common
`native_dx12_vulkan_interop` owner. Its copied C callback table supplies adapter
LUID, shared image creation, image release and shared fence creation. The SDK
bridge, callbacks and device outlive checked interop destruction. NT handles are
closed after import, while Vulkan and DX12 payload references remain owned until
both queues have drained.

The existing XeSS import API is a thin wrapper with its original error codes and
validation order. Short/null setup and ABI mismatch refuse before any SDK/LUID
callback. XeLL sleep and marker requirements remain in the XeSS presenter and
are not imposed on FSR4. Shared callback failure paths close returned NT handles
and release partial COM resources. XeSS's positive device evidence from before
this factoring is not counted as a fresh regression run of the common helper.

## Evidence and limits

The pinned signed frame-generation DLL SHA-256 is
`02297BEEDD285E822D3A64F314CF00FAF378DCEC0EDC47FF0C4DD71B3A8C2F18`.
The loader is
`E2D85AA05A9BD9ED8B38935FDF5199372CCA6F74C12015143BB6F945EE1608AA`.
CPU null-device queries against both return success and only provider 3.1.6,
ID `0xf600000000c01006`. The null-device result alone cannot establish which
driver providers an AMD device exposes.

Actual matching-device queries on the current NVIDIA hardware also return
success and only 3.1.6. Both enumerated hardware identities report vendor
`10de`, device `2c02`, with LUIDs `117213` and `230205413`. The new probe returns
`-13003`, clears provider/maximum outputs, and Create also refuses with a null
owner. No HWND was supplied and presentation was not replaced. These are device
capability queries, not frame-generation execution or image acceptance.

Compact evidence is retained under `.local/godot/`:

- `fsr4-fg-provider-null-query-20261025.json`.
- `fsr4-fg-device-provider-lists-20261025.jsonl`, including raw SDK query result,
  provider name and ID for each actual device.
- `native-fsr4-fg-nvidia-admission-20261025.log`.
- The isolated query executable SHA-256 is
  `304F29EC6E1578B815CC5481F95CAABD51FEE49773B0CB05289D72DE7992512C`.
  The admission fixture executable used for that device run is
  `05A4C8D1017B12C8A7881F21F3F0014B4049FF55DFB4C1CAB19C7C7AF4E87FCC`.
  Later terminal/Off guard corrections do not change provider discovery.

All new implementations compile with Windows clang++ in C++20 mode, with
`-Wall -Wextra -Werror` and vendor aggregate-initializer warnings excluded. Four
focused CPU fixtures pass:

- `native_dx12_vulkan_interop.cpp`: LUID mismatch, ABI refusal, timeline import,
  dedicated image allocation, partial-failure cleanup, closed NT handles,
  required two-queue drain proof, and XeSS prevalidation before SDK callbacks.
- `native_fsr4_fg_contract.cpp`: FSR3-only refusal, exact newest family-4
  selection, actual-context mismatch, query errors/capacity, camera handedness,
  units, source gaps and explicit history resets.
- `native_fsr4_fg_bridge.cpp`: public private-bridge argument/ABI/count guards;
  its opt-in device query is the NVIDIA evidence above.
- `native_fsr4_fg_state.cpp`: actual Off/error state transitions and terminal
  bridge/native-owner retention, using CPU SDK callbacks and no COM operations.

This checkpoint establishes implementation, synthetic guards and honest negative
NVIDIA admission. AMD 9000-series execution, actual FSR4 generated images,
presentation quality, performance and positive hardware lifecycle acceptance
remain untested. The coordinated MSVC Release gate passes 74 discovered tests,
63 executed and 11 explicit opt-in skips, plus Rust formatting and Clippy.
Its log is `.local/godot/verify-godot-fg4-lifecycle-retry.log`. Native DLL SHA-256
is `E2EE9675BFE9A7958F8E261E7181285EB973C4494605663F632B8448C2E6FC5D`.
This includes the generic FG4 dispatch and Godot preflight before retirement of
the working owner; public ABI1 structure layouts remain unchanged. Fresh XeSS
importer regression is a separate gate. A source review also found that Streamline's loaded-plugin
requirements can include unsupported adapters' mandatory extensions; selected
device filtering is owned by the parent integration work. AMD startup must not
be claimed until that path is verified.
