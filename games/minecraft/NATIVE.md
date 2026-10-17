# ReScaleFrameCraft native integration

This module imports the accepted `rescaleframecraft` 1.0.2-dev source at `4e45c46`.
The original standalone checkout remains available as a reference. Native vendor execution,
interoperability and presentation use the shared C++ runtime; the Java adapter owns Minecraft's
render targets, camera conventions, input frame and HUD boundary. No overlay is created.

Build the native tree with `RSF_JAVA_HOME` pointing to a JDK with JNI headers. Build target
`rsf_minecraft`, then run `eng/stage-native.ps1 -SdkWorkspace <SDK-equipped ReScaleFrame checkout>`
and the Gradle `test jar` tasks. The package contains the native DLL, an explicit retail vendor
runtime allowlist, licenses and SHA256 manifest. Extraction is content-addressed within the
instance, so updating the mod cannot overwrite a loaded native module.

Sodium's public Config API supplies a ReScaleFrame page to Reese's Sodium Options. SR quality,
FG provider and generated-frame count, latency settings and the rendered-frame cap apply at a
drained render boundary. F3 distinguishes requested and effective providers. Unsupported explicit
requests retain a native rendering path and report the refusal. Auto is an actual backend choice,
not evidence that a vendor has activated.

Once WGL has presented the loading screen, DXGI refuses that same HWND with `E_ACCESSDENIED`.
The harness now reproduces this sequence. A child compositor surface provides a separate DXGI
owner inside the GLFW window. Hit testing passes through to GLFW and the child never takes focus.
The GL swap is suppressed when native presentation is active. The corrected sequence passes.
With Ixeris, the GLFW event thread differs from the render thread. The public
`IxerisApi.query(Supplier)` creates the child on the GLFW owner; the runtime checks this ownership
and routes resize/destruction to that thread. Graphics and presentation stay on the render thread.
A cross-thread GPU regression exercises this exact split, input hit testing and shutdown.
Ixeris API reference: tag `4.6.8`, commit `972093d5e3cc5f2dcd0f61a1198b9a603caac1c5`.

GL exports vertically flipped RGBA16F SDR scene colour, canonical R32 depth and dense RG16F
current-minus-previous pixel motion. The shared normalizer produces vendor previous-minus-current
pixels. SR resolves before hand/HUD. A separate display-sized RGBA8 HUDless image and the final
composed image go to FG; the supported backbuffer/HUDless mode includes hand in the preserved UI
difference. The input-boundary frame ID travels through evaluation, HUDless capture and the final
DXGI buffer association. Menu, pause, missing world and reset frames cannot interpolate.

The real GPU harness (`RSF_RUN_GL_INTEROP_TESTS=1`, `RSF_GL_TEST_RUNTIME=<staged directory>`) uses
a hidden WGL window. It checks both shared-memory directions, all four formats over changing
dimensions, native SR evaluations and independent FG provider switching. The NVIDIA driver
deferred memory import past the API return; retaining each NT handle for its imported object's
lifetime fixed `GL_OUT_OF_MEMORY` with synchronous GL debugging disabled. Relevant contracts:
[Khronos external Win32 objects](https://registry.khronos.org/OpenGL/extensions/EXT/EXT_external_objects_win32.txt)
and the pinned Sodium/Reese source APIs. No external implementation was copied.

Passed on RTX 4070 Laptop, driver 616.92: Off/FSR2/FSR3/XeSS/DLSS SR evaluation, SDR output
colour checks at all four corners and centre, FG Off/FSR3/XeSS/DLSS
switching, input preparation and HUDless tagging. A hidden window does not prove displayed
generated frames, visual quality, frame pacing or live modpack acceptance. The 14 inherited GPU
TAA regressions plus dense-guide and resource-retirement tests passed independently on Intel UHD770. The actual
Sodium builder regression accepts the complete settings page, including required slider formatters
whose initial omission caused a startup crash. Flywheel GPU instance animation and other
shader-driven deformation still need dedicated previous-geometry producers; camera motion alone
does not establish their motion coverage. FSR4 requires supported hardware and remains unverified.

The first live resize exposed a sticky prior GL error misattributed to memory import and a
nontransactional replacement that invalidated published Java handles. Reconfiguration now prepares
the new SR session and all six surfaces before replacing the old set. Pre-existing context errors
are logged separately; retired texture/framebuffer snapshots are not rebound. Nested window-update
callbacks do not present an unfinished resized target. The GPU harness covers 854x480, 1920x1080,
2560x1494 and 1280x720 with DLSS FG configured, plus a forced unsupported allocation that must
preserve every published resource. Live resize/world acceptance of 1.1.3 remains pending.
