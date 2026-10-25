# Godot Vulkan DLSS frame-generation ownership

## Question and source scope

This work asks how a native Godot Vulkan presentation owner can share the early
Streamline session and CPU source tokens with PCL and independently selected SR,
while retaining actual submitted source identity through asynchronous FG/MFG.
It also asks how to keep secondary and FidelityFX-owned swapchains outside DLSS-G
and how to retire the SDK presenter before releasing engine resources.

The inspected public source is Streamline commit
`2122257e0fce486f91b385aa63b9a09b0a34b363`, version 2.14.1. The signed retail
`sl.dlss_g.dll` inspected on 25 October 2026 has SHA-256
`F4A6B2B14DCC0B1485989E430D3B4E3A44AC1800B92BA1AD74F476E64FB2B09C`.
Its PE analysis uses load base `0x180000000`. Binary findings below are static
Ghidra 12.1.4 observations through REA, not runtime tests.

## Swapchain ownership

The public interposer's `wrapper.cpp` invokes the registered feature hook lists
for every swapchain create, destroy, image query, acquire and Present. Seven
loops cover those before/after hooks. Their entries contain both the callback
and feature ID. DLSS-G's retail embedded plugin JSON registers before hooks
for all five WSI operations plus device idle. The JSON and exact hook-name
string are evidence `ev_62dcbb9ba1731aa40cd8d468e8c0c4c3d5d0fa97d5f2cebb7915b0b72bcb94a7`.

Calling `slSetFeatureLoaded` does not unload the DLL in the inspected plugin
manager. `PluginManager::setFeatureEnabled` changes the feature's enabled flag
and rebuilds the global hook lists. Its source explicitly requires that no
hooks execute concurrently. Temporarily toggling that global state while a
different live DLSS-G chain is presenting can suppress that chain's callbacks.

The proposed checked SDK correction uses a synchronous thread-local suppression
scope around WSI operations belonging to another owner. Only hooks with feature
`kFeatureDLSS_G` are skipped. Common and Reflex hooks remain active. A nested
scope restores the preceding thread-local state, including on callback failure.
Device idle remains unsuppressed so an existing DLSS-G presenter can drain.
The correction was initially developed in an ignored isolated recipe rather
than invalidating the current staged SDK receipt during other agent builds.

Create/destroy differ from acquire/images/Present. LL2's
`notifyCreateSwapchain` and `notifyDestroySwapchain` acquire their own swapchain
mutex, which the existing host WSI broker also uses. Wrapping the entire create
or destroy in that broker would reacquire the same nonrecursive mutex. The
native DLSS owner calls interposer create/destroy directly. A subsequent review
also found that SDK proxy acquire/Present can wait on the SDK presenter, whose
real WSI operations use common's raw dispatch table. Holding the LL2 broker
around a proxy call can therefore block a worker that needs that same lock.
The final proposed split calls all DLSS proxy WSI directly and brokers only
the SDK common backend's real image queries, acquire and Present. Foreign
native/FidelityFX paths keep their existing broker because DLSS-G is suppressed
there. These corrections precede the first device test of this service.

## Source and resource leases

### Game-facing swapchain image layout

The DLSS owner now requests `VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL` at the end
of the engine's game-backbuffer render pass. The existing FidelityFX replacement
requests `SHADER_READ_ONLY_OPTIMAL`; real WSI images still use `PRESENT_SRC_KHR`.
The change corrects the initial inference from allocator metadata below. Its
device validation is pending after the first synchronization trace.

The public `commonInterface.h` `SwapChainContext::resizeProxyBuffers` creates
the proxy images from a `ResourceState::ePresent` description. In the pinned
Vulkan backend, `toVkImageLayout` maps that state to `PRESENT_SRC_KHR`.
Retail `slHookVkGetSwapchainImagesKHR` at RVA `0x57390` returns these proxy
images, through initialization at RVAs `0x50480` and `0x4e320`. The matched
retail `resizeProxyBuffers` at RVA `0x4df60` writes description state `0x40000`,
the public enum's `ePresent` bit. Evidence IDs:
`ev_b6f70ec94228c81d865c9e01ef8f5fa370617c2c521d9ef4388c4017690554c1`,
`ev_eb560189f6e69cd7c3bac82059d9c36300961a8cc48a0be3b549d2a5fbe32302`,
`ev_0dcf4590c090961242cfcd4ec1af240d0204d90f88ef20c192b92516292f02d6`
and `ev_d85b4aa39b705c4e5ad65766955b97069850780665b036051940145947cf1e1b`.

The retail noninterpolating forwarding path at RVA `0x43e50` explicitly wraps
the intercepted game backbuffer with abstract state `0x40000` before its copy
and actual Present. This was initially interpreted as a Present final-layout
requirement. Evidence:
`ev_039c18b2b4f2a6e72decc9ca9096464856f0af40d0b4dc1275599fe3d657aa52`.
The provider-specific image-layout query supplies the final layout;
the generic engine dispatcher must use it per owner instead of treating every
replacement as a FidelityFX chain. The ordinary engine first-use attachment
layout remains `UNDEFINED`, followed by raster writes and the owner's final
layout.

Initial SDK bookkeeping remains a device-test audit item: the public Vulkan
allocator records newly allocated proxy resource metadata as `UNDEFINED`,
despite the `ePresent` description. The FG-on collector resolves its source
through the common resource-state machinery. This source review does not prove
that first-use FG-on transitions preserve every raster write. No speculative
SDK correction or device acceptance follows from this observation. The later
trace exposed `09600`: the SDK's Off path copies from game-facing proxy images
without transitioning the source. Retail `presentFrame` at `0x48830` transitions
only the native WSI destination; public Vulkan `copyResource` hardcodes the source
as `TRANSFER_SRC_OPTIMAL`. The proxy's abstract Present annotation does not prove
an actual source barrier. Input packaging at RVAs `0x45570`, `0x45760` and
`0x4ba50` constructs descriptors and native views without adding that missing
transition. The getter now returns transfer-source layout, with an explicit
FG-on/Off validation check still required. Evidence:
`ev_621953a15c199486163df4c04a22d0e949084d8da4210195b79aea6a58ec1a1c`,
`ev_1210430c6620dd467ae4102e69656721686b1eda15adb65e455dfbea9c936bc4`,
`ev_97862e2b9d6ecafa16fd7fec023fbad84de7c2ffa4622822f37b7b62a9fd0ed2`.

The native service uses viewport zero, leaving DLSS SR on viewport one. It uses
the existing session's synchronous `rsf_slvk_invoke` lease and the original CPU
session/source identity. It never acquires another render-thread token or loads
another interposer. Recording alone cannot enable interpolation: both guide and
HUD-less command buffers must appear in successful host submission notifications.

Normalized R32F depth, dense RG16F motion and post-tonemap pre-UI RGBA8 color are
copied into owned images with views. `eOnlyValidNow` tags cause SDK copies on the
matching source's recording command buffer. Input engine layouts are restored.
Motion scale follows the existing DLSS SR path: adapter pixel scale divided by
render dimensions. There is no additional sign reversal in the FG provider.
Camera matrices, jitter in pixels and inverted depth use the frame record.
Session/view/resource-generation discontinuities request temporal reset.

The initial SDK mode blocked the presenting client queue. The traced binary-wait
violation below required using the supported Vulkan `eBlockNoClientQueues` mode
and retaining each source's SDK input completion semaphore/value. A bounded CPU
wait must finish before its private slot can be overwritten or feature resources
can be freed. Timeout or missing completion identity retains the owner. Host
source and consumer fences remain separate. Partial prepare-only submissions are
also fenced. The first synthetic run exposed an additional identity error:
an abandoned frame retained the same command-buffer address that a later frame
re-recorded. A later submission could incorrectly promote both frames. The
service now assigns a reused address only to its newest recorded source and
invalidates superseded unsubmitted matches. This was a synthetic boundary
finding, not an observed engine failure.

Generated count excludes the real frame and is refused above the SDK-reported
maximum. SDK `numFramesActuallyPresented` deltas are accumulated separately
from generated-output callback counts. Ordering uses a local recording serial so
a new session that restarts its source IDs cannot inherit the preceding session's
replay cutoff. A requested multiplier or a larger
presentation counter is not image or latency evidence. No UI alpha/color
producer is supplied by this ABI, so separate UI recomposition is disabled.

## CPU presenter retirement

The retail hook-name string at VA `0x18007b468` references a hook registered by
`slGetPluginFunction`. The corresponding `slHookVkDeviceWaitIdle` is at
VA `0x180057910`, RVA `0x57910`. Its instructions set the interposer's skip flag,
lock context offset `0x4530`, call the context flush function and then invoke a
compute virtual method with the device. The log text names the hook and states
that current work is flushed before idle. Evidence:
`ev_e431230eccea21ff78b0b11d5ccdbe075d473f72e516222d33b57295e44bd268`
and `ev_e6e33a8c2ac8e5ebc8f36cee7dc5c8c299a5dae806610fd4ef39286a5d6a32b1`.

The called function at VA `0x18004a920`, RVA `0x4a920`, contains the
`DLSSGContext::flushAll` name in its function-object RTTI and logs that all worker
queues are flushed. It invokes worker flush operations before context/GPU waits
and resets queued-frame bookkeeping. Evidence:
`ev_2064531e3d76dafb81647662595409a1c0fac328470ae63b06ff0b21832577e2`.

The worker flush at VA `0x18003b150`, RVA `0x3b150`, waits with a 500 ms timeout
and emits the warning `Worker thread '%S' timed out` on failure. Its caller does
not propagate every timeout through the Vulkan result. The matched public
`sl.thread/thread.h` removes a queued item only after its callback finishes;
therefore a successful empty-queue flush covers callbacks already executing.
Evidence `ev_a00614294df4975384734db26b19b804adc7e3186e1d1cdf1d5f62871efcc5f0`.
A successful proxy idle call alone cannot certify retirement if that timeout
warning occurred. The bridge now keeps a lock-free monotonic warning epoch.
Drain snapshots it before requesting Off and checks it after proxy idle. Any
change refuses retirement and retains the owner/capture lease for retry. Off
uses `eRetainResourcesWhenOff`; explicit resource release follows successful
CPU/GPU drain. Requested mode/count remain available for the next presentation.

## Actual output capture design

The public compute interface has a `FrameBoundaryGuard` that calls
`delimitFrame(presentFrameIndex, genFrameIndex, true/false, ...)` around each
real SDK present. In `CommandListContextVK::delimitFrame`, a nonnegative
`genFrameIndex` explicitly denotes a generated frame and is passed to Nsight as
its generated ordinal. This is a producer discriminator, not an inference from
FPS or Present order.

Retail evidence confirms the call path. `DLSSGContext::presentFrame` at
VA `0x180048830`, RVA `0x48830`, passes `-1` for real frames and `AsyncInfo[1]`
when its frame is marked generated. The helper at VA `0x180048690`, RVA
`0x48690`, calls context vtable offset `0xf8` with that ordinal and begin=true,
calls actual Present at offset `0xc0`, then calls the delimiter with begin=false.
The public interface identifies those exact vtable members. Evidence:
`ev_082568101ca939b24a9e234e0ba02b256cc880d5960ee1ccfca66a9f0cc3c693`.
The retained presentFrame dossier is
`ev_5a1de008cef6184e94871ac9600a92d7f215931e82245ec4ee119dccebfbadd2`;
its large value-flow facet is truncated, while its separately inspected complete
pseudocode projection is retained as
`ev_4d18f00a591301d4a56b401e95d2e6dc21cabfd1ced5b4ddf23d3088800ad02b`.

The NGX parameter writer at VA `0x180060510`, RVA `0x60510`, selects
`OutputInterpolated` with zero-based resource index `param6` and forwards
`MultiFrameIndex=param6+1` and `MultiFrameCount=*Frame`. Evidence:
`ev_05e71ddc984d6b9c8ce5b6f77f862b09df9c575569b78262c820426e7a1936b4`.
The interpolation fraction corresponding to that MFG ordinal has not yet been
established by this source review; the proprietary NGX model remains outside
the inspected public source.

The optional diagnostic correction retains those boundary fields before the
Nsight initialization guard and forwards them at
`CommandListContextVK::present`, where the SDK presents its real native chain
and image index. The private size/version-checked C ABI registers one leased
device/owner and supplies raw native image-query, submit and present pointers.
The host callback never reenters the shared session, PCL or proxy WSI.

The host retains bounded source records independently of the staging ring. It
copies the actual native image after consuming the original binary Present
waits in one submission, waits that diagnostic copy's GPU fence, and presents
with zero remaining waits. This synchronous bounded diagnostic path introduces
no replacement semaphore or cross-image semaphore reuse. The raw image queries
and immediate native Present use the common LL2 mutex; the diagnostic GPU wait
stays outside it. It records explicit
SDK ordinal, original source/session/view/resource identity and real image
index, with `fraction_established=false`. Capture registration is cleared only
after CPU presenter and GPU retirement. Callback ABI tests and actual device
readbacks are still required before this design is accepted.

## Validation state

The checked SDK recipe was moved into the tracked build inputs and built in the
new isolated output
`.local/godot/vulkan/host/artifacts/streamline-production-dlssg-wsi-capture`.
The existing runtime and staged add-on were preserved. The builder stages the
private capture header with the verified-copy helper, checks it again after
patching, and includes `presentCaptureSha256` in receipt/cache validation.
The original pinned Streamline reference checkout remains clean.

The new Production receipt records:

- Patch recipe: `10B58309B7B75F4F277070C7E54D3C83C32FFB23B5EA445520CEB5414472D7F6`.
- SDK builder: `2BB9A266CCFFA4EC9E7BCE9D526F48B034495825BB67A7A1F4ACE6DBA424D91F`.
- Capture ABI header: `F8B97322A708345315B18DC6BA32049343F526277F8057C5FB69BC18DD231E97`.
- `sl.common.dll`: `8F249456BDE6AB6912ED20B04D3AF27951DC89EDB34E755094B1CD6253F1907B`.
- `sl.interposer.dll`: `4CD05B3E7EB0C891096A3C07D0D72AA4D3010247F4B52D846A3A79057FD5D4FE`.

MSVC built compute, common and interposer successfully. PE export inspection
confirms the common capture/WSI exports and interposer suppression export.
A second builder invocation accepts the matching cache and binary hashes.
PowerShell parsing, Python compilation and `git diff --check` pass. These are
build, receipt and export checks, not SDK GPU execution.

The initial build wrapper failed before compilation because a relative
`Tee-Object` log path was resolved after the SDK builder changed directory.
The retry uses an absolute log path. The partial first output was safely moved
to the corresponding `-log-path-failure` sibling after automated review refused
its recursive cleanup. This did not modify an existing runtime or game install.

The service, capture helper and synthetic test target built with MSVC after
correcting a duplicate local name and a deprecated UTF-8 path constructor in
the initial capture compile. The final focused FG synthetic checks pass source
identity, MFG count forwarding, partial/failed-tag submission fences, command
address reuse, source-ID resets, foreign WSI and timeout-epoch retirement
refusal/retry. The initial test failure and stale-address correction are
recorded above. The exact isolated
thread-local helper compiled and ran with clang on Windows, checking nested
restoration, null refusal, callback return/failure/throw and another thread's
independent state. Extracted exact capture-registration and real-WSI helper
code also passed clang synthetic checks for size/version/owner refusal,
foreign-device fallback, explicit ordinal propagation, callback failure/throw,
single native Present, positive acquire timeout preservation and one
nonrecursive broker lease. The isolated checked recipe applied 43 exact edits
to fresh source copies.
These are build and synthetic observations only. Native Godot DLSS FG/MFG
execution, generated-image quality, resize/provider/secondary lifetime and
latency measurements remain pending at this point in the record.

The generic Vulkan presentation dispatcher preserves the existing ABI 1 setup,
frame and status layouts. Its legacy create still selects FSR3 with one generated
frame; explicit creation selects DLSS or FSR3 independently of SR. A single
process admission covers both concrete providers. Unsupported counts are refused
without clamping, and failed retirement retains the owner and admission for a
retry. The FSR3 extraction was mechanically checked as a faithful type/function
rename before adding the scoped SLG suppression and optional timing calls.
The focused dispatcher test compiles and runs with clang on Windows. The first
full native gate exposed three existing contract tests requiring outer ABI
mismatch to precede embedded graphics size validation. The dispatcher now checks
outer size, outer ABI, embedded size and embedded ABI in that order; added focused
precedence cases pass. The subsequent full gate is coordinated separately.

A static shutdown audit found that scene-module uninitialization occurs before
Vulkan swapchain destruction: pinned Godot `main/main.cpp:5312` uninitializes
scene modules, while `OS::finalize` and `finalize_display` follow at lines 5352
and 5354. The scene callback therefore cannot shut down shared Streamline while
a DLSS WSI owner remains. That owner still needs its checked session to release
feature resources and unregister the presenter capture before destroying the
proxy chain. The adapter must split stopping scene work from final SDK shutdown
after WSI retirement. This is a source finding, not a reproduced device failure.

The adapter now defers shared SDK shutdown after scene retirement until the
actual WSI owner is successfully destroyed. Refused retirement retains both
owner and session; a new boot refuses while shutdown is pending. The full native
gate following the validation-order and shutdown corrections passed its enabled
tests; GPU fixtures were skipped in that gate.

## First Godot device capture, 25 October 2026

The first standard-editor run used native library
`ED32434DBBEBB2F14A05AC021E86C925D5C6FA4A2A555F82CBAC224F513A57B7`,
editor `1C6BA49DD71408F6A89D9EF5F15348561E8CBE0A7EB79DCF71BB5EFA283D0250`,
SR Off, one requested generated frame, Reflex On, 1280x720 and process-only
legacy-extension isolation. Khronos synchronization validation was loaded.
`dlssg-first-20261025-combined` retains four generated presents and four real
presents at source frames 12 through 15, plus depth, motion and HUD-less inputs.
All 20 images were decoded, and generated frame 14 was opened for inspection.
`dlss-present-capture.jsonl` establishes actual SDK-generated ordinal zero and
matching source identities. It deliberately does not assert the interpolation
fraction. The SDK reports a maximum of five generated frames on this device;
that query is not higher-count image acceptance. The ordinary generated-callback
counter is zero for this service; the capture sidecar supplies separate direct
present evidence.

The harness **fails** this run on real Vulkan API errors. The early SDK wrapper
appends an optical-flow queue that conflicts with Godot's transfer-capable family
5. The native service's real swapchain also lacks transfer-destination usage
required by the SDK copy. A binary wait depending on a not-yet-submitted timeline
signal remains a separate synchronization investigation. These errors prevent
image acceptance despite successful readbacks and plausible visible output.
The SDK's VSync warning is retained as well. No clean Vulkan or latency claim
follows from this capture.

Godot's optical-family correction leaves transfer-only optical-flow families
unallocated to the engine while the integration is active. Their queue vectors
stay empty: device queue lookup loops only populated vectors, and
`command_queue_family_get` explicitly ignores empty families. Graphics/compute
families remain available. The SDK must still guard capacity and preserve
exclusive ownership; silently aliasing the engine's queue is not a valid fix.

## Queue trace and readback control, 25 October 2026

Evidence `dlssg-queue-trace-valid-20261025-combined` uses editor
`0F6489DF643597792FF50B93899A28121824DEEDB547E4F71CC0DFB40AF9A25D` and
native DLL `4076F1E468D24EF86ED5E88029174502D7762D11460FA84E267175DE14D99042`.
The corrected queue-trace SDK and optical-family engine change remove `02802`
and `06755`. The service's required destination usage removes `01213` and
`06663`. Extracted exact optical allocation code passed clang synthetic checks
for absent/spare/exhausted families, host priority/index preservation, distinct
flags, total capacity across flags, and invalid family refusal.

Both readback-on `capture.stderr` and readback-off
`no-readback-20261025-192609-383.stderr` still fail strict API validation with
`03238`, `03873` and the layout error `09600` described above. Both processes exit
zero and pass the application fixture; that does not override the API failure.
The no-readback summary retains loaded modules and confirms the Khronos layer,
process extension-point mitigation and unchanged native/engine hashes. The SDK
trace is explicitly bounded to 1024 rows. An earlier attempt without the portable
layer environment was refused by the harness before acceptance; it was retried
under a separate evidence label with the layer path and synchronization checks
set explicitly. All GPU runs used the shared validation mutex.

The off trace establishes the ordering defect independently of diagnostics.
Timeline semaphore `0xc900000000c9` is created as
`nv.sl.dlss_g.fence.dlfg-signal`. On the application thread, game-context submit
76 queues a wait for value 13. The next application binary wait triggers `03238`,
and game-context submit 80 triggers `03873` on another binary wait. The SDK worker
only submits the resolving timeline signal at submit 84. Default
`eBlockPresentingClientQueue` therefore places an unresolved future timeline
dependency ahead of later binary waits on the same application queue.

The service now uses official Vulkan `eBlockNoClientQueues`. Immediately after
each accepted proxy Present, on that same presenting thread, it records
`DLSSGState::inputsProcessingCompletionFence` and its value in the submitted
source's private slot. Retail `slGetData` at RVA `0x57a10` copies the native
completion fence from context `+0x4628`, wrapper `+0x10`, and the corresponding
value from `+0x4600` or the current present ordinal minus one. Public Vulkan
`createFence` returns a raw `VkSemaphore`. These are borrowed SDK objects whose
lease ends only after successful slot completion and owner retirement, not after
an original frame number expires. Before slot overwrite or feature release,
`vkWaitSemaphores` waits the captured value on the CPU outside the shared SL
session lock. No future timeline wait is inserted on an application queue.
Synthetic tests now check captured-value identity, timeout retention, successful
reuse, and refusal to discard a pending lease merely because GPU idle succeeded.
Compilation and a new API-clean device test of these final corrections are
pending at this point in the record.

## Clean count-one and shared-session image checks

The corrected zero-value wait guard passed strict core and synchronization
validation with native DLL
`CC92455469F2AC7068B69DF5378989C93B10BBA581AAA2C6C257872CE3093DA5`:

- `dlssg-zero-valid-20261025-combined`: standard editor, native SR 0, DLSS FG 1,
  one generated frame, Reflex mode 1, 1280 x 720. Four actual generated images
  and four real images retain source frames 12 through 15. The readback-disabled
  control `no-readback-20261025-194800-040` also passes with no VUIDs. Neither
  control reports worker/pacer timeout or refused shutdown.
- `dlss-sr-fg-valid-20261025-combined`: independently selected DLSS SR 1 and
  DLSS FG 1 share the same early session and original tokens, with actual
  generated readbacks and clean API use/retirement.
- `dlssg-mono-valid-20261025-combined`: the .NET editor and C# facade pass the
  same native-SR/count-one scope with clean API use and actual images.

An independent native-SR/FG-Off reference,
`dlssg-midpoint-reference-20261025-combined`, renders the same combined object
and camera motion at half steps, 1280 x 720 with MSAA 8x. The reference uses the
FFX Off owner only to capture engine guides; no interpolated reference is used.
The analyzer binds DLSS ordinal zero only to an exact count-one generated/real
producer sidecar pair with matching session, original source, view, resource
generation, SDK token and native chain. Missing, foreign, duplicate, wrong ordinal
and MFG records fail that default proof. The half-step is explicitly a test
hypothesis. Raw capture and analysis both retain `fraction_established:false`.

All four source frames per case pass the existing geometry and input checks.
Generated material-centroid error against the rendered half-step hypothesis is
0.060 to 0.082 output pixels for standard native SR, 0.042 to 0.134 for shared
DLSS SR/FG, and 0.051 to 0.077 for .NET native SR. Corresponding mean edge RGB
errors are 0.0260 to 0.0267, 0.0459 to 0.0462, and 0.0259 to 0.0265. The original
real-source centroid is 2.14 to 2.34 pixels from this reference. Frame 14's actual
generated/reference/error contact sheet was opened for visual inspection.
These results establish the bounded box/floor scene behavior, not broad scene,
UI recomposition or latency acceptance.

## MFG scratch-buffer dependency and image checks

The first three-generated-frame attempt,
`dlssg-mfg3-valid-20261025-combined`, captures genuine SDK ordinals 0, 1 and 2
plus real ordinal -1, but fails synchronization validation. Successive
`vkCmdFillBuffer[nv.ngx.dlssg.Evaluate]` calls write the same NGX scratch buffers
without a write-after-write dependency between generated command buffers. The
trace identifies buffers `0xff50000000ff5` and `0xff70000000ff7`, each 1,843,200
bytes, on the same SDK DLSS-G queue. Existing image-copy/clear corrections do not
cover buffer fills.

The private NGX dispatch now wraps `vkCmdFillBuffer` with the existing
all-commands/transfer memory read/write dependencies before and after the real
fill. Arguments and actual command submission are preserved. The checked recipe
requires the definition, resolver routing and function loading exactly once.
`streamline_vulkan_dispatch_contract` verifies those paths, foreign-device
forwarding, original arguments and successive fills' barrier scopes/order.

The new Production SDK under `streamline-production-dlssg-mfg-transfer` records:

- Patch recipe: `886005E0F17E5D8B23C723334598B14573068AADBCAB98E373D4A36121A539E7`.
- Dispatch header: `91AC72B6A41D93CF7710E0577BAE0E1F5CCE27F4E93D8E0B5D968B5EF7F003B9`.
- Common DLL: `157C9198BDE92331E7FC6202FA756758447EFD23C99352F3A5E1358B83638FBA`.
- Interposer DLL: `D30C68CA5D5A3EDDA8C082D774DD2CBE00F218B22BB349188F4F3060EC316362`.

`dlssg-mfg3-fill-quiet-20261025-combined` passes strict core/synchronization
validation and teardown with native DLL
`19BC537D1FF40A863C91690B2CAE39D005FD3760B65699DEECDF336A2B34F301` and editor
`DE9BA015EDC32412380BE6A57B59118BAC604517538C957371ADF6D8946145CB`.
It saves twelve generated and four real images at source frames 12 through 15,
with every SDK ordinal established independently of presentation counters.
Final status has FG active, SDK result zero, requested generated count three,
88 tagged sources and 329 SDK presentations. An earlier attempt lost foreground
late, paused FG according to policy and failed the fixture's final active check;
the quiet repeat retains foreground and passes.

`dlssg-quarter-reference-20261025-combined` independently renders sixteen native
MSAA 8x frames at quarter steps. Per-ordinal analysis explicitly supplies a
reference-fraction hypothesis; it requires all three generated ordinals and the
real producer record for each source. It selects actual recorded reference pose
times instead of synthesizing or blending a reference image.

| SDK ordinal | Declared reference hypothesis | Generated centroid error, pixels | Mean edge RGB error |
| --- | --- | --- | --- |
| 0 | 0.25 | 0.072 to 0.206 | 0.0248 to 0.0256 |
| 1 | 0.50 | 0.051 to 0.077 | 0.0259 to 0.0266 |
| 2 | 0.75 | 0.180 to 0.250 | 0.0251 to 0.0254 |

All four sources per ordinal pass the same strict geometry/image gates. The
negative control comparing ordinal zero to a half-step reference fails all four
centroid checks. These measured hypotheses do not claim a vendor-provided
interpolation fraction; `fraction_established:false` remains in every capture
and analysis. Higher counts, broader scenes, explicit UI recomposition and
latency measurements remain outside this acceptance.

## Unrelated final-gate artifact investigation

After the toolchain instance changed, the full gate's unchanged shared shim
test reported loader error 1114. A bounded exception probe localized an access
violation before user `DllMain`, at `_DllMainCRTStartup` RVA `0xbab90`: its mapped
entry bytes were zero although on-disk bytes at offset `0xb9f90` were valid.
Byte-identical fresh copies, including the same `dinput8.dll` basename, and the
existing Loader/version aliases load and pass the official forwarding test.

Moving the original aside and using CopyFile alone did not repair the mapping.
A fresh byte write through `FileStream(CreateNew, WriteThrough)` plus
`Flush(true)` followed by an atomic rename did. Hash
`F798DAA60B15060D059E4A8E90CF99C32CF394FA2929F77E639C8CB09A1F0AF8`
remains identical. The official test then passes at its original build path.
Both failed file identities are retained in ignored diagnostics. No loader
source, export contract or test expectation changed. The underlying Windows
image/copy-cache cause is not established beyond these controls. The subsequent
full gate passes all 68 discovered tests with 57 executed and 11 explicit skips,
plus Rust formatting and Clippy.

The first NoBlock/transfer-source device attempt,
`dlssg-noblock-transfer-20261025-combined`, used native DLL
`024DA92F43CFC85CB936D406A252E626E1F4D92F9A804AD3BC5D3E3ABB89BBF1`.
It emitted none of the preceding layout or binary/timeline VUIDs during rendering,
but stopped tagging after six source slots. The new reuse guard incorrectly
treated a returned completion value of zero as a missing lease. Zero is a valid
timeline value; only a missing semaphore identity is invalid. The guard now waits
the exact returned zero value as well, with a synthetic regression check.
The failed attempt refused owner retirement, after which engine teardown
continued to surface/device destruction and reported `01266`/`05137`, then exited
with an access violation. No clean retirement or generated-image acceptance is
claimed from that run. The correction requires a new native build and repeat of
both readback controls; it does not change the SDK recipe.

## Submission identity and explicit lifecycle follow-up

The private FFX Vulkan submission notifier previously matched every pending slot
by command-buffer pointer and could reset an already accepted slot's completed
fence when a later recording reused the address. The corrected implementation
assigns a monotonic local recording sequence to each accepted prepare, discards
older unsubmitted matching roles, and fences only roles newly accepted by the
current notification. Recording and successful SDK preparation are separate:
partially recorded SDK failures still retain a producer completion lease, without
becoming eligible for interpolation. HUD-less admission also checks session,
view and resource generation. Present freshness follows recording sequence so
source frame numbers may restart in a new session.

`tests/native_ffx_vulkan_submission.cpp` includes the real implementation with
fake Vulkan and FFX functions. Windows clang with warnings as errors passes
same-address reuse, combined and split role submissions, duplicate notifications,
abandoned sources, partial SDK failure, timeout retention, exact view identity
and restarted frame-ID cases. The existing camera-reset regression also passes.
These are synthetic results; the coordinated native gate and subsequent device
retake are separate requirements.

The new `fg_lifecycle` fixture exercises DLSS Off/resume, resize while active,
count one to three to one, FFX provider replacement and return, and secondary
window creation, resize and retirement. The first standard run,
`fg_lifecycle-20261025-203843-939`, used native DLL
`4765226F3364854814CBDA3AC5516F9F388A49FD6E5F31ABD9FCB76BA3A46EC2`.
All eleven scalar samples and player images pass their state, foreground,
extent and root-session checks. Nevertheless the process fails strict
acceptance: immediately after active resize at source 85, validation reports
`03238` and cross-queue read/write racing on the new proxy image. The binary wait
depends on the SDK's older `0xc900000000c9` timeline, value 90. The process later
exits with an access violation during shutdown. The first error precedes the
count-three change, which starts only after the source-115 sample. An initial
provider-transition suspicion was corrected by matching SDK timestamps to PNG
write times. The root cause remains under investigation; healthy counters and
the report's scalar `passed:true` do not establish lifecycle acceptance.

Pinned retail review confirms `slSetData` at RVA `0x58250` stores the options
with the SDK present ordinal; `presentCommon` at RVA `0x46840` reconciles viewport
options and mode one skips the client-queue completion wait at context offset
`0x45d0`. Evidence IDs are
`ev_1d1ac670924d385c4f4404917c24575682891425091306c5750cb8c2f4ec61d0`
and `ev_6d5e342a5f442c885f3056a3ce4ac8189cd72d6528634adfc1c73fd63b999113`.
The public pinned guide says options apply to the next Present. Native options
publication now follows that boundary on every owned proxy Present, including
initial Off and unchanged states, and on drain. The previous local-state cache
could skip publishing Off for a newly reconstructed owner even though its local
state did not describe the shared SDK viewport. Synthetic assertions cover this
publication contract, including refusing WSI if both enable and explicit Off
publication fail. Resource-completion waits are unchanged. Whether this corrects
the observed resize error requires the next strict device retake; the static
review does not establish the runtime root cause.
