# Reusable SR orchestration, 29 September 2026

Research history: findings, hook addresses and pending statuses below apply to their recorded
experiments. Later increments can supersede earlier conclusions. See
[current implementation and validation](../current-status.md) before using this as a feature list.

## Question and approach

Can the working D3D11 reconstruction integration select FSR2, FSR3, FSR4 and XeSS at runtime,
without putting backend lifecycle or graphics transfers into a game plugin?

Use the existing vendor-neutral C contract and SDK frame record. The fixed runtime owns provider
contexts, selection, version queries, history continuity and synchronized GPU transfers. A plugin
owns hooks, matching view data, game input conventions, renderer preparation and reinsertion.
No new game addresses, executable patches or reverse-engineered structures were introduced.
The existing AC7 build and accepted DLSS evidence are in [the consumer-session notes](ac7-consumer-session.md).

The direct `sr_session` API consumes prepared D3D12 resources. `sr_bridge` adapts prepared D3D11
resources on the same adapter. The legacy pipeline is a compatibility caller of those APIs.
It is not the public plugin lifecycle migration specified in the representation plan.

## Reference revisions and findings

- AMD FSR SDK `60f4ea81909200d8542eca14dccb2628b763a9a3`, SDK 2.3.0. The old dependency-table
  description of this revision as an FSR3.1-only package was incorrect. Its signed upscaler DLL
  offers FSR 2.3.4, FSR 3.1.5 and hardware-dependent FSR4 providers through the versioned FFX API.
- Intel XeSS SDK 3.0.2, `8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0`. Its SR DLL reports XeSS
  2.0.2. SDK/package version and the SR implementation version are separate facts.
- Streamline SDK 2.14.1, confirmed from both compiled and runtime-package `sl_version.h`.
  The extracted release package has no independent Git checkout revision. The existing local
  retail runtime was used by the compatibility fixture.
- OptiScaler `92337ebfbf07d533eabe3731fd8de1bb19437aea`, cloned untracked as a reference at the
  user's request. Inspected `IFeature.h`, `IFeature_Dx11wDx12.cpp` and
  `inputs/NVNGX_DLSS_Dx11.cpp`: feature lifecycle is separate from API input handling; backend
  changes happen during evaluation; command allocators are retired with GPU fences before reuse.
  ReScaleFrame retains its own C ABI and ownership split. No OptiScaler code was copied.

AMD's [version-selection contract](https://gpuopen.com/manuals/fsr_sdk/getting-started/ffx-api/)
requires IDs from a device-specific query and allows checking the actual created provider.
The implementation queries paired IDs/names, copies names, chooses an explicit major family,
applies a version override and verifies the created provider ID. It never obtains a default FSR3
context while claiming FSR4. Separate runtime directories for each FSR family permit different SDK
releases to coexist; the tested current SDK contains all three algorithms in one package.
Only releases with the required versioned API entry points are supported by this adapter.

Intel's [SR guide](https://www.intel.com/content/www/us/en/developer/articles/technical/xess-sr-developer-guide.html)
and the pinned `xess_d3d12.h` define create/init/execute, quality planning, pixel motion scaling and
resource states. Native XeSS D3D11 is not implemented here. Both new adapters advertise SR only:
FG and latency are not implemented and cannot be selected through their capabilities.

## Runtime use and ownership

`sr_session_select` opens and plans a replacement before closing the current provider. Open,
version or planning failures preserve the current backend. Status separates requested/effective
backend, last switch result, SDK provider version and frame result. Selection, GPU calls and
teardown run on the graphics owner thread; a frontend queues commands to that boundary.
Direct D3D12 callers must complete GPU work before selection or destruction.

Inputs use one SDK frame/view record, origin-zero valid rectangles and `ONLY_NOW` lifetime.
Generation mismatches, duplicate/out-of-order frame IDs, non-finite scalar inputs and unsupported
output sizes are refused. A new backend, frame gap, rejected evaluation, view/session change or
surface generation change resets history on the next accepted frame. Backend handles and strings
do not cross the C boundary with ambiguous ownership.

The D3D11 bridge creates D3D12 on the game's adapter, copies input rectangles into shared NT-handle
surfaces and signals a shared fence. The D3D12 queue waits, transitions inputs to compute-read and
evaluates into its own UAV output. It copies output into shared memory, transitions resources back
to COMMON, signals completion and returns the result to the caller's D3D11 output texture.
GPU completion precedes allocator reuse, surface replacement and successful teardown. A device
failure poisons the bridge. If completion times out while the device remains live, teardown retains
the failed bridge's resources until process exit instead of freeing descriptors under pending work.
This is intentionally a synchronous SR baseline. It has CPU/GPU serialization cost; no performance
or latency improvement is claimed.

The generic motion resolve accepts a decoded sparse field, a plugin-specified sentinel,
decoded-to-pixel scales and a current-clip-to-previous-clip matrix. Valid written motion replaces
camera displacement. Unwritten pixels use depth reprojection. It outputs dense previous-minus-
current displacement in render pixels and R32 device depth. It preserves graphics bindings.

The compatibility adapter assumes current-minus-previous NDC in the older decoded field: horizontal scale
`-render_width/2`, vertical scale `render_height/2`, with valid written values replacing camera
motion. It assumes the existing reversed, infinite-depth HDR path. Its frame ID counts accepted
input calls and its frame time is measured between calls with QPC, clamped to 0.01..1000 ms.
It uses auto exposure with pre-exposure 1. These are compatibility limits, not newly measured AC7
engine timing/exposure or proof of AC7 object-motion direction. New integrations should supply the
full SDK record and prepared inputs directly. The legacy and SDK camera types had conflicting
names with different layouts; the legacy type is now `rsf_pipeline_camera_frame`.

The carrier supplies AC7's engine identity and centimetre-to-metre factor. The reusable runtime
no longer embeds the AC7/UE4.18 identity. Existing AC7 startup still starts DLSS first; once running,
the overlay switches backends through the compatibility selector. Direct sessions/bridges can
start FSR or XeSS without DLSS or NVIDIA hardware. The DLL/plugin preparation lifecycle and an
AC7 initial-backend preference are separate remaining work.

## Experiments, failures and checks

Windows MSVC 19.51 / Visual Studio 2026 was used. Device fixtures do not access a game installation.

| Fixture | Observation |
| --- | --- |
| Controller with deterministic providers | Replacement rollback, copied paths, exactly-once close, reset after gaps/refusals/surface changes, duplicate/stale/non-finite rejection and disable passed |
| Motion resolve on WARP | Valid zero remains zero; written vectors replace camera motion; a +0.25 NDC shift produces +1 horizontal pixel at an 8-pixel extent; depth remains 0.5; non-finite transform is refused |
| D3D11/D3D12 SR bridge on Intel UHD Graphics, device `4688`, driver `32.0.101.7088` | FSR2 → FSR3 → XeSS → FSR2, three frames per selection, native and reduced render rectangles, succeeded |
| Same bridge on RTX 4070 Laptop, device `2860`, driver `32.0.16.1692` / 616.92 | Same switch sequence and evaluations succeeded |
| Compatibility pipeline on RTX 4070 Laptop | DLSS → FSR2 → FSR3 → XeSS → DLSS, three evaluations per selection at 512x512, and shutdown succeeded |
| FSR4 on the tested adapters | Version query supplied no selectable FSR4 implementation; selection returned NOT_SUPPORTED and preserved FSR2 |

The bridge fixture copies the returned output to staging and checks its centre red value against
the constant 0.25 input. Observed values were 0.249390..0.250244. This proves numeric output and
the transfer on these fixtures, not temporal quality, moving objects, reactive-mask handling or
game reinsertion. The compatibility fixture checks SDK evaluation return values; it is not an
AC7 run or a DLSS visual comparison. Hardware fixtures are opt-in and skip in the ordinary gate.

Two implementation findings were corrected during testing:

1. FSR2's quality query rejects `NATIVEAA`, though its dispatch accepts a 1:1 render rectangle.
   Native FSR2 sizing therefore returns the explicit output dimensions; other presets use the
   provider's query. Native and Performance evaluations then passed on both tested adapters.
2. The new shader compiler loader initially unloaded the compiler before releasing its blobs,
   whose vtables lived in that module. The fixture faulted. Both blobs are now released before
   unload. The numeric check also distinguishes signed half-float zero from a nonzero vector.

The Release `eng/verify.ps1 -VS2026` gate passed with SDKs present: 26 native tests passed and
the two opt-in hardware fixtures skipped. `cargo test --workspace --locked` passed 52 tests,
including backend intent and ABI-layout checks. Header-absent provider/controller checks are
passed in a separate MSVC Release build, including both providers' NOT_COMPILED versus missing-DLL
behavior. The hardware fixtures also passed against the final Release binaries. Build verification
never deploys into AC7.

SHA-256 of the unmodified runtimes used by the fixtures:

| Runtime | SHA-256 |
| --- | --- |
| `amd_fidelityfx_upscaler_dx12.dll` | `d0dcccc74a43c44ba435b7a369b456e0970d8a4464e4bd683119b374f2c9fb46` |
| `libxess.dll` | `251659dd84a3e84de67c886a4186e01f3eca49b00641906fe38bb6b807e5d5b7` |
| `sl.interposer.dll` | `8c87c9499461da561edd529aa9bf7831d67d7b94ebb1c1a5ed54ef4934e1ea4c` |
| `sl.dlss.dll` | `73bf52c0cfaa5900a8f3f4a91306e4625e7cca696dfb305aae44c9f97b582e1f` |

FSR4 evaluation remains untested without compatible hardware. AC7 switching, independent object
motion, camera cuts, mission transitions, resize, exposure, visual quality and overhead need game
validation. The accepted DLSS game result is not extended to FSR or XeSS by these fixtures.
