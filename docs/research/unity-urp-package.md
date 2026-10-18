# Reusable URP integration, 18 October 2026

## Requirement and source

Provide a central Unity project integration with full shared SR, FG and PCL on DX11,
DX12 and Vulkan. RTSS compatibility is a hard acceptance requirement: RTSS stays enabled
during validation, including active frame generation and runtime provider changes.
Canonical source is `integrations/unity`; the open development project
is `C:/Users/N1GHT/ReScaleFrame`, Unity `6000.6.6f1` (`d20c9ac777da`), URP `17.6.0`.
This is distinct from the shipped-player Mono/Harmony adapter for Drag'n Wash.

Source inspection found URP's `IUpscaler`, `UpscalerRegistry`, `UpscalingIO`, context
management and injection path behind `ENABLE_UPSCALER_FRAMEWORK`. Enabling that switch
compiled the installed core and universal packages and exposes the intended custom
upscaler interface. The initial missing-type compilation errors and the obsolete
`GetInstanceID` call were corrected; camera identity uses `EntityId`.

The first editor Play attempt exposed a real loader error: `rsfu_initialize` was not
resolved, and Unity then reported that `Library/ScriptAssemblies/ReScaleFrame.Unity.dll`
was locked. Both the native plugin and managed assembly had the same filename. The
native artifact and P/Invoke library were renamed to `ReScaleFrame.Unity.Native.dll`;
the native export table contains `rsfu_initialize`, `rsfu_surface` and `UnityPluginLoad`.
Editor-native compatibility is enabled for preview acceptance after restaging.

Unity 6.6 serializes plugin platform data as a dictionary. The initial staging script
incorrectly wrote the older list format, leaving Editor compatibility disabled. The
importer API corrected this; staging now preserves Unity-authored metadata. P/Invoke
uses the complete DLL filename including `.dll`. A subsequent editor run evaluated
FSR3 at 326x166 to 979x498 with 2,620 accepted and zero refused evaluations. This was
SR evidence only, not FG/PCL or all-provider acceptance.

Subsequent tests exposed the editor's missing presentation owner and a Streamline
lifetime conflict: DLSS creation returned `RSF_BACKEND_ERROR_NOT_READY` while the
package's own interposer remained loaded. The standalone-SR guard refuses a second
registration. The new editor Game-view surface establishes the shared presentation host
before SR creation. Quality/model changes free the SR viewport at the drained boundary
without shutting down that shared SDK. A native-session epoch keeps source IDs distinct
across editor Play restarts.

The first HUD also sent FG Auto mode 2 to a Fixed-only path; it now sends Fixed mode 1.
A refused request retains the previous configuration and permits later valid requests.
Ultra Quality stays visible but disabled for DLSS, whose shared SDK plan accepts quality
IDs 0 through 4. Explicit invalid DLSS requests retain the previous configuration.

The first run of the shared host exposed a render-graph exception in the HUD-less
capture: at `AfterRendering`, the active colour handle referred to the built-in
backbuffer and had no sampleable texture descriptor. URP 17.6's
`UniversalRendererRenderGraph.OnAfterRendering` keeps a sampleable post-process
intermediate when an `AfterRenderingPostProcessing` pass is queued. Capture now uses
that boundary on every API, before final backbuffer/UI composition, and converts to
the shared RGBA8 display contract. Completed-frame presentation remains after UI.

Unity's installed PluginAPI headers establish native texture and queue ownership.
Unity's native plugin `UnityPluginLoad` and device events replace game fingerprinting
and private registry lookup for this entry point. The existing managed game helper
is not loaded. The DX12 submission implementation is shared with the game adapter;
the package supplies its own host and DX11/Vulkan interop.

The local Streamline SDK is 2.14.1 (`.local/streamline/sdk`); guides for
[SR](https://github.com/NVIDIA-RTX/Streamline/blob/v2.14.1/docs/ProgrammingGuideDLSS.md),
[FG](https://github.com/NVIDIA-RTX/Streamline/blob/v2.14.1/docs/ProgrammingGuideDLSS_G.md)
and [Reflex](https://github.com/NVIDIA-RTX/Streamline/blob/v2.14.1/docs/ProgrammingGuideReflex.md)
govern feature setup and frame-token ownership. Vulkan's
[D3D12 fence submit contract](https://docs.vulkan.org/refpages/latest/refpages/source/VkD3D12FenceSubmitInfoKHR.html)
provides explicit values for imported fence semaphores without requiring a new
timeline-semaphore feature chain. Resource imports use same-adapter LUID identity and
the committed-resource external handle type.

## Implemented path and lifetime

The URP upscaler records HDR scene colour, raw depth, URP current-minus-previous UV
motion, the actual subpixel jitter, non-jittered transforms, render/output extents and
frame delta. The shared native reconstruction normalizer converts motion to SDK pixel
units. URP's motion-vector shader applies the NDC-to-UV half scale and top-left Y
conversion; the package does not repeat that half scale.

Per-camera history tracks discontinuities and native settings generations. A complete
fallback is recorded before dispatch. Cross-API output buffers receive that fallback
before evaluation so SR Off, FSR1 and refused vendor work cannot copy uninitialized
output over it. Temporal jitter remains off until the native temporal context exists.

Native packets carry the same nonzero player-loop source-frame identity through scene
capture, pre-UI capture and presentation. Pacing precedes input/simulation; submission
and Present markers come from the native execution/presentation owners. Win32 input
and PCL messages are forwarded; Input System state events cover controller consumption.

DX12 uses Unity's submission fences. DX11 uses shared NT textures/fences and same-adapter
D3D12 execution. Vulkan adds external-memory/semaphore device extensions during native
preload, acquires Unity texture access, flushes preceding work through `AccessQueue`,
and queues resource copies around shared D3D12 work. Imported image ownership returns
to external/general state between APIs. Command-slot and resource retirement wait for
their GPU completion, rather than inferring lifetime from frame numbers.

Vulkan copies the completed composed image at `vkQueuePresentKHR`, then presents through
the runtime-owned DXGI child surface. Per-image ready semaphores preserve the original
Vulkan swapchain wait contract. The parent Unity window retains input/focus. The editor
uses the same native facade on its Game-view surface, receiving the completed Game-view
texture after UI from an end-of-frame coroutine. Editor presentation, standalone
presentation and each graphics API need separate runtime evidence.

## Evidence in progress

- Windows VS2026 Release `rsf_unity_package` builds, including DX11, DX12 and Vulkan.
- The test editor compiled runtime, editor, Input System and demo assemblies.
- The package is registered by local UPM path in the development project.
- A separate animated validation scene was saved without replacing the original scene.
- The first standalone player built with DX11, DX12 and Vulkan shaders.
- `eng/verify.ps1 -Configuration Release -VS2026` completed after the shared editor-host
  changes: 45 native tests passed, eight environment/vendor checks skipped, Rust
  formatting and Clippy clean. The native package target and managed assemblies built.

The initial shared editor host, completed-frame presentation and FPS API were built and staged.
Unity released the old DLL before replacement. The staged native SHA-256 matches the
Release build: `92dd4163b4312ea022715c53d7c3b98115478839cd43cb39a36d3a7d4c8b7086`.
After reopening, the DX12 editor run evaluated DLSS with zero refusals and reported
active DLSS FG, Reflex sleeps, PCL availability and driver latency reports. Live
quality changes reached Performance (604x351) and Ultra Performance (402x234), both
to 1207x702. A model M / four-times MFG request reported three effective generated
frames with 117.58 Base FPS and 470.33 output FPS from presentation-counter deltas.
The status snapshots and Game-view texture capture are under
`.local/unity-urp-validation/editor-dlss-*`. The human confirmed upright scene/HUD
orientation. Repeated Play then exposed a separate lifecycle failure: a
`HideAndDontSave` coroutine owner survived Play with no scene membership, stopped
presenting and left the shared token ring occupied. The runner now uses
`HideInHierarchy` in the normal persistent scene. Native session teardown retains
ownership until FG retirement completes and aborts that session's remaining CPU tokens.
This replacement still needs editor reload and repeated-Play acceptance.

The updated standalone player built successfully. DX11 initially missed FG because
the presentation bridge rejected flip chains with more than one engine buffer. It now
allocates and retires the requested shared buffer set. Shared SR execution also uses
the native device/queue aliases, so native resource ownership is compared consistently
instead of comparing a native resource owner to a proxy device. The subsequent
`dx11-v3` run exited cleanly with 4,292 DLSS evaluations, zero refusals, 4,290 tagged
frames and advancing PCL/sleep markers. Its native DLL SHA-256 was
`5e97354df7a2a5f22eea5b4134c24445542a2b9c7467741dca20aaf3352c61ed`.

Vulkan's first run hung in `NtUserShowWindow` on the submission thread while Unity's
main thread waited for rendering. `.local/unity-urp-validation/vulkan-hang-stacks.txt`
records the captured stacks. The owned surface now uses `ShowWindowAsync`, and initial
shared presentation registration happens on the window thread before SR. The
`vulkan-v2` run exited cleanly with 3,552 DLSS evaluations, zero refusals, 3,551 tagged
frames and advancing PCL/sleep markers, using native SHA-256
`8d2be4aedddb35e10a4cfff069c0ec4bc73a48aaad2cfe926ed6d6b7593397a2`.
These two background runs did not establish active generation; the driver reported
foreground zero. Foreground-test authorization is pending.

Standalone DX12 failed before scene rendering. The D3D12 debug-layer callback captured
RTSS's D3D11-on-12 overlay submitting work on the engine queue to DLSS-G's pacer-owned
backbuffer. The trace is in the older player's `ReScaleFrame.log` under
`Ownership call`, including `RTSSHooks64.dll`; debug logs are `dx12-*-player.log`.
Trying an explicit Streamline factory-proxy preference did not fix it and was reverted.
Temporary ownership instrumentation was removed after identifying the caller. A
test-player-only RTSS exclusion was incorrectly proposed. The human reaffirmed that
RTSS compatibility is required, so that proposal is withdrawn. No RTSS configuration
has changed. The queue association and overlay ordering must be repaired with RTSS
enabled; exclusion is not acceptance.

The installed RTSS hooks SHA-256 is
`68c496dea7ded5b8766092f0159f4e9ba3947df0829f594eb6981d2b06352ad5`.
REA/Ghidra inspection found creation queue state in `0x18005b2e0` and a post-creation
cache update in `0x18005b7c0`; queue selection is in `0x180047f00`/`0x180047ea0`.
Evidence `ev_677f8271932321303752b83dd87dc915ddf3bd2c2e94d8ad8730cb3d25576d4e`
and `ev_7846510c84d718251568d8d1253ff9f6ba00a1506f1a75d75573b3af2f06ff0f`
show global creation state rather than a nested call stack. The retained analysis is
`.local/unity-urp-validation/rtss-68c496de-analysis.json`. The facade now prepares its
provider on the first engine use after returning from the outer factory callback.
This removes the nested provider/native creation that corrupted the overlay's queue
association. A proposed `ResizeBuffers1` rebinding experiment hit an exception in the
resize path and was removed. Changing plugin-event binding flags did not remove the
separate depth-state errors and was also reverted.

The first FSR3 transition then exposed Unity's retained physical-provider wait handle.
Matching Unity symbols identified `SwapChainWaitingContext::WaitOnSwapChain` while all
graphics workers were idle (`rtss-fsr3-transition/wait-symbols.txt`). Runtime-switching
facades now own the application latency event and signal availability from their copy
completion fence. The returned duplicate belongs to the caller and survives physical
provider replacement. Non-waitable facades return no handle, following the
[DXGI contract](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_3/nf-dxgi1_3-idxgiswapchain2-getframelatencywaitableobject).
The native hot-switch fixture now retains one handle while changing providers; the
explicit GPU run passed with active FSR3/XeSS samples and preserved buffer identities.

RTSS-enabled Unity player runs now report active FSR3 generation, successful DLSS SR
and PCL markers on all three APIs. The captures show Base/output FPS separately; the
DX12 capture reads 290.2 / 580.5 FPS. Results are under `rtss-stable-waitable`,
`rtss-dx11-fsr3` and `rtss-vulkan-fsr3`; module snapshots confirm RTSS was loaded.
All three exited normally. Native SHA-256
`3f74260edb4e3a31a69d08c335bc6cb459df93e955cc8aab653d459c27ef6e30` is staged in
the package and test player. The native gate passed 45 tests with eight optional skips.
The DX12 depth-state investigation now has a matched stock-URP control. With
Bilinear, render scale 0.67, the ReScaleFrame renderer feature disabled and the
native runtime path empty, `rtss-stock-urp-scale` reproduced 6,898 depth-backbuffer
validation errors (527/538) and exited normally. Its module snapshot contains RTSS
but no Streamline interposer. Full-resolution stock URP had no such errors. The
earlier `rtss-no-native-events` and `rtss-no-native-handles` controls also reproduced
the errors, excluding native render callbacks and texture-pointer lookups as the
cause. Unity tracks this matching engine defect as
[UUM-136054](https://issuetracker-mig.prd.it.unity3d.com/issues/multiple-d3d12-errors-are-thrown-when-playing-urp-based-builds-due-to-d3d12-depth-backbuffer-barrier-mismatch).
Moving pointer lookups and splitting input preparation into raster passes did not
fix it; those experiments were removed. This is an engine validation limitation,
not evidence that RTSS must be excluded.

The reopened DX12 editor (native hash above, RTSS loaded) completed a Stop/Play
cycle and resumed DLSS evaluation and PCL markers. A live switch to FSR3 SR/FG
reported active generation with Base/output FPS 13.37/26.74 while another debug
player was running. Switching back to DLSS SR/FG resumed SR and PCL without a hang;
one transient SR refusal occurred across provider changes. Foreground DLSS-G/MFG,
visible RTSS OSD and complete provider/image acceptance remain open. Background
rendering was enabled temporarily for this editor validation; the project's
normal setting was false and initially held Play at frame two when unfocused.

The subsequent DX11 image-contract check found a defect hidden by successful SDK
counters. DX11 retains its buffer-zero render-target view across Present, while DX12
applications rotate explicitly, as described by Microsoft's
[DXGI overview](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/d3d10-graphics-programming-guide-dxgi).
The facade had rotated among ordinary shared textures, which cannot perform DXGI's
automatic backing-resource rotation. The new `11-flip` GPU fixture retains one DX11
view, changes its colour every frame and reads a pixel from the physical presentation
copy. Before the correction, frame two was RGBA 0/0/0/0. The DX11 facade now presents
logical buffer zero and reports index zero; DX12 keeps explicit indexing. Its final
copy also uses the provider creation queue and the corresponding native/proxy list,
instead of the separate SR interop queue. The corrected fixture passed all 40 pixels
through DLSS/FSR3/XeSS/Off changes. The test waits on the presentation queue; waiting
on the SR interop queue had produced false stale-read failures during Off.

The updated native SHA-256 is
`228d7755db1a70a9cd74ce388bed97f77fff8ad7fd6b2a054e050e49ac06caa5`.
It is built and copied to the isolated current Unity player; the open editor still
owns the previous package DLL and must close before staging. The current Unity
player built successfully through Unity's build pipeline, rather than only the
private managed diagnostic compiler. FSR4 initialized and evaluated successfully,
but its first run spent most of the short validation interval initializing and does
not establish sustained image quality. Full foreground FG acceptance, current package
delivery and measured latency improvement remain open. Build and SDK counters do
not establish image quality or a latency improvement.

Delivery was reconciled with upstream `01635b8` in a separate clean `main` checkout,
preserving the original checkout's unrelated Wingman and motion-source edits. The
native build uses the current 0.1.1 core; the Unity package retains its own 0.1.0 version.
The current native SHA-256 is
`2180e274643764744c1ea57078d61f5411c8d7071906af30f5b0b7cf8db11ccc`.
The delivery gate passed 44 native tests, eight optional skips, formatting and Clippy.
The original checkout's 45th test was unrelated pending Wingman work and is not part
of this delivery. The demo's effective multiplier now reads one while generation is
inactive, independently of the requested/configured multiplier.

The real XeSS SR/FG run exposed an additional identity defect: Unity supplies 64-bit
session/frame identities, while XeLL and XeFG accept 32-bit counters. Rejecting every
identity above `UINT32_MAX` permanently disabled generation. The XeSS provider now
assigns local SDK counters and retains a bounded source-to-SDK mapping for CPU markers,
resource tags and Present. It does not truncate the session epoch. The GPU hot-switch
regression now starts above 32 bits and changes epochs while XeSS remains loaded; it
passed with seven active XeSS samples, nine FSR samples, preserved buffer identities
and one retained application latency handle. `delivery-dx12-xess-fixed` then ran XeSS
SR with active XeSS generation and zero SR refusals in Unity with RTSS loaded.
`delivery-dx11-fsr2` also ran FSR2 SR with active FSR3 generation and zero refusals.
`delivery-vulkan-fsr1` ran FSR1 with active FSR3 generation and zero refusals, completing
the current native build's three-API execution check. Each player exited normally;
module snapshots include RTSS, including its Vulkan layer. The verified UPM archive is
`.local/packages/com.kpc.rescaleframe-0.1.0.tgz`, SHA-256
`fa63e32dc79dfa66e33d7b6f7e77d204b76d2bb8dc109836d0f1d44ec44f526c`.
Extraction checks matched the native DLL, package identity and effective-multiplier HUD
source. These are SDK execution and presentation proofs, not broad image-quality acceptance.
