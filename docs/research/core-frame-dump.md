# Core frame dump

26 October 2026. The orchestrator owns one requested frame capture per process.
F10 and the C API select the next eligible primary frame. Integrations supply
real resources at their production/consumption boundaries; no texture matcher,
screen-size guess, shader replacement or second runtime is used.

The shared `ReScaleFrame::FrameDump` library links into the existing runtime
owner: AC7's carrier, Unity's runtime DLL, Minecraft's native bridge and Godot's
native extension. PNG encoding, raw storage, metadata, request identity and
completion are shared. The image codec also accepts CPU readbacks, so a new
graphics adapter can use it without a vendor SDK or a D3D11 device.

## Capture contract

`runtime/orchestrator/include/rescaleframe/input_dump.h` is C ABI version 1.
Structures are size checked, frame records use frame ABI 1, and resources remain
borrowed. No STL value, exception or caller-owned allocation crosses the ABI.

1. Poll on the graphics owner, or call `rsf_input_dump_request`. Configure an
   optional UTF-8 output directory/game identity before rendering starts.
2. Call `rsf_input_dump_begin_record` with the real session/frame/view/generation
   record. That identity selects all later images; different views, generations,
   sessions and stale request tickets are refused.
3. Capture each named image while its lease and contents are valid. D3D11 can
   use `rsf_input_dump_texture_record`. D3D12 and Vulkan helpers record readback
   copies into the owning command stream and restore the declared resource state.
   Another API can reserve an image and submit borrowed CPU rows through
   `rsf_input_dump_pixels` after its own GPU completion.
   `input_dump_d3d12.h` and `input_dump_vulkan.h` expose opaque C readers so C/Rust
   integrations need not use the internal C++ readback helpers. D3D12 readers retain
   source/readback references until submitted fences complete; unsubmitted recordings
   must be discarded before their reservations are abandoned.
4. Publish SR parameters/results and additional native pass metadata. `frame.json`
   records camera availability, dimensions, jitter, transforms and history identity;
   `sr_parameters.json` records the submitted motion scale, exposure, reset and SDR
   declaration. An absent camera is explicitly unavailable rather than invented.
5. Complete the same frame at actual Present completion. Pending GPU reservations
   keep the manifest open. Cancel the owning session at teardown after its readers
   retire normally. Cancellation invalidates tickets without freeing GPU resources.

D3D12 owners flush after the exact recording slot completes, or attach an actual
submission fence and poll it. A removed-device fence is an error. Callback lists
must be unpredicated, or their engine owner must suspend and restore predication
through its own state tracker: D3D12 can predicate copies and has no getter for
the current predicate. [Microsoft's predication contract](https://learn.microsoft.com/en-us/windows/win32/direct3d12/predication).
D3D11 readbacks suspend/restore their queryable predicate, as checked by the fixture.

Vulkan copies retain their buffers through execution, include the host
memory dependency and restore the image layout. An actual source-queue fence join
certifies only the readbacks already offered for that frame/device; later copies
need a later join. Uncertified recordings are refused at drained destruction. The explicit
snapshot helper joins a bounded fence on an engine-serialized graphics submission,
without consuming Present's semaphores. An unsuccessful join preserves the owner.
This uses a queue-to-host fence dependency, not an event-state query.
[Vulkan synchronization contract](https://docs.vulkan.org/spec/latest/chapters/synchronization.html).
Adapters must declare transfer-source usage. Godot requests that usage only when
the surface supports it and registers readback ownership separately from FG,
including its native-WSI path. Release joins source work before freeing the engine's
queue callback or swapchain. Capture support does not narrow ordinary FG admission.

The default output remains `%LOCALAPPDATA%/ReScaleFrame/<executable>/website/scene-*`.
AC7 keeps `AC7`; Minecraft identifies itself as `Minecraft`. `RSF_DUMP_DIR` overrides
the base. Each bundle has opaque PNGs, packed lossless `.bin` rows, `_raw.json`,
camera/parameter/status files and a manifest. Native API/format are recorded
separately from the equivalent DXGI storage encoding used by the preview codec.
UTF-8 paths work for both PNGs and raw files. Concurrent/unfinished requests cannot
mix frames; filenames include a process/request identity.

Optional or unproduced resources stay unavailable. Detection-only plugins cannot
produce temporal inputs until their renderer adapter exists. A separate HUD must
come from the integration's actual UI producer; the service does not derive a fake
HUD by subtracting images. Internal vendor history is not an exposed input.

## AC7 boundary correction

The first capture used only NimbusHUDCombine (role 5) and the final engine scene.
The user's `scene-20261026-191635-943-355613625`, source 9153/native frame 8236,
showed menu UI already composed in that late scene and no separate HUD. Treating
the late FG input as universally HUD-less was wrong.

The existing guarded Nimbus composite at RVA `0xfc86c0` carries the menu's scene
in graph input zero and its separate UI in input one. The plugin now leases both
native pools and publishes that existing pass through the standard UI-composite
boundary. The native HUDCombine path still leases its view-owned raster.
No new binary patch/site was introduced. The dump captures the pre-UI scene and
UI at begin, then the readable composite at end. The later actual FG input is named
`fg_scene_color`; it can contain UI and is preserved as such.

The user-run retake `scene-20261026-195414-807-357872500`, source 7167/native frame
4651, was inspected as PNGs: the pre-UI image contains only the hangar/aircraft,
the HUD image contains the menu alone, and the final image contains both. All six
UI/final stages were captured at 1600x900 with one matched source identity.
The stored UI alpha can remain zero for this producer; it is exported unchanged,
not advertised as a complete compositable alpha layer. This correction does not
change the game's existing FG input or establish broader FG visual acceptance.

## Evidence

Source baseline: `ba9a486`. The primary workspace's 83-case Release batch passes,
with 11 opt-in skips, plus formatting and Clippy. The staged feature was separately
exported without the existing RDR2/census/overlay edits and verified from that clean
source: 78 discovered, 67 executed successfully and 11 opt-in skips; formatting and
Clippy pass. Logs: `.local/core-dump-verify-fixed.log` and
`.local/core-dump-scoped-verify.log`. The shared build-file changes were scoped
against HEAD so those unrelated entries and source files remain outside this commit.
The first batch exposed an ANSI conversion in the Unicode fixture's console print;
its three image bundles were already complete/cancelled correctly. The fixture
print was corrected without replacing the valid production files.

D3D11/WARP checks cover independent PNG decoding/opaque alpha, exact HDR bytes,
mask/depth/sparse/dense motion mappings, predicate restoration, Unicode paths,
frame/generation/ABI mismatch refusal and cancellation. The D3D12/WARP fixture
copies real GPU pixels, rejects premature readback, packs padded rows exactly and
completes only after its submitted fence. The Vulkan fixture uses a real device,
source clear, queue submission, layout restoration, readback fence and original
alpha bytes without any vendor SDK. These fixtures are GPU/API evidence, separate
from new Unity/Minecraft/Godot gameplay acceptance. AC7's UI retake above is game
capture evidence for that scene only.

The real hidden OpenGL integration fixture exercised SR Off/DLSS/FSR2/FSR3/XeSS,
FG switching and resizing, then verified its core source/HUD-less/final raw bundle
and SDR declaration. `.local/core-dump-opengl.log` records success. The separate
Vulkan readback ran on Intel UHD 770 with the Khronos layer, synchronization
validation and RTSS's Vulkan layer loaded; no validation errors were reported.
Log: `.local/core-dump-vulkan-validation.log`. This does not establish all-device
support or game visual/latency acceptance. The C-only header target and public
D3D12-reader fixture validate the new integration-facing interfaces.

Minecraft's 43 Java/GPU checks pass. The native payload was restaged, rebuilt into
the JAR, and its embedded DLL/hash manifest compared with the final DLL before
deployment. Godot's full native add-on, required Vulkan SDK payloads, standard
custom editor and .NET custom editor built successfully. The C# facade builds
against the exact 4.7.2 GodotSharp assembly with no warnings. Godot also exposes
`ReScaleFrameRuntime.request_frame_dump()` and C# `ReScaleFrame.RequestFrameDump()`;
both queue the request on the rendering thread.

AC7's proxy/plugin/overlay and SDK files are backed up and deployed through the
established script. Unity's owned runtime/plugin pair and Minecraft's existing JAR
were backed up and hash-verified after replacement, without changing settings or
other mods. The existing Minecraft JAR filename is retained for Prism's cache.
Godot's package is at `.local/godot/package/addons/rescaleframe`; custom editor
manifests are at `.local/godot/engines/{standard,mono}/editor.json`.
Primary-workspace delivery builds include the separately preserved SDK work;
the feature-only source build above independently verifies this commit's scope.
Deployment receipts: `.local/deploy-backups/core-frame-dump-20261026-225301-249`,
`ac7-pair-20261026-225529-108` and `ac7-sr-20261026-225528-599` beneath that backup
directory. No user's game was launched or stopped. A new foreground Godot capture
fixture was not run; its harness explicitly takes PC focus and approval remained
pending. New gameplay captures outside the accepted AC7 scene remain unverified.
