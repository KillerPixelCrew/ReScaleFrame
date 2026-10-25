# Godot Vulkan FSR3 FG reset feedback, 25 October 2026

The question was why actual XeSS SR plus FSR3 FG images failed midpoint gates
while SDK execution succeeded. Source images showed correct object motion and
jitter on ordinary frames, but the available generated images copied the current
geometry instead of interpolating it. The investigation followed source-frame
reset identity and timed the native boundaries before changing any convention.
The XeSS prerequisites and earlier experiments are recorded in
[the XeSS note](godot-xess-vulkan-20261025.md).

The initial `visual-20261025-185108-331.json` fails both standard and .NET image
checks. Source frames 13/14 have CPU intervals above 500 ms, reset flags, identity
camera reprojection and zero static-floor motion. Frames 12/15 have ordinary
intervals and accurate reconstructed camera motion. The module deliberately
resets history when `render_delta > 0.25`; that condition remains unchanged.
The next actual FG callbacks carry SDK reset and reproduce the current geometry.
Decoded RGB below row 120 matches the HUD-less base exactly. Remaining pixel
differences are UI text, as preserved in
`.local/godot/xess-generated-base-copy-inspection.json`.

Three bounded controls reproduce the same pauses: PCL Off, FSR3 SR in place of
XeSS SR, and no Khronos validation. Their labels are
`xess-no-pcl-20261025-1956-combined`,
`xess-global-fsr3-20261025-2002-combined` and
`xess-no-validation-20261025-2008-combined`. This excludes XeSS dispatch,
requested PCL mode and validation overhead as necessary causes. It does not
exclude the loaded shared SDKs or presentation owner.

The first process-mitigated old-payload control,
`xess-no-rtss-20261025-2018-combined`, crashed before foreground acquisition and
has no rendered frames. Engine/native staging was changing during that work;
binary skew was suspected but not established. This failed launch supplies no
renderer or image acceptance.

The timed run uses the following synchronized payload:

| Artifact | SHA-256 |
| --- | --- |
| Native DLL | `ED32434DBBEBB2F14A05AC021E86C925D5C6FA4A2A555F82CBAC224F513A57B7` |
| Standard engine | `1C6BA49DD71408F6A89D9EF5F15348561E8CBE0A7EB79DCF71BB5EFA283D0250` |
| FidelityFX Vulkan DLL | `273AA4A84B3BE01AFE0F0CA111B291BB37BD9A94F703DC39EC1A96D8B788288D` |

`xess-timed-20261025-191514-059-combined` runs standard Godot with XeSS SR,
FSR3 FG, PCL Off, synchronization validation and process-local legacy extension
points disabled. `process-isolation.json` records effective policy 1. It uses the
immutable fixture runtime cache and four source frames. It exits with a functional
FG-active assertion failure and retains real queue-creation VUIDs `02802` and
`06755`; it is diagnostic evidence, not validation acceptance. The queue defect
belongs to the separate Streamline queue-preparation investigation.

`native_vulkan_timing.h` records bounded, opt-in CPU QPC intervals around native
boundaries. The CSV is `.local/godot/xess-timed-20261025-191514-059.csv`, with
1,329 records from player PID 32412. Start/end ticks, frequency, PID, thread,
stage and original source-frame identity are retained. The companion
`.summary.json` and `.present-localization.json` are compact derived reports.
The instrumentation submits no GPU work and is inactive unless
`RSF_VULKAN_TIMING_FILE` names an absolute output path.

| Timed boundary | Maximum observed CPU time |
| --- | --- |
| FFX prepare, including slot reuse | 0.47 ms |
| Slot completion-fence wait | 0.02 ms |
| Reflex sleep | 0.46 ms |
| Reflex options | 0.02 ms |
| Real image acquire | 0.10 ms |
| Real QueuePresent through the latency broker | 3.54 ms |

Most recurring pauses occur before the SDK QueuePresent call, inside the native
`configure_frame()` call to `ffxConfigure`. Source frame 13 takes 532.177 ms in
total: 528.660 ms before QueuePresent, 3.511 ms in QueuePresent and 0.006 ms after
it. Frame 12 similarly spends 520.002 of 521.753 ms before QueuePresent.
Startup also contains slower actual SDK QueuePresent calls, including 524.839 ms
on frame 6. The recurring configuration pauses are the relevant stable cadence;
the evidence does not attribute every startup pause to the same internal wait.

Matched FidelityFX source is revision
`c6efa6bf7f2027b3ec94f28578bb5965eabb9e55`, with the recorded local SDK build
receipt in `.local/godot/vulkan/runtime/fsr3-sdk-build.json`.
`ffx_provider_framegeneration.cpp` around 322 updates frame identity and the
HUD-less resource on each configure call. Skipping ordinary per-frame configure
would lose those updates and is not justified.

`FrameInterpolationSwapchainVK.cpp` around 1848 detects changed activation,
queue or callback state. Around 1877, an activation change invokes
`waitForPresents()` and starts or stops presenter threads. The wait around 1910
drains game/interpolation/presentation semaphores, queues and the device. The
native integration had treated each camera-reset frame as an invalid FG frame,
so it disabled the SDK presenter for that frame and re-enabled it on the next
ordinary frame. The expensive transition created another large CPU interval,
which requested another camera reset. This explains the repeating reset,
disable/drain, enable/drain cycle without a motion-vector sign change.

The fix keeps activation stable for submitted, valid input leases across camera
resets. The existing `generate()` callback still ORs the source reset flag into
the SDK dispatch's reset field. Matched `ffx_frameinterpolation.cpp` around 931
uses `params->reset` to discard temporal history. Thus camera cuts still reset
history and can produce a base-copy frame, while an ordinary reset no longer
destroys the presentation owner. Explicit FG Off, invalid or incomplete inputs,
and unknown GPU completion still disable admission. New timing stages separate
enable, ordinary update and disable configuration calls.

`tests/native_ffx_vulkan_reset.cpp` exercises the real native source with fake
SDK and WSI callbacks. Alternating ordinary/reset frames must configure each
frame, activate once, and forward all reset values. Explicit Off must still
disable activation; an unknown completion state must not admit a lease. This
synthetic test and the code change await the coordinated build at this checkpoint.
The fix requires a new timed device run and independent midpoint comparisons on
both editions. No image gate, camera-reset threshold or convention was relaxed.

The next device run, `ffx-reset-timed-20261025-192646-076-combined`, uses native
DLL `4076F1E468D24EF86ED5E88029174502D7762D11460FA84E267175DE14D99042` and
standard engine `0F6489DF643597792FF50B93899A28121824DEEDB547E4F71CC0DFB40AF9A25D`.
It passes functional checks, records 89 generated callbacks for 89 submitted
source frames and captures all four requested generated frames. No VUID or
synchronization-validation error appears in its logs under the explicit layer
and process-isolation settings. This is not midpoint image acceptance.

Its 1,707 timing records confirm that repeated presenter reconfiguration is
removed: one enable takes 0.90 ms and steady configure updates take approximately
0.00 ms. However, roughly 500 ms pauses remain inside the SDK QueuePresent call,
while real WSI, input preparation, fences and Reflex remain fast. Source frames
12 through 15 therefore still have approximately 516 to 530 ms intervals and SDK
reset on every generated dispatch. The activation fix is valid but does not fix
the underlying SDK queue/interpolation wait. This corrects any inference that
the reset-driven reconfiguration cycle was the entire cause.

The next investigation adds guarded, opt-in SDK trace instrumentation through
`runtime/vulkan/fsr3/sdk-fixes/swapchain-trace.json` and its first-party header.
It records actual event wait returns, timeline handles/targets/completed values,
callback frame/reset/results, queue wait/signal values, submit time and pacing
targets. `build-sdk.ps1 -SwapchainTrace` records its inputs in the SDK receipt.
The patch's 42 combined anchors with the existing lifetime/CPU-drain corrections
were checked against the pinned source; that text check is not an SDK build or
device run. No ordering correction is claimed until those internal waits are
measured.

The traced SDK is `F9B030AA97FE254FA32394D007C900FC811C75691BD1C7A93EC09E95B01922D5`,
with trace patch/header hashes recorded in its build receipt. Run
`ffx-sdk-timed-20261025-194018-755-combined` uses native DLL
`024DA92F43CFC85CB936D406A252E626E1F4D92F9A804AD3BC5D3E3ABB89BBF1` and the
same standard engine above. It passes functional checks, captures four generated
frames and reports no VUID or synchronization-validation error.

The SDK trace locates the early stalls in replacement-buffer and interpolation
timeline waits. One replacement wait on frame 12 requests value 21, observes 19
before and 21 after, returns `VK_SUCCESS` and takes 855.03 ms. Its presenter
thread spends 851.62 ms in an accepted queue submit. Early interpolation waits
take about 245 to 259 ms. The interpolation CPU event wait itself is negligible;
generation callbacks all succeed and take at most 2.31 ms, and pacing waits take
at most 7.23 ms. From source frame 13 onward, SDK QueuePresent settles near 2 to
3 ms. Source frames 12/13 still reset after 529/867 ms intervals; 14/15 have
ordinary intervals. This cold-start capture is not image acceptance.

The compact `.sdk.timeline-order.json` report associates queue wait/signal intent
with accepted submit intervals on each CPU thread. All 99 nontrivial,
frame-identified waits return success and have an exact accepted signal for
their target value. None depends on a signal belonging to a newer source frame.
The trace supplies no evidence of the future-frame semaphore mistake found in
the separate DLSS investigation. It does not prove the driver was compiling
shaders; cold-start driver/GPU work remains an inference.

The controlled warm repeat `ffx-warm-no-trace-20261025-194243-677-combined` unsets
`RSF_FFX_VK_TRACE_FILE`, so it performs none of the diagnostic semaphore-counter
queries. It retains ordinary native timing. SDK QueuePresent has a 1.32 ms median
and 6.80 ms maximum; 89 steady configure updates take at most 0.001 ms. There is
one enable and one final disable. Source frames 12 to 15 have 15 to 18 ms
intervals and no reset. Thus steady progress does not require the trace queries.

That warm run passes all unchanged midpoint gates against fresh matching MSAA
reference `ffx-warm-reference-20261025-194343-822-combined`, with the same native
DLL and engine. All four actual generated images are present. Their material
centroid errors are 0.1913, 0.1313, 0.0363 and 0.0326 output pixels; interpolation
residuals after source displacement are 0.0028, 0.0871, 0.0496 and 0.0291 pixels.
The frame-13 comparison PNG was opened. Both runs use explicit Khronos
synchronization validation, filtered implicit layers and process-local extension
point isolation. This is warm, standard-edition, combined-motion SR5/FG3/PCL-Off
acceptance. The earlier startup failures remain retained. A six-case standard
and .NET PCL-On matrix is still pending at this checkpoint.
