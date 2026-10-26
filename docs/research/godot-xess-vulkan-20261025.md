# Godot native Vulkan XeSS SR, 25 October 2026

The question was whether the pinned XeSS SDK could execute native Vulkan SR in
the Godot developer integration, and which requirements must reach Vulkan device
creation before the first SR context. This investigation starts at repository
revision `d3250986dea7fddd8e866b903a2129ef68005610`. Engine and .NET acceptance
remain pending at this checkpoint.

The approach uses the SDK's official Vulkan requirements and execution APIs,
existing native SR setup/frame ABI 1, and private images. This preserves the
engine's submission owner and avoids another graphics API or an enlarged frame
ABI. Evidence comes from local pinned headers, source samples, isolated instance
queries and a standalone RTX 5080 device fixture. Source inspection is distinct
from that device execution.

The vendor checkout is package tag `v3.0.2`, revision
`8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0`, matching
`integrations/godot/dependencies.json`. The SR DLL reports **XeSS 2.0.2.0**;
package 3.0.2 is not the SR component version. Capture version ID
`562949953552384` uses the existing XeSS packing:
`major << 48 | minor << 32 | patch << 16 | reserved`.

| Artifact | SHA-256 |
| --- | --- |
| Original `vendor/xess/bin/libxess.dll` | `251659DD84A3E84DE67C886A4186E01F3ECA49B00641906FE38BB6B807E5D5B7` |
| Device-tested `rsf_native_xess_vulkan_test.exe` | `BEBBDA682FD104B885963662BA390C32CEDF9C0AD62C899AC221936992D5D687` |
| Explicit Khronos validation DLL | `E86FA4F70676A120F00A32BEAC6979CEB3CAFCE70DE42FA9107C89F587AB206E` |

`vendor/xess/inc/xess/xess_vk.h` documents the instance/device requirement
queries, SDK-owned extension/feature storage and writable feature-chain patching.
The Vulkan sample's `triangle.cpp` calls those queries before logical-device
creation. The current SR guide describes current-to-previous pixel motion,
velocity scaling, optimal-resolution queries and application-owned Vulkan
synchronization. Its older minimum-Vulkan prose says 1.1; the actual pinned DLL
returns **Vulkan 1.3**, which is authoritative for this payload.

The initial instance-only query is retained in
`.local/godot/xess-vulkan-instance-requirements.json`: success, minimum API
`4206592`, no extra instance extensions. The later isolated script
`query-xess-features.py` creates an instance, selects the RTX 5080 physical device
and queries requirements with a null feature chain. It creates no `VkDevice` or
queues. Its receipt is `xess-vulkan-device-feature-requirements.json`, under
`.local/godot`. The device requires `VK_EXT_mutable_descriptor_type` and returns:

| sType | Structure | Enabled members |
| --- | --- | --- |
| `1000059000` | `VkPhysicalDeviceFeatures2` | `shaderStorageImageWriteWithoutFormat` |
| `1000351000` | `VkPhysicalDeviceMutableDescriptorTypeFeaturesEXT` | `mutableDescriptorType` |
| `51` | `VkPhysicalDeviceVulkan12Features` | `shaderInt8`, `scalarBlockLayout` |
| `53` | `VkPhysicalDeviceVulkan13Features` | `shaderIntegerDotProduct` |

Every other Boolean in those returned structures is zero. `scalarBlockLayout`
is an additional measured requirement beyond the older guide's short list.

Godot's device-creation path uses `pEnabledFeatures` and individual promoted
feature structures, including float16/int8, buffer device address, memory model,
timeline semaphore and pipeline cache control. A second instance-only query
reproduced that shape with writable caller-owned structures. XeSS preserves
existing caller bits and patches the existing int8 member, but appends Vulkan
1.2/1.3 aggregate structures for the other missing members. These aggregates
conflict with the caller's individual promoted feature structures. Passing the
unprepared Godot chain directly to the SDK is therefore insufficient.

The corrected query preseeds individual
`VkPhysicalDeviceScalarBlockLayoutFeatures` (`sType=1000221000`) and
`VkPhysicalDeviceShaderIntegerDotProductFeatures` (`sType=1000280000`), initially
disabled, before asking XeSS to patch the full writable chain. XeSS sets both
members and appends only mutable-descriptor-type features. It preserves the
test's existing clip-distance, float16, device-address and timeline bits.
`query-xess-features-preseeded.py` and
`xess-vulkan-device-feature-preseeded-requirements.json` preserve this experiment.
The engine wrapper must clone caller-owned features, copy `pEnabledFeatures`
into `VkPhysicalDeviceFeatures2`, clear `pEnabledFeatures`, avoid duplicates,
preseed the two individual features and retain all storage through
`vkCreateDevice`. This result is specific to the pinned payload and tested GPU;
unknown future requirements must refuse safely.

The private provider lives in `runtime/orchestrator/src/native_xess_vulkan.cpp`.
An early requirements service loads the explicit private SDK directory before
Vulkan creation. The successful DLL load stays mapped through process exit,
because Godot retires host services before its Vulkan device. A live SR context
refuses requirements shutdown. The existing public SR service routes provider
`RSF_SR_XESS` (`5`) to the private provider.

At SR context creation, XeSS receives the borrowed instance/physical device/device,
HDR colour, automatic exposure, depth inversion and jittered-motion flags. The
SDK supplies optimal render dimensions and their valid range. Quality mappings
include native AA, Quality, Balanced, Performance, Ultra Performance and Ultra
Quality. No fixed ratio is used as proof of the SDK's chosen dimensions.

Each evaluation copies RGBA16F colour, R32F normalized depth and RG16F motion
into owned images/views. Engine inputs require transfer-source usage and return
to shader-read layout before SDK execution. Velocity scale converts the caller's
motion units to current-to-previous render pixels; jitter is in input pixels.
Exposure scale is reciprocal pre-exposure. Session, source frame, view and
resource-generation changes, frame gaps and explicit reset flags reset history.
Only an accepted SDK dispatch copies private output into the engine's
transfer-destination image. The caller serializes recording, submits even a
failed recording or retires it, retains all image leases through completion and
drains referencing work before context destruction. The service submits no queue.

The standalone fixture passes 24 accepted GPU dispatch/readbacks, 12 each for
Quality and Ultra Quality, with explicit resets at frames 1 and 7. It checks
foreign-device refusal, missing transfer-source usage and live-context shutdown
refusal. The fixture uses the shared named GPU validation lease and the original
vendor DLL. These are device fixtures, not Godot engine or game tests.

An initial claim of core validation was corrected. The first
`test-native-xess-vulkan-device.log` run requested
`VK_LAYER_KHRONOS_validation` through the environment, but that layer was not
registered. Loader diagnostics in `test-native-xess-vulkan-capture.log` show the
loader continued without it. Those passing pixel checks are unvalidated GPU
evidence. The corrected `test-native-xess-vulkan-validated.log` sets explicit
`VK_LAYER_PATH=.local/godot/vulkan-validation/0693dca1739ee70f/files`, disables
implicit layers and confirms the Khronos DLL was loaded and inserted. All 24
dispatch/readback checks pass with no reported VUID or validation errors.
Synchronization validation was not requested or established.

Bounded validated captures are in
`.local/godot/xess-vulkan-device-validated-raw`. The original fixture reused
session 43 between quality cases, so the retained frames 1/2 are the final Ultra
Quality case, `214x120` input to `320x180` output. Their metadata records SR
provider 5 and the SR version above. `pixel-inspection.json` reports maximum
interior RGB error `0.00029296875` against expected linear colour
`[0.18, 0.05, 0.7]`. The decoded first output PNG was opened and shows the expected
flat blue image. This constant-field test does not establish moving-object,
camera, jitter-sign, transparency or occlusion quality.

The first native compile rejected deprecated C++20 `filesystem::u8path` under
warnings-as-errors. The implementation now constructs a path from UTF-8
`char8_t` storage. `build-xess-vulkan-first.log` retains that failed build.
The device-tested fixture predates subsequent test hardening: requested-layer
presence is now asserted, and quality cases use distinct sessions 44/48 to
preserve both capture sets. A later evaluator review also added pairwise
input-alias rejection and finite reciprocal-exposure validation before any
staging commands, with focused negative checks. These latest guards and test
changes await the next build and device run at this checkpoint.

Pending work is the cloned engine creation wrapper, full native verification,
standard and .NET Godot execution, switching/resize/retirement, C# execution,
independent moving-scene image comparisons and exported-player acceptance.
Neither plugin readiness, measured latency nor broad provider acceptance follows
from these standalone results.

The subsequent engine image run `visual-20261025-185108-331.json` uses native DLL
`ECEA26DA96C3EF97E958E602E6985E8D863CD97736D9BEAD01110FAA80B07F6B`, standard
engine `E60D28D55689EF5DA3E28A98023032E21C6EB8C5264B5BF224EAF41AEB0034EF`
and .NET engine `64E12816663F9840EAB59C4757A9161C8528775820B5CF5FF7EA585A71FD2218`.
It executes XeSS SR with FSR3 FG/PCL on both editions, but fails unchanged image
gates against matching MSAA references. The first .NET comparison PNG was opened.
Below row 120, generated RGB equals its HUD-less base at every pixel, rather than
an interpolated midpoint. Differences are confined to the label region
`[12,17]-[256,85]`; `xess-generated-base-copy-inspection.json` preserves that
check. The approximately 1.0 to 1.14 output-pixel interpolation residual is
therefore not, by itself, evidence of a XeSS jitter or velocity convention error.

Captured source frames 13/14 have 512.18/531.20 ms CPU frame intervals and explicit
reset flags, while frames 12/15 have 2.77/3.13 ms intervals without source reset.
The module's existing `render_delta > 0.25` condition requests those resets and
sets camera reprojection to identity. Static-floor camera motion consequently
becomes zero on frames 13/14. FSR3 presentation refuses interpolation for reset
source frames. Its next available callbacks, 12/15 in this capture, report SDK
reset and copy the base image. Missing generated frames and poor midpoint
positions follow this interrupted temporal history. No reset threshold, image
gate, jitter sign or motion scale was changed to hide the failure.

Two bounded standard-only isolations retain the same native DLL and reuse the
matching reference. `xess-no-pcl-20261025-1956-combined` disables PCL mode but
reproduces the same pixels, reset cadence and approximately 528 ms intervals.
`xess-global-fsr3-20261025-2002-combined` uses FSR3 SR instead of XeSS, also with
PCL Off, and reproduces 512/519/509 ms intervals with one captured generated frame
and failed image gates. This excludes a failure specific to XeSS evaluation.
`xess-no-validation-20261025-2008-combined` then disables Khronos validation and
still records 528/515/528 ms reset frames around a 1.55 ms ordinary frame. It
excludes validation overhead as the necessary cause. These captures use the
shared immutable fixture payload, four bounded source frames each and the named
GPU lease. Neither vendor payloads nor the SDK cache were modified.

The remaining investigation is common submission, WSI/presentation, overlays or
device preparation. Streamline's matched source has a 500 ms semaphore-wait
constant, but no corresponding timeout warning appears in these capture logs;
attribution to that wait remains unproven. The existing FSR3 submit tracker can
also promote abandoned records when a reset command buffer reuses an old pointer.
That source defect merits a recording-identity guard, but these captures do not
prove it causes the measured pauses. Fresh analysis JSONs, capture metadata and
comparison PNGs retain the failures while further timed evidence is gathered.

The final warm-cache editor matrix `visual-20261025-194828-946.json/.html` passes
all six standard/.NET object, camera and combined-motion cases with XeSS SR,
FSR3 FG and PCL On. It uses native DLL
`CC92455469F2AC7068B69DF5378989C93B10BBA581AAA2C6C257872CE3093DA5`, standard
engine `0F6489DF643597792FF50B93899A28121824DEEDB547E4F71CC0DFB40AF9A25D` and
.NET engine `E6AEE5F92E4F3FC743D636F1EFC5418BA472866F73240565A7779CC7760B247E`.
All 24 actual generated images pass unchanged gates against fresh matching MSAA
references. Maximum material-centroid error is `0.2598366608` output pixels;
maximum interpolation residual after source displacement is `0.2492487485`
pixels. The .NET camera frame-14 comparison PNG was opened.

`xess-final-matrix-20261025-summary.json` checks all six analyses, matching binary
identities, actual generated-frame availability, C# success on .NET, explicit
Khronos layer identity, reported synchronization validation, effective process
extension-point policy 1 and absent RTSS injection/layer modules. SDK tracing is
off; these captures perform no diagnostic semaphore-counter queries. Vendor
payload identities are retained in the compact summary and immutable per-fixture
runtime manifests. The first cold-start failures remain distinct from this warm
acceptance. The reset-feedback correction and its timing evidence are documented
in [the focused FG note](godot-vulkan-fg-reset-feedback-20261025.md).
