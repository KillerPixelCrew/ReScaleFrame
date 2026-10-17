# ReScaleFrameCraft native integration

### Cleanup, 1.1.6-dev

The render path now reuses camera matrices and writes them directly into the retained metadata
buffer. TemporalCamera also builds canonical projections in existing matrices. JNI fills a
caller-owned status array; configuration comparisons use a retained typed snapshot, and F3
text is formatted when requested. TAA/frame-guide draw attachments no longer allocate arrays
per pass. Configuration work is separated from frame progression, and shader-construction
failure/shutdown paths release their owned GL objects. The native extraction root is cached.
Two JNI CPU notifications were removed because the shared FG CPU consumer ignores them:
SIMULATION_BEGIN (2) and FRAME_END (4). Actual simulation, render and Present markers remain.

Twenty-two Java checks pass, including moving-camera metadata equivalence over repeated reuse
and actual staged-DLL status-buffer binding. Release verify passes 44 native checks, eight opt-in
skips and Rust formatting/Clippy. The separately enabled RTX4070 GL test follows the simplified
CPU sequence and passes SR/FG preparation, input/ping/token ownership, pixel and resize checks.
This establishes removed allocation sites and preserved tested behavior, not a measured FPS gain.
Build 1.1.6-dev is deployed to N1GHT; its new startup/world acceptance is recorded separately.

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
preserve every published resource.

Build 1.1.4 corrected a separate frame-lifetime defect: menu presents used frame zero and leaked
Streamline tokens. World evaluation then returned NOT_READY and displayed the raw scaled image.
Every menu present now closes its actual acquired frame; skipped resize frames explicitly abort.
The GPU regression presents 32 menu frames per scenario before real vendor evaluation. Live
world logs show successful DLSS reconstruction after loading and live FG-provider changes. The
human accepted the resulting image. This does not establish displayed generated-frame cadence.

### Input and PCL, 1.1.5

KeyboardHandler.keyPress and MouseHandler.onMove/onPress/onScroll notify the CPU input owner.
After-present callbacks, including Ixeris 4.6.8's queued replay, are retained for the next simulation;
they never reuse the just-presented token. Calls from another thread are also queued. The sequence
is acquire, provider pacing, pending input, SimulationStart, actual tick/controller sampling,
SimulationEnd after render-pre listeners, render submission and Present. A controller sample marker
may occur within simulation when that mod actually polls. Ordinary keyboard/mouse InputSample=6
is deprecated in the installed SDK; keyboard/mouse latency uses the registered PCL ping instead.

The JNI adapter observes PM_REMOVE on the GLFW owner's WH_GETMESSAGE hook, including parent,
child and thread-targeted registered messages. It never replaces GLFW callbacks or injects input.
Only a pending flag crosses threads. The render thread emits PCLatencyPing with the next acquired
simulation token. Input observation remains independent of the chosen SR/FG provider; availability
follows the existing shared SDK host. ControllerInputSample is deduplicated by that host per token.

The installed Controlify jar SHA256 is
`0c8b8d6e57599b516a16f2df2a7314ee96228c94f145f5d7bea2e2ec7b2f90e7`.
REA artifact evidence `ev_0d6ebbe832f69dda4ea91a2f0a18fceb146cd48c72480caeef041c71e0895f37`
identified the package. Matching LTS source and shipped javap bytecode confirm
`tickActiveController(ControllerEntity,boolean)` reads the driver-published state before bindings,
movement and gyro handling. The optional hook runs there; controller input is not inferred from
keyboard emulation. No controller was attached in the inspected live startup.

Online comparison and selected adapters:

| Mod and pinned source | Acquisition / consumption | Adapter |
| --- | --- | --- |
| [Controlify 3.0.1 LTS, fd81677](https://github.com/isXander/Controlify/tree/fd816773a67cc41d4fa242911eae4cc6e36bca12) | SDL3 event/driver or GLFW state on client ticks; active controller consumes published state; per-frame look uses cached tick deltas | Optional tickActiveController hook |
| [Controllable 0.25.4, 95a8a81](https://github.com/MrCrayfish/Controllable/tree/95a8a81597afe8218670607efd2ac7da088083f7) | SDL2/GLFW polls at render/tick boundaries; button snapshots drain only at START_CLIENT; camera turns at START_RENDER | Optional hook before accessible Controller.captureButtonStates; INPUT/BUTTON event declarations are never posted in this revision |
| [MidnightControls 1.10.0.1 beta, e022fd1](https://github.com/TeamMidnightDust/MidnightControls/tree/e022fd1827da0805a0e0a72ccabb3ef4371d04bd) | Tick-time GLFW/bindings plus a separate 1 ms camera Timer writing player angles | Research only; Fabric/Architectury adapter not shipped in this NeoForge mod |

Both controller integrations are optional pseudo mixins. Controlify's shipped method is verified;
Controllable is source-grounded and is not installed in this pack. Neither adapter starts another
controller polling loop. Counters describe callback/sample categories, with queued categories
coalesced per consuming frame; they are not physical event totals or measured latency.

Validation: 20 Java checks pass, including post-present and worker-thread frame association.
Release verify passes 44 native checks plus Rust format/Clippy, with eight opt-in skips. The
separately enabled RTX4070 GL test passes real SDK keyboard/mouse/controller/ping submission,
closed-token rejection, menu-to-world lifetime, cross-thread presentation, resize and pixel checks.
The native observer test proves PM_NOREMOVE does not consume a ping and PM_REMOVE consumes
window/thread pings once. Synthetic hidden-window messages are not a physical controller test.
The standalone latency fixture passes Off/On/Boost when invoked with the absolute staged Streamline
path, vendor `0x10de`, profile `1`: 12 frames, 12 sleeps, 72 lifecycle markers, fresh 64-bit IDs and
Reflex/latency-report availability. Profile `2` passes PCL-only: zero Reflex sleeps and the same
72 lifecycle markers. An initial relative-path invocation refused initialization; the absolute-path
invocation succeeded without a source change. All four required Streamline DLLs are packaged.
Build/run commands are `eng/verify.ps1 -Configuration Release -VS2026`, Gradle `test jar`,
`RSF_RUN_GL_INTEROP_TESTS=1` with `RSF_GL_TEST_RUNTIME=<staged root>` for
`rsf_gl_shared_surface_test.exe`, and `rsf_latency_host_test.exe <absolute staged root>/streamline
0x10de 1` (or `2`). In-game Off/On/Boost use the Low latency selector on the ReScaleFrame page;
they are runtime settings, without a restart. These fixtures do not change live game settings.
The PCL tool must enable its sampler for natural tool pings. No ETW/physical latency or performance
measurement is claimed; fullscreen, missing-runtime and other-GPU cases were not rerun here.

Live first 1.1.5 run: the native report reached source frame 3374 with `mouse=93`,
`pcl_ping=58`, a valid Reflex latency report and advancing simulation/render/present/GPU timings.
Keyboard and controller counters remained zero; Controlify reported no attached controllers.
The game then shut down normally for deployment of the final build, which also accepts
SDK thread-targeted pings. Final jar SHA256:
`d7440db2dc82eef826cce1182d3d107dea1164d3a51cad1ebdd0ee9d0a82eabd`.
Final payload `43fb8ab6d3ca21c21897` completed startup in 109.672 seconds and remained
responsive. Its source frame 1910 reports 167 genuine PCL pings with valid advancing timings;
keyboard and controller input still need manual acceptance.
