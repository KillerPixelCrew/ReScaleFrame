# Godot FSR4 recording handoff, 25 October 2026

The question is how Godot can release Vulkan inputs to a D3D12 FSR4 queue and
resume Vulkan recording without submitting a command buffer from inside a graph
callback. An immediate submit would bypass the engine's transfer-worker and
swapchain-acquire waits, and could allow its frame pool to reset a live buffer.

The inspected engine is Godot revision
`ed1daf0bf001b61586d9930840f2f1394092c079`. The relevant source owners are
`RenderingDeviceGraph::_run_render_commands`, its `CommandBufferPool`, and
`RenderingDevice::execute_chained_cmds`. The graph already splits buffers for a
driver workaround. The device submits the initial frame buffer followed by the
pool's used buffers, links them with binary semaphores, preserves the original
first-section transfer waits and uses the final frame fence only on the last
section. This existing path is the reason for using a recording lease.

`GraphScope` temporarily exposes the graph's current buffer reference and pool
around `TYPE_DRIVER_CALLBACK`. Its handoff callback validates the active driver
and exact native command-buffer identity. The handoff's retained user pointer is
the driver, not the stack scope. A thread-local scope resolves that stable
identity only during synchronous graph execution. Other callbacks and D3D12
continue through their existing paths.

The split allocates or reuses a continuation in the engine pool, including its
normal binary link. Allocation and `command_buffer_begin` happen before the old
buffer is consumed. It closes the active graph debug label, ends the old buffer,
replaces the graph's reference, and reopens the same command label on the new
buffer. No queue submit occurs in this callback. A failure after consuming the
old buffer is terminal because recording further commands into it is invalid.

The Vulkan driver's per-buffer metadata records the external semaphore and
values. The old section waits for the preceding FSR4 `done` value and signals
`ready`; the continuation waits for the new `done`. Queue-family identity comes
from creation of the command pool and is copied into each buffer. Metadata is
cleared when a retired buffer begins its next recording. The submit path appends
the external timeline dependencies, preserving every binary semaphore and giving
each binary entry value zero in `VkTimelineSemaphoreSubmitInfo`. It refuses to
combine multiple external sections into one submit, which could otherwise wait
for a producer in the same submit. Godot's existing chained path submits one
buffer at a time and announces each through `vk_submitted`.

The private native FSR4 service copies inputs and releases external ownership in
the old section. Its D3D12 queue waits for `ready`, evaluates and signals `done`.
The continuation acquires and copies the result. SDK refusal after a successful
split still leaves both sections in the engine's normal submission list; the
native host records its fallback in the returned continuation. No engine pool is
reset outside the existing frame-fence lifecycle. Native SR/frame ABI 1 remains
unchanged; the module uses a separate handoff ABI and advertises its capability
before native boot.

Checks completed on 25 October:

- `eng/patch-godot.py` applies and `--check` passes against the exact pinned
  engine revision. `git diff --check` passes.
- `tests/godot_graph_handoff.cpp` compiles the actual `graph.cpp` against narrow
  fake engine headers with Windows clang++ and `-Wall -Wextra -Werror`, and
  passes. It covers active lease identity, disabled timelines, invalid values,
  allocation/semaphore/begin refusals before consumption, continuation allocation
  and reuse, label balance, and preserved continuation ownership after refusal.

- The coordinated Windows standard and .NET SCons editor builds both completed.
  Their SHA-256 identities are
  `DE9BA015EDC32412380BE6A57B59118BAC604517538C957371ADF6D8946145CB` and
  `4C6461FC557BE416EAF35207A92B7963ECC3487B8FE224C36F9AA33729A594D0`.
  Build logs are `build-engine-sr4-graph-standard.log` and
  `build-engine-sr4-graph-mono.log` in the ignored Godot evidence directory.

These checks alone did not establish actual timeline submission or image
acceptance. The first matched runtime results below add narrower device evidence.
The earlier standalone FSR4 interop fixture remains separate evidence.

The SR4 functional fixture initially switched to DLSS at frame 60 and resized
only that second provider. It now returns to FSR4 at frame 90 before resize and
history reset, checks FSR4 at frame 125, and verifies that the .NET C# facade
observes the resized FSR4 provider at frame 150. Other initial providers retain
their original fixture behavior. This correction prevents a passing DLSS resize
from being reported as FSR4 resize evidence.

## First matched runtime results

The candidate native DLL is
`19BC537D1FF40A863C91690B2CAE39D005FD3760B65699DEECDF336A2B34F301`, with
the standard and .NET editors identified above. Runs use the immutable fixture
payload cache and named GPU lease. The portable Khronos validation layer is
`E86FA4F70676A120F00A32BEAC6979CEB3CAFCE70DE42FA9107C89F587AB206E`.
Synchronization validation is requested and reported enabled. The process
extension-point policy is 1, with RTSS injection and implicit layers excluded.
SDK trace counter queries are disabled.

The standard functional scene `sr-20261025-201352-429` finishes with no scene
failures. Its first snapshot has active SR4, result zero, 44 evaluated frames and
no refusals at 426×240 to 640×360. After switching to DLSS and returning to FSR4,
the resized snapshot has active SR4, result zero, 123 evaluated frames and
533×300 to 800×450 output. The one refusal is the expected DLSS switch-frame
426/427-pixel input-plan mismatch, followed by spatial fallback and convergence.
Real player PNGs retain the visible cyan object before and after resize.

That first standard harness invocation is **not a harness pass**. A redundant
deprecated `VK_LAYER_ENABLES` setting produced a settings warning beginning with
`vkCreateInstance():`, which matched the existing API-error guard. No VUID or
synchronization hazard was reported. Subsequent runs clear the deprecated setting
while retaining `VK_VALIDATION_VALIDATE_SYNC=1` and the reported Synchronization
check. The original failed receipt and settings warning remain retained.

The .NET functional fixture `validation-20261025-201640-075.json` passes the
complete strict harness on this candidate. It covers SR4 to DLSS, return to SR4,
resize/reset, real player PNGs and C# effect creation/status. The C# facade
observes active SR4 after resize. Its resized snapshot also reports result zero,
123 evaluated frames and the one expected DLSS switch-frame refusal. There are
no reported VUIDs or synchronization hazards. This run omits raw SR capture due
to the separate diagnostic problem below.

`visual-20261025-201525-585.json` passes the standard combined-motion image case
with SR4, FSR3 FG, latency mode zero and a fresh matching 8× MSAA reference. All
four actual generated frames 12 through 15 pass the unchanged geometric and image
gates. Maximum material-centroid error is `0.1763010601` output pixels; maximum
interpolation residual after source displacement is `0.1120420613` pixels. Final
status reports active SR4/result zero, 89 evaluated frames, no refusals, FSR3 FG
active, 89 generated callbacks and 89 tagged sources. Both captures use the same
native and engine hashes. The frame-13 comparison PNG was opened. This run has
loaded Khronos validation, reported Synchronization and no VUID or sync hazard.

The compact ignored `fsr4-graph-first-20261025-summary.json` retains the identities,
functional samples, C# result, layer fingerprint, mitigation evidence, exact
image errors and original failed raw-capture roles. The results establish these
bounded Godot paths; they do not establish broad FSR4 image quality or lower
latency. A .NET generated-image comparison and the full motion matrix remained
pending at this first checkpoint. The corrected-package .NET comparison is
recorded below; separate object-only and camera-only cases remain outside scope.

## Raw output diagnostic correction

The first SR4 raw capture records producer color, depth and motion for all four
selected sources, but fails `sr-output-linear` for frames 30 through 33. The
scene itself succeeds. Source review identifies the cause: the hook passes the
engine's upscaled image to `FrameCaptureVk::record`, which correctly refuses its
`copy_source=0`. The engine allocated this image for copy destination use. No
missing raw output is counted as successful SR capture evidence.

The correction is to record the provider's private shared output in the
continuation after external acquire, while it is in `TRANSFER_SRC_OPTIMAL`, and
before releasing it back to external ownership. This image already has transfer
source usage and contains the actual SDK result. The native correction and a
fresh matched raw-capture rerun were pending at this first checkpoint. The existing
FG-generated image readbacks are complete and their passing comparison above is
independent of this missing SR diagnostic role.

## Corrected raw output and both-edition image acceptance

The fresh native DLL is
`4765226F3364854814CBDA3AC5516F9F388A49FD6E5F31ABD9FCB76BA3A46EC2`.
Its corrected capture hook reads the private output before external release.
Both engine identities and SDK payloads remain unchanged from the preceding
candidate. Root's coordinated verification reports 68 tests, 57 executed and 11
skipped, plus formatting and Clippy success. The prior loader-test failure was
resolved by repairing a stale artifact identity with the same bytes; it was not
suppressed or bypassed by source changes.

Fresh matching standard and .NET combined-motion pairs pass:

- `visual-20261025-202351-936.json`, standard.
- `visual-20261025-202302-354.json`, .NET including C# facade success.

Each pair uses a fresh matching 8× MSAA reference and the same native/engine
hashes for generated and reference captures. All eight actual generated images
pass the unchanged gates. Each final status reports SR4 active, result zero,
89 evaluated frames, no refusals, FSR3 FG active and 89 generated callbacks.
Maximum material-centroid error is `0.1763010601` output pixels and maximum
interpolation residual is `0.1120420613` pixels in both deterministic fixtures.
Latency mode is zero. Loaded Khronos validation and reported Synchronization
are verified, with no VUID, validation error or synchronization hazard. Process
mitigation is 1 and RTSS modules are absent.

The raw SR analyzer also passes on the same captures, with four complete frames
per edition in `frames/decoded/analysis.json`. These contain actual producer
color, normalized depth, motion and private SDK SR output. Their geometry,
camera metadata, jitter, finite values, output extent and nonconstant output
checks pass. There are no `capture-errors.log` files. The .NET frame-14 SR guide
PNG was opened, showing the input, actual SDK output, depth and motion panels.
This directly corrects the preceding missing-output diagnostic result without
changing an image gate or the engine output allocation.

`fsr4-graph-final-20261025-summary.json` retains both paired identities, analyses,
final statuses, C# result, validation and process-isolation fingerprints, output
metadata and error maxima. These are device-tested standard/.NET combined-motion
results with FSR3 FG, plus the preceding provider-switch/return/resize/reset
evidence. They do not establish broad FSR4 image quality, lower latency, or a full
object-only/camera-only motion matrix.
