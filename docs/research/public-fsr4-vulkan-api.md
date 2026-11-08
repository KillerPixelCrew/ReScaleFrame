# Public FSR4 Vulkan recording API, 8 November 2026

The public `native_sr_vulkan.h` ABI 1 can evaluate a single Vulkan command
buffer. FSR4 already has a working Vulkan/DX12 shared-resource implementation,
but it needs a producer/continuation split and was only reachable through a
private C++ service. Passing FSR4 to the ABI 1 selector cannot express that
handoff. The repair exposes that existing mechanism through a separate C ABI;
it does not route FSR4 through the single-buffer selector.

Source baseline: `150ff92f5f0e00fa1146828818faf5f781774b00`. The implementation
owners are `native_sr_fsr4_vulkan.cpp`, `runtime/vulkan/fsr4/bridge.cpp` and the
new public facade `native_sr_split_vulkan.cpp`. The existing
[interop research](godot-fsr4-interop-20261025.md) and
[graph handoff research](godot-fsr4-graph-handoff-20261025.md) explain the
resource and submission mechanism. This change uses source inspection and
native fixtures, with no new executable reverse engineering.

## Contract

`rescaleframe/native_sr_split_vulkan.h` exports
`rsf_srvk_fsr4_create`, `rsf_srvk_fsr4_plan`, `rsf_srvk_fsr4_evaluate` and
`rsf_srvk_fsr4_destroy`. The opaque owner and handoff layout match the existing
private service. The new handoff version is `RSF_SRVK_SPLIT_ABI_VERSION = 1`.
Setup/frame structures retain `RSF_SRVK_ABI_VERSION = 1`. Short structures fail
before version reads; versions must match; appended fields are accepted.
The C header is compiled as C11 and the facade is linked by an actual DLL
consumer, in addition to the static embedded-host route.

Create requires the FSR4 backend, enabled timeline capability, actual queue
family, callback, signed runtime directory and matching Vulkan device dispatch.
The existing provider performs adapter LUID, external resource/semaphore,
memory-type and exact-provider checks. Its existing signed INT8 compatibility
admission is preserved. There is no new vendor/device whitelist, provider
substitution or native SR ABI change. Validation reports backend errors;
other Vulkan, HRESULT and FidelityFX results remain opaque native nonzero
values. Code must test zero/nonzero rather than infer a native error domain.

All calls are serialized. Setup strings are used only during create; device,
callback code and user-data leases last through successful checked destruction.
An owner retained by failed creation is for cleanup only. Plan/evaluate reject
that incomplete owner. The DLL compiles its tiny facade directly and links the
static service normally, retaining the four public exports. The embedded FSR4
bridge suppresses its legacy exports; the standalone bridge keeps them.

Each owner has one stable session/view and increasing source frame IDs.
Resource-generation changes reset history. Recreate after draining for a new
view, resolution or quality. Images are borrowed whole 2D level-zero,
single-sample resources: RGBA16F color/output, R32F depth, RG16F motion. Inputs
promise transfer-source usage and shader-read layout; output promises
transfer-destination usage/layout. Nonempty reactive/composition masks refuse
rather than being ignored. Runtime-owned interop images and the imported
timeline remain private.

Evaluate copies and releases inputs in the producer, then asks the host to end
that buffer and return a different recording continuation. At normal submission
the producer waits previous-done and signals ready; the continuation waits done.
Wait stages cover all commands. The DX12 provider waits ready and signals done,
including provider evaluation failure. Successive frames and both sections
submit in order on the same host queue. Submit each frame before evaluating the
next instead of accumulating unsubmitted
frames that could exhaust the existing three DX12 allocator slots.
A successful split always returns the continuation so fallback can be recorded
there without losing either section.
Host images and both graph sections remain leased until GPU completion.
Validation before recording returns the original buffer. Callback failure is
terminal and now clears any callback-written pointer rather than accidentally
offering an ended producer for reuse. Neither buffer can be reused after that
failure, and the owner remains retained. Both successfully leased sections must
be submitted before draining and checked destroy. Failed retirement preserves
the owner; cleared released handles make a later cleanup retry safe.

## DLSS and VR boundary

The current `native_dlss_vulkan.cpp` has process-wide single-owner admission and
a fixed Streamline viewport 1 for SR. Its history tracks view changes by resetting
one history; that is not stereo history. The generic facade also requires
increasing source frame IDs, so a second eye with the same frame ID cannot be
evaluated on that owner. `native_latency_vulkan.cpp` owns shared per-source-frame
tokens, and FG owns viewport 0 and final presentation separately.

The SDK calls already accept viewport handles, and `rsf_slvk_invoke` can retrieve
the same source-frame token for multiple calls. A generic extension can therefore
allocate a distinct SR viewport per opaque owner, reserve viewport 0 for FG, and
retire each owner's SDK resources separately without game policy or necessarily
changing SR ABI 1. It needs real multi-owner, shared-token and two-view validation;
the limitation is current runtime ownership, not evidence of a vendor/device
stereo restriction. Removing the admission flag while retaining viewport 1, or
alternating eyes on it, would share one history and is not a fix. This change
leaves DLSS, FG, latency and provider selection behavior intact. Multiple
independent FSR4 view owners were not tested.
This work does not establish stereo performance, VR presentation, FG acceptance
or headset/device quality.

## Validation

The contract suite checks the real DLL imports, C11 declarations, short and
wrong-version setup/handoff/frame/record/camera structures, absent timeline and
callback capability, reserved queue families, unsupported provider/masks,
explicit not-compiled behavior, provider-error continuation, retained failed
creation/destruction owners, exception containment and cleanup retry.
Stub-provider checks are synthetic and do not establish GPU execution.
`rsf_native_sr_fsr4_vulkan_test --device <signed-runtime>` now calls the public
create/plan/evaluate/destroy route for the existing six-frame GPU fixture.
Executed Windows VS2026 Debug results:

- `eng/verify.ps1 -VS2026`: passed, 102 discovered / 91 executed / 11 optional
  skips; C11 header compilation, fmt and Clippy passed. No Rust behavior changed.
- Focused final four FSR4/split contract tests: passed after rebuilding the
  fixture with explicit discrete-GPU preference. `dumpbin /imports` confirms
  all four calls imported from `ReScaleFrame.Runtime.dll`; `/exports` confirms
  all four exports and no private/legacy FSR4 bridge exports.
- An initial first-adapter attempt refused create with raw result `-11`, zero
  splits and successful cleanup. Vulkan enumeration lists Intel UHD Graphics
  770 first; the raw error's precise origin was not localized. Its failed receipt
  remains in `build/public-fsr4-device/receipt.json`. It is not a passing Intel
  acceptance result or evidence of an API/device restriction.
- The discrete-GPU attempt passed six 426x240 to 640x360 public API evaluations
  and reinsertion copies on NVIDIA RTX 4070 Laptop GPU, vendor 4318/device 10336.
  Six duplicate frames and invalid transfer-source inputs were refused; changed
  views were refused before recording. Create, evaluate and destroy returned 0.
  Provider `17700776140664213505` used the existing INT8 compatibility lease.
  Khronos core/synchronization validation was enabled with zero errors.

The passing receipt is `build/public-fsr4-device-discrete/receipt.json`, timestamp
`2026-11-08T17:36:55.5883151Z`. Fixture SHA-256 is
`342A40E0B13D8599F94E4B4F2CBDED520A0268D43E04CFC330327738215E2B9C`;
signed runtime SHA-256 is
`D0DCCCC74A43C44BA435B7A369B456E0970D8A4464E4BD683119B374F2C9FB46`.
Verification logs are `build/verify-public-fsr4-final.log` and
`build/public-fsr4-device-discrete-run.log`. The bounded device runner used the
existing process-only extension-point policy (effective 1), with a process-local
hidden/no-console launch flag; no persistent setting or helper source changed.
This is synthetic GPU execution and ordering evidence, with constant images and
no pixel readback. It establishes neither visual quality nor overlay/RTSS,
application, VR, FG or headset acceptance. No game was launched or deployed.
