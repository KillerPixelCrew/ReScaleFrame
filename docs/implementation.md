# Implementation tracker

AC7 carrier startup passes the admitted Present size directly to the backend
and exposes its install-time settings in the shared panel. A refused native
install stays reachable for retirement and reports unprepared. The session
fixture and capture hook batch policy moved to Rust. Source checks passed.
Native build, fixtures and game validation remain pending.
[A01 scope and checks](research/rust-ac7-a01.md)

G04 moves D3D11 colour, motion, depth and fullscreen passes and six fixture bodies to Rust.
Source comparison covers the public C signatures, exact shader bytes and fixture wiring. The
standalone exposure probe keeps its optional SDK calls in native leaves. Native and GPU
verification remains user-owned. [G04 evidence](research/rust-graphics-passes.md)

The active migration build candidate is `9f53f1b0`. It includes the
Rust Godot SR and final-scene records and the merged Streamline,
FidelityFX, Vulkan and Unity batches. Pinned formatting, locked offline
metadata and the source public header and carrier definition comparison
passed. Binary ABI, fixture, device and game results are pending.
[Candidate checks](rust-migration.md#candidate-9f53f1b0)

The shared D3D11 host now checks SR session identity before graphics work,
preserves input receipts across configure-only requests, and accepts independent
depth inversion and far-plane facts. Rust owns the D3D11 output leaves and the
Runtime's Anomaly backend admission service. The adapter starts its requested
provider through the D05 pipeline entry. Overlay admission and device checks
are pending.
[D3D11 host port evidence](research/rust-d3d11-host-admission.md)

Steam D3D11 uses a static C entry for both Rust adapters and the existing
C++ callers. Rust owns fingerprinting, MinHook installation and the typed
relay, while C++ guards exceptions at the foreign calls. AC7 uses the shared Rust XInput owner.
Pinned binding regeneration and source checks passed. Native linking,
Steam cold creation, Present forwarding and controller observation
remain to be verified. [Steam continuation evidence](research/skyrim-steam-present-continuation-20261116.md)

Godot's DLL exports native control and presentation notifications from Rust. D3D12
publishes its device binding after fence creation and tracks the render and capture slots
by source. Godot and Unity share packaged vendor paths, the legacy FG result convention
and the public sharpening binding. Godot binds a Runtime CPU service and reports its
actual Vulkan submission and Present receipt. The export source coverage, pinned engine
patch generation, binding regeneration and pinned formatting were checked. Build, fixture and device checks remain open.
[Godot native control evidence](../integrations/godot/NATIVE.md#native-control-and-d3d12-follow-up)

RDR2's ASI startup leaves, research-attach worker and native settings probe
now run in Rust. Settings fixtures share the measured vectors with the
Rust policy, and refused startup sessions retain their cleanup owner.
DXGI and the ASI diagnostics adapters now also run in Rust. The CRT entry
binds their existing C services. Restart overlay controls remain open.
Build, fixture and device checks are pending.
[RDR2 startup source evidence](research/rust-rdr2-asi.md#r01-follow-up)

The Anomaly SR stage calls the Rust swap owner directly at the seam.
Guard admission, constant rollback outcomes, spatial recovery reporting,
GPU query retirement and the seven SR overlay controls have been
source-reviewed. The D3D11 startup dependency on DLSS still blocks
Runtime-owned backend selection for this adapter. Builds, fixtures and
game acceptance are pending.
[Anomaly SR checkpoint](research/rust-anomaly-sr-stage.md#n06-source-review-and-port-checkpoint)

Godot Vulkan uses device-keyed capability receipts and one Runtime SR session.
Each FG pass reports its own outcome. The V03 presenter owns retired guide resizing,
and the engine retains its exact submission and Present receipts. Source ABI
comparison and pinned formatting passed. Build, fixture and device checks remain
open. [Godot Vulkan E03 evidence](research/rust-godot-vulkan-e03.md)

The Unity package now routes render callbacks, admits preview and
generation windows, and handles device events in Rust. The managed
exports matched in source, and pinned binding regeneration matched the
private callback table. Build, Editor and player checks remain open.
[Unity migration](rust-migration.md#remaining-integration-ports)

Unity's D3D11, D3D12 and Vulkan bridges now share the Rust scalar pass
builder. D3D12 retains view submission receipts through Unity's queue
flush and acknowledges the reconfiguration generation after delivery.
Separate Runtime rings track D3D11 and Vulkan interop submissions.
D3D11 shared-surface replacement requires both fence timelines and
rejects incompatible copy inputs. Source checks, pinned formatting and
targeted binding regeneration passed. Build, fixture and device results
remain open. [Unity bridge evidence](research/rust-unity-bridge-submission.md)

The shared Vulkan-to-D3D12 FG paths now admit swapchain extensions and
select vsync in Rust. The FSR4 state fixture uses the current graphics
callback table. Build, fixture and device checks remain open.
[Swapchain admission evidence](research/rust-vulkan-dx12-copy-plan.md#swapchain-admission-facts)

The Godot SCons module now owns host boot, configuration, shutdown and
viewport extent policy in Rust. Source checks matched the private
callbacks and public declarations. The SCons build, editor and player
checks remain open. [Godot evidence](../integrations/godot/NATIVE.md#engine-module-host-lifecycle)

D3D12 FidelityFX frame generation now plans context flags, extents,
provider admission, per-frame camera constants, SDK entry admission,
configure records and the present copy in Rust. Native code retains
typed descriptors and SDK calls. Source review matched startup,
prepare and dispatch ordering. Builds and device checks remain open.
[Migration evidence](rust-migration.md#fast-path-to-a-verified-candidate)

The D3D11 Streamline signature check is restored for the shared DLL and
linked consumers. Owned D3D11 shutdown now retains the session and closes
SDK admission on failure. Source checks cover both feature branches.
Compilation, linking and SDK shutdown remain unverified.
[ABI and retirement checks](rust-migration.md#fast-path-to-a-verified-candidate)

Native D3D12 DLSS retains its owned Streamline registration and
dependencies if SDK shutdown fails, including during startup rollback.
Source review confirmed the public destroy signature and borrowed-owner
route. Formatting and locked metadata passed. Build and shutdown behavior
remain unverified. [Retirement and verification](rust-migration.md#fast-path-to-a-verified-candidate)

Native D3D12 DLSS now shares Rust tag assembly and quality mapping
with the D3D11 route. Native D3D12 keeps colour, depth, motion and
output in order, with optional exposure covering the entire resource.
Rust selects HDR and auto-exposure options. The typed SDK calls and
public declarations match in source. Current-candidate builds and
runtime checks remain open.
[Current verification priorities](rust-migration.md#fast-path-to-a-verified-candidate)

Native D3D12 DLSS now owns startup admission and the frame constants
plan in Rust. Source checks matched the owned and borrowed feature
routes and the public declarations. The native inverse still uses
DirectXMath with the same determinant threshold. Builds and runtime
checks remain open.

The shared overlay input owner now routes window messages in Rust.
Native leaves read Windows contact and mouse facts and forward the
message. Source comparison matched key and button release pairing,
touch delivery and handle ownership, raw-input cleanup, and the
public declarations. Live input and linkage remain unverified.
[Window message routing](research/rust-overlay-host.md#window-message-routing)

Auxiliary SR now admits D3D11 texture shapes and packed-colour
capabilities in Rust. Typed descriptor and capability queries remain
native. The public host ABI is unchanged in source, and the existing
device fixture remains unrun. [Source comparison](rust-migration.md#native-policy-cutover)

The fullscreen pass now admits the immediate context and views and
calculates the render-target mip extent in Rust. The present blit uses
the same owner. D3D11 facts, shader code and recording calls keep their
native boundaries. The fullscreen device fixture remains unrun.
[Source comparison](rust-migration.md#native-policy-cutover)

Scene promotion now builds its replacement texture and views in Rust.
It accepts a view hint only within the source's format family and
releases partial allocations on refusal. The typed D3D11 descriptors
and public C calls are unchanged in source. The device fixture remains
unrun on this port. [Source comparison](rust-migration.md#native-policy-cutover)

The fixture-only negotiation policy has been retired. Production SR and FG
selection stays with the orchestrator. The C backend declarations remain
available to callers through the header-only Contract target.

Plugin sessions use the generated GameSDK option record and the shared
Windows path admission helper also used by the FidelityFX and Vulkan timing
callers. The Rust fixtures cover plugin ABI, DLL and embedded session
lifetimes, and Runtime export ownership. Native C++ still supplies the
throwing plugin and the exception guard. The C fixture continues to exercise
the SDK's inline frame eligibility rules. Pinned formatting, locked offline
Windows Cargo metadata and source comparisons passed. Builds, fixture
execution and device checks remain open.

## Remaining Rust integration ports

Fallout 4 now selects the viewport, dynamic-resolution and depth
prepass calls in Rust. Rust owns the HUD-less admission, overlap
refusal, and same-source checks. The native detours retain the
original calls, LastError, unwind guard, and typed effect calls.
Unused resolution and depth wrappers were removed. The producer
fixtures and game checks remain unrun.
[Producer ownership](research/rust-fallout4-render-hooks.md#viewport-dynamic-resolution-and-prepass-seams-in-rust)

Fallout 4 now selects the imagespace and Effects seams in Rust
and owns their recovery and cleanup sequence. Rust also applies the
existing TAA gate and bounded receipt. Native detours retain the
original calls, return addresses, unwind guard and LastError handling.
The new fixtures and game checks remain unrun.
[Imagespace and TAA ownership](research/rust-fallout4-render-hooks.md#imagespace-effects-and-taa-seams-in-rust)

Fallout 4 now admits and closes FRAME, SUBMISSION and WINDOW scopes
in Rust, including the balanced post-Swap close and recovery retry.
The native COM lease probe still catches exceptions at the C++
boundary. The scope fixtures and game checks remain unrun.
[Fallout 4 frame scopes](research/rust-fallout4-render-hooks.md#frame-submission-and-window-scope-in-rust)

Fallout 4 now admits and publishes its SR inputs and HUD-less scene
in Rust. Source comparison matched the FRAME and SUBMISSION owner
checks, SR-off path, and FINAL_SCENE and WINDOW order. The typed
engine reads, COM identity and Runtime calls remain in the native
leaves. A null framebuffer texture now refuses the extent check.
The new fixtures and game checks remain unrun.
[Fallout 4 publication](research/rust-fallout4-render-hooks.md#sr-and-hudless-publication-in-rust)

Fallout 4's Rust recovery transaction now owns the reservation token,
five-surface plan and checked SEAL, FINISH and RETIRE requests. The
adapter retains the original raster reads, COM identity checks and
held leases. After imagespace clears its frame flag, cleanup after
Swap can retry RETIRE using the token's successful FINISH proof.
The fake-leaf regression and in-game retry remain unrun.
[Recovery retirement](research/rust-fallout4-render-hooks.md#retirement-after-the-imagespace-exit)

Godot's private Rust object now tracks the primary viewport and target,
resource generation and the SR inputs that drive reset and admission.
The engine still supplies typed RID and render-buffer facts. AC7's
Rust F9 owner now verifies and registers the six root observers and
retires their registration. Guarded reads, MinHook calls and engine
trampolines stay native. [Godot engine module](../integrations/godot/NATIVE.md) ·
[AC7 F9 roots](research/rust-ac7-motion-root-admission.md)

RDR2's pending and ready readbacks and retained final draw live in Rust.
The native submission and Map leaves keep the exact list, generation,
queue, fence and value receipt. The pending owner caps simultaneous
shots at the existing 84-copy budget. Interleaved submissions,
descriptor-copy and object-retirement coverage and live capture
remain open. [Shot ownership](research/rust-rdr2-asi.md#native-capture-shot-and-tail-ownership)

Fallout 4's render callbacks sequence World and Present admission, producer
reads, SR publication, and reduced recovery in Rust. Typed detours call
their originals on native frames and own COM leases. Effects cleanup uses
the admitted World's identity across the original call, and the HUDless
lease stays on the viewport detour's stack. F03 has source and binding
checks. Build, fixtures and matched game captures remain open.
[Render hook transactions](research/rust-fallout4-render-hooks.md#render-transaction-contracts)

Unity's Editor preview validates its parent and child before admitting
rendering. Runtime owns the per-output D3D12 copy, swapchain and fence.
Its fixture injects a signal refusal after execution and checks that
retirement preserves the recording. Build, fixture and Editor checks
remain open. [Editor preview](research/rust-unity-preview-resources.md#runtime-presenter-and-editor-window-replacement)

The Unity Vulkan resolver publishes the interceptor table, selects
device extensions and resolves the swapchain destroy fallback. The
saved function slots remain process-wide, and retirement depends on the
destroy hook. [Vulkan resolver](research/rust-unity-vulkan-resolver.md)

AC7's Rust F9 capture owner now writes the cloud and contact-light
before-draw rows with the existing query and receipt order. Source
comparison covered the native reads and 60 archived rows. Fresh
capture and device verification remain open.
[Before-draw records](research/rust-ac7-motion-before-draw.md)

Pinned formatting, locked offline Windows Cargo metadata and Git
whitespace checks passed for these four ports. Builds, binary ABI
checks, fixtures and game or engine checks remain user-owned.

Godot now owns the CPU frame lifecycle, render handoff and render frame
identity in its private Rust SCons object. Typed Godot callbacks still
supply the clock, pause and focus facts and report events at the same
engine boundaries. The AC7 velocity pass now owns renderer publication,
replacement and release in Rust. D3D11 objects and shader calls remain
in the native leaves. The renderer borrow relies on the render-thread
ordering and frame-tap drain used by the carrier.
Pinned format, locked Windows metadata and whitespace checks passed.
Builds, fixtures and game or engine validation remain open.
[Godot native migration](../integrations/godot/NATIVE.md) ·
[AC7 renderer lifetime](research/rust-ac7-velocity-pass-cache.md#renderer-lifetime)

The source integration at `17904281` brings the following owners into
the migration branch. Pinned Rust 1.94.1 formatting, locked offline
Windows Cargo metadata, and Git whitespace checks passed. Builds, binary
ABI checks and fixture, device and game runs for these ports remain open.

- Fallout 4's Rust render-input owner applies the 21 guards and owns
  resource admission, camera history and pass construction. Rust also owns
  World and Present admission. Typed native calls keep their existing
  trampoline frames and CPU Present receipt. Reduced-render recovery and
  the remaining hook lifecycle are native.
  [Render inputs](research/rust-fallout4-render-inputs.md) ·
  [Render hooks](research/rust-fallout4-render-hooks.md)
- The Fallout 4 F02 follow-up shares render-context reads and camera jitter math, rechecks
  the native records before handing out leases, stores the validated depth maps and allows
  advanced-frame resolution cleanup at the ordinary World boundary. The camera fixtures
  moved to Rust and the native padding fixture calls the production comparator. The changes
  are source-checked and await build, fixtures and game or device validation.
  [F02 input ownership and resolution retirement](research/rust-fallout4-f02.md)
- AC7's Rust F9 path admits geometry reads and writes the existing JSONL
  schema. The velocity owner admits commit targets and applies the
  existing profile and log limits. D3D11 readback, blob deduplication,
  velocity merge, and cloud and contact-light before-draw rows remain native.
  [Geometry](research/rust-ac7-motion-geometry.md) ·
  [Velocity commit](research/rust-ac7-velocity-commit.md)
- Unity's Rust D3D11 pass owner selects the route, plans input surfaces and
  copy-back, and decides retirement from completion and hand-back facts.
  The typed graphics calls and managed ABI remain in place.
  [D3D11 pass plan](research/rust-unity-d3d11-pass-plan.md)
- RDR2's Rust capture owner selects ready readbacks using the exact
  submitted fence and value and sequences manifest publication. The
  poller retains its 100 ms interval. Descriptor-copy and object-retirement
  observations remain unverified.
  [Capture drain](research/rust-rdr2-asi.md#native-capture-readback-drain)
- Godot's private Rust SCons object assembles CPU event records. The engine
  callbacks still supply the timestamps, frame identity and input kinds.
  [Godot native migration](../integrations/godot/NATIVE.md)
- Anomaly's Rust reflection owner publishes a complete D3D11 snapshot and
  refuses unreadable rows. Manifest admission, draw classification, and
  reports share stable shader-family IDs. The offline tree compiler uses the
  shared Rust compiler, and the guarded engine include lookup remains tied
  to the Standard and AVX profiles.
  [Shader loader port](research/rust-anomaly-shader-loader-n05.md)

The [integration migration scope](rust-migration.md#remaining-integration-ports)
covers the existing game and engine adapters beyond Anomaly. Project Wingman
now publishes its GameSDK table and exact build identity from Rust. The
existing DLL export forwards to that entry, and native pre-hook verification
uses the same fingerprint.
Source checks cover the entry path; the plugin and carrier
contracts remain unrun.

Wingman's Rust camera decoder uses the same guarded field offsets and
reprojection order. It shares matrix inversion with RDR2 and refuses
inverted or overflowing view rectangles. The engine hooks and native
fixture use the same guarded copy. The expanded camera fixture remains
unrun.

Wingman's Rust graph observer validates the same AddPasses site, records
borrowed graph facts and rate-limits diagnostics. The native detour still
calls the original first and keeps its pinned trampoline. Stop retains the
host services while a callback remains. This port pins the observer DLL
before installing the hook. The plugin and live observer remain unrun.

Wingman's Rust execution owner tracks queued graph packets, nested TAA
metadata, hook lifetime and RHI callback retirement. The native leaves
still call the original passes in order and acquire and release the D3D11
leases. Reservation respects the original 256-command cap across threads,
and retirement keeps the borrowed services until calls and commands finish.
Build, fixture and live execution checks remain open.

Wingman's dedicated XInput carrier starts its optional plugin session in
Rust. It retains the exact executable gate and `ObserveNative` setting.
Both XInput carriers use the same native System32 forwarding code with
the original ordinals. The carrier and game contracts remain unrun.

The Steam D3D11 chain uses one Rust owner for the exact overlay profile,
guarded forwarding and Present hook lifetime. Fallout 4 and Skyrim SE
keep their existing native entry points. MinHook, C++ exception guards and
the shared file hash remain native. Source checks cover both call paths
and the private table. The game plugin links, Steam-present path and
device verification remain unrun on this revision.

Unity Mono's Rust adapter selects the researched build, owns the plugin
lifecycle and starts and stops the managed helper on attached Mono threads.
The existing game DLL export and native render bridge are used by the same
callers. The Unity package still builds the shared bridge. Build, fixture
and Unity player checks remain unrun on this revision.

Both Unity native DLLs link the same Rust D3D12 frame owner. It handles
packet, policy generation, command-ring and submission lifetimes. A
device-removed fence value cannot admit a frame or retire in-flight slots.
The existing Unity graphics calls and CPU fact callbacks remain native.
The generated private binding was checked. Build, export and player
verification remain open.

Unity Mono starts the already-loaded Runtime through Rust using the
existing `[UnitySR]` settings. Mono and package callers share path
resolution. The four Unity exports still use the typed native facade.
The private setup tables were regenerated against their headers. Builds,
fixtures and Unity player verification remain open.

The Unity package's Rust owner handles settings, render plans, status
and session retirement. A restart reapplies the retained FSR1 selection
to the engine's spatial path. The native package keeps the existing
exports and Unity graphics calls. Build and player verification remain
open.

The RDR2 game DLL uses Rust for exact-build admission, SR input routing,
camera decode, scene facts and its lifecycle. The engine entry export and
typed native leaves remain available. Stop returns `BUSY` on refused hook
retirement and retains its services until a later stop succeeds. The RDR2
plugin and camera builds and game verification remain unrun on this revision.

RDR2's ASI now admits the exact executable and sequences its configuration,
ForceD3D12 and engine settings hooks in Rust. The ASI loader entry retains
its existing path. The native
settings fixture now calls the Rust policy through its existing facade.
Source checks cover ABI and hook fields. Builds and ASI verification remain
open.

The RDR2 ASI worker and explicit attach helper now share one Rust Runtime
setup and plugin-session caller. The ASI loads its staged Runtime. The
attach helper only borrows an existing Runtime and still reads `attach.ini`.
The Unity SR fixture uses the same RDR2 setup through its native facade.
Path bounds, ABI and link wiring were checked in source. Builds,
fixture execution and game validation remain open.

RDR2's optional ScriptHook input guard runs in Rust on the same game
fiber. It polls Runtime settings, detects F7 and the two-stick chord,
and suppresses gameplay actions while the shared overlay is visible.
The binding table is complete before registration can call the fiber.
Build and live input verification remain open.

RDR2's read-only settings probe uses Rust for two exact-byte guards and
bounded JSONL capture. It still requires the opt-in setting and an arm
call, and the current ScriptHook path has no arm call. On partial hook
enable, it can retain the trampolines for any
callback already in flight. The typed detours preserve the original
call order. Source checks cover the bytes, keys and bounds. Builds and
live probe verification remain open.

RDR2's Rust camera probe owns the 64-frame capture window, request-file
polling, worker-local snapshot and ScriptHook reference records. The native
detours retain their original call order and the binary record remains
2568 bytes. A missing or non-finite ScriptHook result skips its reference
record. The two read-only probes share the hook transaction. Build and
live capture verification remain open.

RDR2's Rust Social Club owner checks the recorded renderer build and
publishes the existing backbuffer relay. It preserves RAGE's supplied
index on an external recording list and leaves the original comparison
in the other path. If enable is refused after registration, the relay
stays valid for any caller already inside it. Source comparison checked
the guard and relay bytes. Build and live verification remain open.

Fallout 4 now owns exact-build identity, GameSDK lifecycle, Runtime
session setup and cold D3D11 creation in Rust. The creation owner
checks the final outer swapchain slot before binding presentation, and
keeps the existing Steam facade and typed native call. The rendering and
CPU producers still need ports. Link, fixture and game checks remain open.

The Minecraft native adapter uses Rust for session startup, the surface
and message observer, SR and HUD-less packets, and configuration and status
translation. Existing JNI exports still marshal Java arguments and call the
same linked Runtime. The log directory is opened through the Windows wide
path API. Builds, the native and Gradle fixtures, and gameplay remain open.

Skyrim SE's Rust creation owner guards the original game thunk and the
verified Display Tweaks route. It preserves the existing Steam facade,
scoped Runtime calls and native creation outputs. The GameSDK lifecycle
and rendering producers still need ports. Build and both-route gameplay
verification remain open.

AC7's GameSDK table, exact-build identity and lifecycle now use Rust.
The public game entry and the native renderer and combined carriers keep
their interfaces. The lifecycle retains a renderer returned on a refused
prepare for later retirement. The private binding was regenerated. Build
and game verification remain open.

AC7's Rust renderer policy owns the exact-build site table, hook counters
and scene precision and velocity patches. The native renderer lends the
guarded reads and protected writes. Source comparison matched 65 hook
entries, 26 helpers and 11 depth and view gates. The public renderer ABI
and linked carrier route were checked in source. Builds and gameplay
remain open.

Godot's native control and settings owner is Rust. It preserves the
GDExtension entry points and linked Runtime, and refuses overlapping or
reentrant D3D12 boot attempts. Build and Godot player verification remain
open.

Godot's Rust Vulkan session owns boot, requested settings, status,
provider admission, and SR and FG accounting. Vulkan calls remain in the
typed native leaves. Shutdown rechecks the live session under the guard
so an overlapping caller cannot release it twice. Build, device and
Godot player verification remain open.

The Godot Vulkan SR context owner now handles creation, retirement,
evaluation, fallback and FSR4 continuation in Rust. It keeps the
provider calls and exact command buffer handoff in typed native leaves.
The existing FSR3 Auto behavior and previous-device retirement order are
preserved. Device replacement, FSR4 and DLSS still need live verification.

Godot's window admission, subclass forwarding and input-session
ownership moved to Rust. A refused restore retains the previous
window procedure so subsequent messages can still forward. The
engine's input facts and Runtime pacing calls keep their existing
contracts. Shutdown with an intervening subclass still needs live verification.

Godot's Vulkan resolver and device creation policy are Rust-owned.
The existing path still tries XeSS, Win32 interop and optional Runtime
latency requirements before native creation. Readiness and extension
merging use the same session owner. Native leaves retain the Vulkan
types and process SDK requests. Build and device verification remain open.

Godot's D3D12 SR context, recording slots and submission ledger are Rust-owned.
Pending recordings keep their slot and context until the engine reports a
submission and the queue accepts a fence. A later submission on that queue
can recover a failed signal. The public C header and engine module are
unchanged. Builds, fixtures and player verification remain open.

RDR2's native graphics capture uses Rust for requests, draw and input
limits, timeout, drain admission and bundle status. It uses the existing
native mutex to order those decisions with the retained D3D12 resources.
An interrupted NGX exit releases its input reservation. Rust tracks
resource transitions and snapshots the submitted command-list generation
before the queue call. The native code retains only readbacks matching
that snapshot. Rust detects the diagnostic F10 edge, consumes the request
file, and writes the camera records. It writes each mapped pixel file
before its JSON sidecar and publishes the bundle manifest after its
temporary file is complete. The source comparison matched the existing
artifact names and JSON fields. GPU copies, fence completion and mapping
remain native.
Builds and the two native capture fixtures remain unrun.

The RDR2 readback ledger claims the exact list generation before submit.
If the post-submit fence fails, a later signal on the same queue can
complete the original readback with its original submission identity.
Rust also checks fence completion before recycling the tail across
recordings. Rust now owns the readback buffers and completion fences.
It retains the exact submitted queue across refused signals, and drops
idle queue owners outside the capture mutex once no readbacks are
pending. Native code executes the GPU calls. Missing submissions
still keep the readback pending. Source and layout checks passed.

The RDR2 capture owner now leases the admitted swapchain and its full
backbuffer batch in Rust. A failed buffer lookup drops the partial
batch and leaves capture unarmed. Draw selection commits after the
copy is recorded, and Present closes the draw phase only for the same
swapchain and selected buffer. Idle leases drop outside the capture
mutex. The native list-creation hook now resets the generation for
the actual returned list. Builds and GPU fixtures remain unrun.

Rust now owns the RDR2 capture probe's 16 hook registrations and
trampolines. A refused enable keeps the created trampoline and retries
only when the same vtable target returns. The native detours still call
their originals with the same typed arguments. Source comparison
matched every slot and call. The refusal regression remains unrun.

The RDR2 capture owner now stores the observed command-list, RTV and
pipeline facts and bounded draw records in Rust. Native callbacks still
lend the same D3D12 descriptors, caller and stack. An unsupported
successful pipeline result discards stale metadata at that address.
Descriptor-copy and object-retirement coverage, build and live capture
remain open.

RDR2's capture also owns presentation and NGX evaluation metadata
in Rust. Native reads supply the fresh Present descriptor, timing
and camera facts. Evaluation output is admitted for the same wrapper
and recording generation. Source comparison matched the artifact
fields and all 18 scalar keys. Build and live verification remain open.

RDR2's capture installer now prepares paths and owns its worker startup
in Rust. A refused thread start releases the prepared owners before
census registration, so a later install can retry. Game and fixture
entry points keep their existing contract. The fixture still needs
native verification.
[Capture installation audit](research/rust-rdr2-asi.md#native-capture-installation)

RDR2's Rust diagnostic workers preserve the module sampling marks,
dump policy, census summary and optional RenderDoc request loop. Native
leaves retain the existing diagnostics ABI. The RenderDoc trigger still
requests a camera record before queuing the frame capture. Rust also
sequences installation of the opt-in D3D12 HRESULT trace and appends
the failure records. The native detour captures the caller and stack
on its original frame and forwards the original HRESULT. Builds and
live diagnostic verification remain open.

Skyrim SE's Rust adapter publishes the GameSDK table and owns executable
and observer admission. It reuses the cold-creation platform for SHA-256
and guarded byte reads, and publishes the hook targets after all checks.
Rust also owns the five observer hooks' install, start and disable
transactions. A failed start retains any hook whose disable was refused
so stop can retry. The native detours still consume
the same targets. The session lifecycle now owns Runtime setup, reuse
and teardown in Rust as well. Quiesce withdraws observer admission again
under the session lock to cover a concurrent start. Build, binary ABI
and game verification remain open.

Skyrim SE's SR and presentation paths now convert the copied native
camera cache in Rust. The conversion retains the forward-depth check,
camera-relative origin correction, pixel jitter and identity fallback
for invalid history. A refused camera clears the SDK record and history
flag. The existing camera fixture links the same Rust archive as the
adapter. It remains unrun.

The pre-HDR SR and FG producers now share one Rust camera-cache reader.
Each producer keeps its own previous camera, native frame, extent and
callback timestamp. The reader retains the native TAA and unique-entry
checks, the render-to-output ratios and the bitwise origin match used
for history. D3D11 leases and pass delivery still use the existing
native paths. Source comparison passed. Live timing remains unverified.

Skyrim SE's sampler owner now checks the native flush entry and owns
scene admission, mip bias, slot selection, and pending restoration in
Rust. The typed detour still forwards the native flush first. The owner
uses guarded reads of the same cached sampler tables and calls the
existing Runtime sampler service on the renderer thread. Stop checks
for entries again after disabling the hook. Live restoration and
shutdown remain unverified.

Skyrim SE's interface observer, final-scene handoff and window pass
retirement are now owned by Rust. The owner copies the existing scene
fields, matches the native frame, swapchain and CPU scope, and carries
later screen refusals through to the window pass. The window keeps its
own context and swapchain references until the matching end callback.
Rust defers an end that reenters during begin. Pre-HDR texture discovery
and submission still use the native producer. Source checks passed.
Live callback and device validation remain open.

Skyrim SE's auxiliary reconstruction and diagnostic selection are now
Rust-owned. The four surfaces keep their original formats, rectangles,
jitter and restoration flags. A refused restore retains the exact
session, frame, view and renderer-thread receipt. The owner leaves
output resolution only if that scope entered it. Native leaves still
make the COM and Runtime calls. Source and layout checks passed.
Build and live auxiliary restoration remain unverified.

Skyrim SE's resolution owner now retains the native ratio and lock
snapshot, produces the same Halton jitter, and controls the temporary
output viewport in Rust. Native leaves keep the constant upload before
the viewport call. If output restoration must retry, the auxiliary
owner retains its receipt and skips the Runtime restore it already
completed. Source checks matched the engine fields and call sites.
Build and in-game resize and fallback verification remain open.

Skyrim SE's pre-HDR SR and FG producers now share one Rust reader for
texture discovery, leases, D3D11 admission and pass construction. SR
keeps its optional rejection mask and both colour formats. FG checks
its source, submission and scope again before the synchronous handoff.
Native leaves retain the exact typed D3D11 and Runtime calls. Source
comparison passed. Build and live producer verification remain open.

Rust now verifies Skyrim's Community Shaders 1.9.1 fingerprint, PE
section bounds and six guarded entry sites. It also checks the cold
device and feature state and carries the exact HDR caller and arguments
through nested callbacks. Rust also owns resolver admission, original
initializer capture, plugin path checks and the handover transaction.
Failed activation disables the hooks and retains their pinned
trampolines. The native code keeps the file mapping and typed calls.
Build and live coexistence remain open.

Skyrim's CPU source reservations, input classification and scoped
render admission are Rust-owned. The adapter still reports the same
native tick, poll, simulation and World callback facts through the
GameSDK. Native job-generation proof and hook teardown remain in
C++. The expanded input fixture remains unrun.
[Skyrim CPU ownership audit](research/rust-skyrim-cpu-ownership.md)

Skyrim's pre-HDR observer now admits the exact post-process call,
publishes FG inputs before SR, and owns the temporary TAA override
in Rust. It clears the main history bit only after reconstruction
succeeds and restores the enable byte when the same owner survives
the native call. The trampoline keeps its original five arguments.
Source and layout checks passed. Build and live fallback remain open.

Skyrim's renderer-end, upload and jitter observations now sequence
their work in Rust. Presentation binding and sampler restoration
precede the native end call, and window and auxiliary retirement
follow it with the retained session. The upload path verifies the
native dirty byte before the original call and records its flags
afterward. The adapter keeps the exact window-class rule. Source
and layout checks passed. Build and live observer behavior remain open.

Skyrim's 13 CPU hooks now use Rust admission, registration, rollback
and service retirement. Typed originals stay on native frames, and
entered detours drain before services are released. Source checks
matched all targets and 247 guard bytes. Build and live CPU validation
remain open.
[CPU ownership audit](research/rust-skyrim-cpu-ownership.md)

Skyrim's native job ticket, poll epochs and consumer proof now live in
Rust. The typed submit, join and PollDevices detours retain their exact
native observations and GameSDK ordering. Hook effects and guarded engine
reads remain native. Build and live CPU validation remain open.
[CPU ownership audit](research/rust-skyrim-cpu-ownership.md#native-job-and-poll-proof-continuation)

Fallout 4's camera and raster math now run in Rust. The port follows the
previous DirectXMath SSE2 operation order and preserves its determinant
and refusal rules. The fixture uses the same adapter archive. Bit-level
parity and builds remain unverified.
[Camera source comparison](research/rust-fallout4-camera.md)

Fallout 4's CPU event owner now tracks poll and worker sources, scoped
GameSDK facts and hook retirement in Rust. The typed C++ shells keep
the original engine calls on their native frames and pass stack receipts
to Rust before and after them. The private binding was regenerated.
The previous handoff fixture moved to Rust. Build, event parity and
game teardown remain open.

Fallout 4's screen owner now verifies the menu entry points and
classifies before-and-after screen and world-lease facts in Rust.
The C++ observer runs the menu, freeze and camera queries on its
native frame and preserves last error. The source comparison matched
all six menu profiles and guarded engine addresses. Build and live
screen transitions remain open.
[Input and screen evidence](research/fallout4-input-ownership-20261116.md)

Fallout 4's depth-domain owner now checks the 35 guarded sites, samples
the native raster and camera, and composes both depth domains in Rust.
Typed D3D11 leaves supply the resource and lease facts. The port keeps
the existing refusal stages and forward-depth convention. Build and
native capture verification remain open.
[Depth-domain ownership audit](research/rust-fallout4-depth-domains.md)

Fallout 4's producer resolution, recovery reservation and restoration
receipts now live in Rust. The existing native callbacks still supply
the renderer and viewport facts. Each temporary field is restored only
while the same scope owns its bytes. Source comparison matched the 11
guards and eight receipt stages. Moving and all-quality recovery,
downstream output and build verification remain open.
[Resolution ownership audit](research/rust-fallout4-resolution.md)

Unity package D3D12 SR contexts and their recording slots are Rust-owned.
The existing Unity bridge still provides recording and completion facts.
The view owner preserves reconfiguration, spatial fallback and retirement
through the shared Runtime. The native setup table was regenerated, and
build and player verification remain open.

Unity's package presentation identity, FG installation and provider
requests now use the Rust owner. Both Unity entry routes share the FG
setup builder and resolve Auto through the Runtime. The private package
table was regenerated. Build and player verification remain open.

Unity's package session services, log lifetime and input routing are
Rust-owned. A refused window subclass leaves the next attempt eligible.
The window callback snapshots its forwarding procedure before reporting
input. The package entry points still use the same timestamps, frame
facts and Runtime services. Builds and player verification remain open.

Unity's D3D11 bridge shares Rust packet admission and owns submission
slots and fence values in Rust. A failed hand-back still records and
requests a completion signal before the adapter stops. The shared packet checks now apply
to both D3D11 and Vulkan producers. Native builds and player verification
remain open.

Unity's Vulkan bridge now owns event, window-frame and shared-device
admission, queue slots and retirement in Rust. A present interception
holds admission through forwarding. Swapchain setup destroys partial
semaphores and the new chain if setup fails. The native queue drain
still needs live synchronization verification. Builds and player
checks remain open.

Unity's Vulkan swapchain records and presentation lease are
Rust-owned. The lease serializes native Present, resize and
semaphore retirement. Registration allocates fallibly, so native
rollback still owns partial setup if allocation refuses. The Vulkan
queue drain, native builds and player verification remain open.

Unity's Editor preview window selection, graphics admission and
submission ring now live in Rust. Fence failure leaves device
setup retryable, and the backbuffer is acquired before resetting
a command list. A refused post-submit signal stops slot reuse.
Win32, DXGI, D3D12 and source leases remain in typed native calls.
Build and Editor playback verification remain open.

Unity Vulkan render events and window frames now carry their own copied
host services. A held probe covers native fence reads during drain
checks. Shared-device creation publishes its instance only with the
associated objects, and release clears it. Native queue synchronization,
borrowed device use and live teardown still need verification.

Unity Vulkan resolver slots and Win32 extension admission now live in
Rust. The resolver preserves native passthrough when either required
extension is unavailable and records interop readiness for the device
that was actually created. Native leaves still resolve typed functions
and create the device. Build and runtime verification remain open.

Unity Vulkan image plans and first-use layout publication are
Rust-owned. The queue callback and Present share a lease so they
cannot replace the same native image or reuse a slot concurrently.
Lifecycle release checks the bridge's Vulkan and D3D12 fences without
calling `vkQueueWaitIdle`. An accepted upload without a hand-back fence
keeps the bridge retained. Swapchain destruction and render-thread
resize still use native queue waits. Live device and teardown
verification remain open.

Unity Vulkan's Rust image owner now retains the D3D12 resource, Vulkan
image and imported memory together. Native queues borrow a view under
the presentation lease. Resize installs a complete replacement before
destroying the old handles, and teardown removes the Rust records
before releasing the device. The device and replay checks remain open.

Unity Vulkan's command pool and three recording slots now share the
Rust interop ring and presentation lease. The owner publishes a complete
set, rolls back partial creation and retires the pool after the slot and
shared fences prove completion. Present checks the DXGI buffer before
consuming Unity's waits. Native leaves still issue the graphics calls.
Build, linkage and player verification remain open.
[Command resource audit](research/rust-unity-vulkan-command-resources.md)

Unity Vulkan now publishes its shared D3D12 device, queue and fence,
imported semaphore and command slots as one Rust-owned record. Both
submission paths borrow it under the existing admission, and package
callers hold COM references through adapter lookup and Editor preview
setup. Retirement refuses the D3D12 device-removal sentinel and keeps
bridge bookkeeping on a refused release. Build, queue synchronization
and player verification remain open.
[Shared device audit](research/rust-unity-vulkan-shared-device.md)

Unity Vulkan's DXGI swapchain and resize transaction now use the Rust
presentation lease. A failed drain or resize retains the chain, and
checked release drops it before the shared device. Package shutdown
now holds bridge admission and submitted-fence proof through Runtime
session retirement, SR view destruction and bridge release. Refusal
keeps the live session retryable across shutdown and restart. A physical
device shutdown or DLL unload initiated by Unity still requires the
host's lifetime guarantees. Build and live teardown remain open.
[DXGI ownership audit](research/rust-unity-vulkan-dxgi.md)

Unity's D3D11 bridge now publishes its shared graphics interfaces,
fences and command slots as a complete Rust-owned record. Failed
initialization rolls back the partial objects. The existing retirement
proof covers both completion fences. The six-surface table and typed
graphics calls stay native. Build and live verification remain open.
[D3D11 ownership audit](research/rust-unity-d3d11-shared-device.md)

Unity's six D3D11 shared surfaces now belong to the Rust graphics
record. Replacement checks the fence and the current frame lease.
Refused retirement retains the surface and its device. Native callbacks
still lend the queued packet textures. Build and live resize verification
remain open.
[Surface ownership audit](research/rust-unity-d3d11-surfaces.md)

Unity's D3D11 packet store now owns queued texture references through
the matching render event. A consuming slot remains reserved until its
lease returns, and retirement releases cancelled entries after resetting
the store. Vulkan and Mono still use the existing unleased packet calls.
Build and Unity callback verification remain open.
[Packet lease audit](research/rust-unity-d3d11-packet-leases.md)

AC7's Rust renderer transaction owns prepare, start, quiesce and stop.
Failed activation keeps any renderer with reachable callbacks, and a
refused hook disable or removal keeps the renderer available for retry.
The native detours and TrueSky operations still use the same interface.
The binding was regenerated. Builds and live retirement remain open.

The AC7 TrueSky patch owner now manages the verified sites, relay
installation, rollback and retirement in Rust. Restore holds the cloud
resolution lock and retains relay slots and D3D11 resources while a
guarded handler is active. The renderer and shaders still use the
existing native calls. Builds and live TrueSky verification remain open.

The AC7 queued render scopes and tickets are Rust-owned. Native leaves
guard state, pass and resource lease callbacks. The end marker frees its
ticket before reporting idle. An out-of-order end removes its copied
scope without reporting a completed pass. A failed end resolve also
retires the scope and restores enclosing state when needed. Native link,
fixtures and in-game retirement verification remain open.

The AC7 view-uniform reader and jitter removal now use the Rust view
owner in the game DLL and both linked carriers. The same offsets and
ABI remain in use. The reader handles unaligned buffers and refuses
nonfinite view data without overwriting the caller's earlier result.
The two-Rust-archive carrier links and captured-buffer fixtures remain
unverified.

The AC7 UI classifier now owns the existing declaration, widget target
and draw rules in Rust. The verified HUD producer checksum and size
also moved, with CRC32C still supplied by the shared shader hash. The
classifier still only reads the frame tap's facts and leaves diversion
to the Runtime. Fixture and gameplay verification remain open.
[UI extraction evidence](research/ac7-ui-extraction.md)

AC7's capture-derived scene colour selector and layer leases are
Rust-owned. Resource retain and release use the existing native calls.
The selector keeps the single-recombine and byte-colour boundary rules,
uses overflow-safe aspect checks and refuses short or truncated draws.
Builds, frame-tap fixtures and replay remain open.
[Scene colour evidence](research/ac7-composed-scene-color.md)

The TrueSky composite's cloud-depth parameters and resource ownership
now live in Rust. It refuses nonfinite viewports and retains the
previous target on a failed resize, while returning no output for that
draw. The D3D11 shader, bindings and dispatch remain unchanged. The
regression case and device and game verification remain open.
[TrueSky relay evidence](research/rust-ac7-truesky-relays.md)

AC7's contact-shadow shader rewrite now runs in Rust. The Runtime shader
patch service, velocity shader rehash and contact-shadow rewrite share
one DXBC checksum implementation. The Rust port preserves the hash
gate, shader tokens and engine trailer. Native callers still supply
SHA-256 and output storage. Shader replay and device verification
remain open.
[Contact-shadow evidence](research/ac7-lighting-shadow-20261004.md)

AC7's velocity shader container and signature readers and writers are
Rust-owned. The Runtime patch service shares the same DXBC chunk bounds
check. The typed compiler calls, shader instruction transforms, HLSL,
and C++ caller vectors remain in place. Shader replay, linkage and GPU
verification remain open.

AC7's velocity shader transforms now use Rust to decode instructions
and operands, validate edited register shapes and serialize the
token stream. The same HLSL, typed compilation and C++ stage assembly
remain. Malformed custom data, invalid edits and oversized temporary
declarations are refused. Shader replay and GPU validation remain open.
[Velocity token evidence](research/rust-ac7-velocity-tokens.md)

The AC7 velocity pixel stage now assembles its shader in Rust. It
preserves the helper HLSL and compiler arguments, recovers the captured
additive opacity and injects at native unconditional returns. It also
keeps aliased input alive and refuses shaders with no injection site.
Emitted bytecode parity, builds and GPU replay remain open.

The AC7 velocity vertex stage uses the same Rust DXBC codecs for
particle classification, signature linking and injection at native
returns. Source comparison matched all four helper HLSL sources and
compiler requests. The C++ wrapper keeps the typed compiler cache and
retains aliased input through reconstruction. Malformed declarations
and missing return sites now refuse. Build, bytecode parity and gameplay
remain open.

Rust also links AC7's history geometry shader and augments the native
geometry shader before each emit. The typed compiler cache and native
container payload are preserved. Bounds checks reject malformed masks,
registers and helper triangle indices. Build, emitted bytecode and
replay verification remain open.
[Geometry stage audit](research/rust-ac7-velocity-geometry.md)

The velocity hull and domain stages now share the Rust DXBC codecs.
Hull keeps the native control-point and patch constants, and domain
preserves the native output while evaluating the three control corners
for the history helper. Source comparison matched the HLSL, compiler
arguments and token literals. The superseded private C++ codecs were
removed. Build, emitted-bytecode parity and replay remain open.
[Tessellation audit](research/rust-ac7-velocity-tessellation.md)

A04 moves the helper compile requests and module caches into Rust and
links the ordinary hull, corner hull and domain programs in one call.
The history writer and readers share their HLSL table contract. The
native pass keeps D3D11 shader and stream-output creation. Source checks
and pinned formatting are complete. The Rust fixtures, WARP replay and
game verification are pending.
[A04 port evidence](research/rust-ac7-velocity-a04.md)

AC7 motion capture now stores its bounded engine and renderer-root
records and associates native queued commands in Rust. The typed
detours and guarded reads stay native. Unwinding restores the nested
observation scope, and a refused record allocation follows the
existing capture failure path. Rust also formats the checkpoint and
final artifacts using the original fields and write order. Native
file calls keep the existing narrow path behavior. Publication runs
after the Rust record lock is released. Build and captured-frame
verification remain open.
[Renderer root evidence](research/ac7-renderer-roots-20260930.md)
[Motion artifact audit](research/rust-ac7-motion-artifacts.md)

The AC7 F9 session now schedules capture intervals and retires
records in Rust. The trigger and paired samples remain at `f000`,
`f030` and `f059`. Scoped failures and late observations cannot
retire or append to a newer session. The native layer supplies
window, clock, frame-tap and directory effects. Build and captured
artifact verification remain open.
[F9 session audit](research/rust-ac7-motion-session.md)

Rust now owns AC7 F9 shader and input-layout records, including the
existing cache limits, constant-slot scan and stage/ordinal/hash file
names. D3DDisassemble and its typed blob lease stay native. The draw
and geometry captures still consume the same metadata. Source checks
passed. Build and captured shader verification remain open.

AC7 F9 now stores constant-buffer upload snapshots, serials and
readback admission in Rust. The recorded spans remain the exact
creation, update or mapped bytes lent by the native callbacks. Live
buffer reads and draw serialization remain native. Build and captured
byte comparison remain open.
[Upload observation audit](research/rust-ac7-motion-uploads.md)

Rust now formats the AC7 F9 dispatch rows and admits the shared
12,000-row stream. The native callbacks supply the observed bindings,
dispatch arguments and blob IDs. Build and captured artifact parity
remain open.
[Dispatch record audit](research/rust-ac7-motion-dispatch.md)

AC7 F9 ordinary draws, colour admission and contact-light pairing now
use Rust ownership. The writer preserves the shared row cap and blob
references, and encodes nonfinite viewport origins as JSON null.
The ordered fields match the six archived fixture rows. Fresh capture,
build and device verification remain open.
[Draw record audit](research/rust-ac7-motion-draws.md)

The AC7 adapter now owns the carrier's executable and window identity,
GameSDK probe, guarded patch bytes and post-decryption install order.
Both carriers keep their native forwarding, the guarded patch leaf
and existing exports. Rust now also schedules the optional F9 installation
and publishes carrier readiness after native preparation returns. The
automatic start attempt and startup extent admission now use that
Rust owner before native activation and compatibility fallback.
Build and game startup verification remain open.
[Carrier startup audit](research/rust-ac7-carrier-startup.md)

The AC7 velocity pass now keeps its source, layout and variant admission
caches in Rust. Variant creation still uses the same native D3D11 objects
and shader transforms. A reset discards late cache results, and the F9
capture keeps its own observation rules. Build, replay and moving-game
verification remain open.
[Velocity pass audit](research/rust-ac7-velocity-pass-cache.md)

The AC7 velocity pass's model and view history, epoch retirement,
record capacity and commit state now belong to Rust. Native D3D11
effects consume those decisions during the same observed draws.
Build, replay and moving-game validation remain open.
[Velocity history audit](research/rust-ac7-velocity-history.md)

The AC7 velocity stream owner now checks draw and indirect arguments,
sets stream and record bounds and packs the existing 112-byte b13
record in Rust. Zero and overflowing expansions refuse explicitly.
Native effects still consume the same shader parameters. Build and
replay verification remain open.
[Stream policy audit](research/rust-ac7-velocity-stream.md)

The Godot compositor probe now keeps requests, snapshots and motion
classification per effect instance in Rust. The typed godot-cpp
callbacks retain scene reads, the GPU readback pass and the same status
keys. The existing callback destruction and XR velocity extent limits
remain open, along with build and live renderer verification.
[Compositor owner notes](../integrations/godot/NATIVE.md#rust-compositor-effect-owner-seventh-slice)

Godot Vulkan Auto now asks Runtime policy using the observed physical
device and eligible providers. DLSS-G is excluded from FG Auto while
the recorded secondary-window shutdown failure remains open. The FG
selector is private to linked consumers, and the public Runtime export
declarations match the baseline. Binary exports, build and device
behavior remain unverified.
[Vulkan Auto ownership notes](../integrations/godot/NATIVE.md#vulkan-auto-through-the-runtime-provider-policy-eighth-slice)

Godot's engine module now links its viewport mode and initial scale
policy as a private Rust object through SCons. The module keeps the
existing extension settings conversion and C ABI. The documented
Windows x64 build needs a matching Rust target. Editor and player
linkage and viewport behavior remain open.

The D3D12 render host and D3D11 presentation bridge now reuse the linked
Runtime FG Auto selector. The render host keeps its Off choice, and the
presentation bridge keeps its adapter default for an unrestricted mask.
All 192 source comparisons matched the earlier host rules. The public
Runtime declarations stay the same. Build and binary export verification
remain open.
[Current source ABI check](rust-migration.md#c-abi-comparison)

Anomaly's non-MSVC native copy helper now uses a checked Win32 copy.
The previous path used an unguarded `memcpy`. The helper refuses
inaccessible input or output and preserves last error. The existing
native hooks fixture now covers both refusals. The fixture remains
unrun on this revision.

Anomaly's native observer checks the exact Standard or AVX running
build, PE image size, mapped regions and hook guards in Rust. The
native leaves copy PE and engine data under SEH and call MinHook.
Cold startup lends its verified build to synchronous observer prepare
and tracks the two input installations independently. Build, fixture
execution and live observer verification remain open.
[Observer admission audit](research/rust-anomaly-native-observer.md#prepare-admission-from-3c73e3c4)
[N01 source port](research/rust-anomaly-startup-n01.md)

## Rust orchestrator and first Anomaly port

Anomaly's Rust geometry owner now reads and interprets draw-time producer
facts using the selected profile's offsets and a guarded native copy.
Skinned snapshots borrow storage for bounded reads of up to 64 matrices
at a time. A failed read or invalid transform discards the whole palette.
The geometry fixtures and live checks remain unrun.
[Geometry reads and palette transfer](research/rust-anomaly-native-geometry.md#geometry-reads-and-palette-transfer)

Anomaly's Rust geometry owner now distinguishes owned and borrowed
child ranges before model reuse and retirement. The selected profile
and guarded memory reads remain native. The expanded geometry fixture
remains unrun.
[Child-range admission](research/rust-anomaly-native-geometry.md#child-range-admission)

Anomaly's vertex constant-buffer reads and writes now share Rust
admission for reflected size, slot and span. Native code still
supplies the selected profile and performs the guarded copy.
The geometry hook fixture remains unrun.
[Vertex constant-buffer admission](research/rust-anomaly-native-geometry.md#vertex-constant-buffer-admission)

FidelityFX Vulkan SR now records its first-use mask clear and
output copy-back in Rust. Source comparison matched the five
barriers, narrower shader and transfer access masks, capture order
and public SRVK forwarding. Builds and device checks remain open.
[FidelityFX SR recording](research/rust-vulkan-ffx-sr-images.md#first-use-initialization-and-output-publication)

FidelityFX Vulkan SR now resolves its SDK and staging dispatch in
Rust. The context export list is shared with Vulkan FG. The native
initializer keeps the typed bindings, memory query and capture setup.
The existing fixture and device checks remain open.
[FidelityFX Vulkan SR startup](research/rust-vulkan-ffx-sr-images.md#fidelityfx-vulkan-sr-startup)

The shared presentation bridge now selects the plain physical
swapchain's queue, flip effect, tearing flag and buffer overrides
in Rust. The native DXGI create and interface query retain their
existing order and status. The device fixture remains unrun.
[Source comparison](rust-migration.md#native-policy-cutover)

XeSS Vulkan SR now admits its ten required SDK exports in Rust.
The native leaf binds the complete typed table and still publishes
the session only after pinning the module. The source comparison
matched the lookup order, first-miss refusal and disabled routes.
[XeSS resolver](research/rust-xess-vulkan-sr-cutover.md#required-sdk-exports-in-the-rust-owner)

The D3D12 Streamline host now owns startup admission, SDK preferences,
device, factory and queue staging, and rollback in Rust. Source
comparison matched the signature gate, required and optional lookup
order, and enabled and disabled public entry points. The typed COM
and SDK operations remain native. Build and device checks remain open.
[Streamline startup](research/rust-d3d12-streamline-host-startup.md)

D3D12 Streamline now shares prepare ordering with FSR and XeSS, releases failed
combined-begin tokens, deduplicates controller samples and reconciles Reflex
after rejected DLSS-G options. Rust also owns the common Present hook cache and
compatibility module handoff. Source review is complete. Build, synthetic and
device checks remain open.
[Streamline generation port](research/rust-d3d12-streamline-generation-port.md)

Anomaly now owns Stage 3 burst staging and mapping in Rust. The
leased texture and device facts admit the same single-sample copy,
and map refusal retains size and format for the burst summary.
The private binding regenerated identically. Standard and AVX
device and burst runs remain unverified.
[Anomaly SR staging](research/rust-anomaly-sr-stage.md)

Anomaly now checks the current render-target identity, scope and
MSAA exclusions, and loading capture gate in Rust. The native leaves
keep the guarded Standard and AVX reads. A null target refuses the
stock capture seam before computing the texture address. The new
fixtures and game checks remain unrun.
[Frame and loading admission](research/rust-anomaly-sr-stage.md#frame-and-loading-admission)

Anomaly now assembles the round-trip, spatial, temporal preparation,
and reinsertion bindings in Rust. The native host borrows those
bindings for one draw and no longer stores pointers to seam-local
records. Source comparison matched the binding order and shader
inputs. The updated fixtures and game checks remain open.
[Seam draw bindings](research/rust-anomaly-sr-stage.md#seam-draw-bindings)

Anomaly now owns the OnFrame profile admission, engine frame report
order, and buffered hardware receipt gate in Rust. The native detours
keep the original engine and DirectInput calls and unwind cleanup.
Source checks matched the Standard and AVX guards. Updated fixtures
and held-input game checks remain open.
[Verified input entry](research/rust-anomaly-input.md#verified-input-entry-and-buffered-receipt)

Native D3D12 DLSS now resolves the required interposer and feature
addresses in Rust. The native capsule binds the typed pointers
after admission. Source comparison matched the ordered lookups and
the extra Present callbacks required by the owned route.
[Source comparison](rust-migration.md#native-policy-cutover)

XeSS Vulkan SR now plans its guide staging, private output barrier
and copy-back in Rust. It shares the typed Vulkan recorder with
the FG sequence and preserves the SR output layouts and extent.
Source comparison covered the capture order and disabled routes.
[XeSS SR transfers](research/rust-xess-vulkan-sr-cutover.md#transfer-recording-in-the-rust-owner)

Anomaly's FG input owner now leases and retires the compute shader
and constant buffer in Rust. A refused buffer allocation releases
the shader and retains the existing device failure latch. The
shader and normal recording order match the prior source.
[FG pipeline ownership](research/rust-anomaly-fg-stage.md#input-pipeline-ownership)

Anomaly's overlay owner now orders the DirectInput and DXGI probe
calls and admits their methods in Rust. Typed COM and Win32 calls,
native detours and the existing rollback remain in place. DLSS and
FidelityFX Vulkan FG now share the Rust staging copy sequence while
preserving their respective copy extents, capture inputs and fence
order. Device and game checks remain open.
[Anomaly probes](research/rust-anomaly-overlay-probes.md) ·
[Vulkan FG staging](research/rust-vulkan-dlss-fg-cutover.md#guide-and-hud-less-staging-commands)

The Anomaly adapter now implements the overlay's DirectInput and DXGI callbacks
and COM access in Rust. The input gate reports from the buffered read that
consumed the records. Diagnostic controls appear in the panel, and option
callbacks report persistence failures. Source and binding checks are complete.
Build, fixture and game verification remain open.
[Anomaly input and overlay port](research/rust-anomaly-n02.md)

The native D3D12 overlay now owns its three command and descriptor
slots in Rust. The fixed slot array keeps partial references until
initialization releases the fence and tears down the renderer.
The private binding regenerated identically. The backpressure
device fixture remains unrun.
[D3D12 overlay slots](research/rust-d3d12-overlay-slots.md)

The D3D11 host now constructs the bridge setup, chooses the install
mode and publishes startup FG Off in Rust. Anomaly's Rust FG input
owner keeps all six staging textures and views and retains the
previous complete set when a resize allocation fails. The private
FG binding regenerated identically. Both device and game paths
remain unrun. [Bridge installation](research/rust-d3d11-host-bridge-install.md) ·
[Anomaly FG stage](research/rust-anomaly-fg-stage.md)

D3D12 SR now owns the constant SDR exposure texture and upload row
through allocation, initialization and rollback in Rust. A null
mapped address is refused without publishing either reference. The
linked and DLL SR12 fixtures remain unrun.
[Source comparison](rust-migration.md#native-policy-cutover)

Vulkan-to-D3D12 interop now admits its required dispatch table in
Rust. The native capsule binds the same 13 entries after successful
resolution, with the original lookup order and error status.
The device fixture remains unrun.
[Source comparison](rust-migration.md#native-policy-cutover)

The D3D11 host now adopts the outer creation facade in Rust. It
checks the context and facade identity before transferring the caller's
reference, and releases any facade returned with a failed wrap. The
presentation owner now also owns the three command allocator and list
pairs and retains them if GPU drain fails. Source checks matched both
private callback tables. The device and game paths remain unrun.
[Outer-chain adoption](research/rust-d3d11-native-chain-adoption.md) ·
[Presentation command slots](research/rust-d3d11-present-slots.md)

The shared presentation owner now holds the interop queue, completion
fence, and both event handles. The D3D11 path keeps a separate interop
queue, and native D3D12 retains the engine queue. The application still
receives its own duplicate of the ready event. Source review matched
the drain and release order for shared and private hosts, including
retention after failed drain. The new fixtures and device checks remain
unrun. [Presentation synchronization](research/rust-d3d11-present-sync.md)

The same owner now holds the provider retirement fence and issues each
successful receipt under one nonblocking lock. Reentered retirement
returns `NOT_READY` with zeroed outputs. The native wait, optional
command list submission and signal retain their source order. The
extended fixture and device checks remain unrun.
[Provider retirement](research/rust-d3d11-present-sync.md#provider-retirement-receipt)

Native FidelityFX Vulkan FG now plans the HUD-less copy and its
image-layout barriers in Rust. The SDK, capture and WSI borrow still
use the same order. Source checks covered the private table and the
disabled feature path. [Copy sequence](research/rust-vulkan-ffx-fg-copy-plan.md)

The XeSS Vulkan bridge now admits Present inputs and sequences
resource transitions, copies and SDK tags in Rust. Aliased inputs
transition once and still receive each required tag. Source comparison
matched both HUD-less modes and the 27 C exports. Standalone and
integrated builds and device checks remain open.
[XeSS Present inputs](research/rust-vulkan-xess-present-inputs.md)

FidelityFX Vulkan SR staging images now allocate in Rust through
the shared typed image leaves. The owner selects the first compatible
device-local memory type, or the last compatible type when there is
no device-local choice. It retains each partial handle for the same
rollback. Link and device verification remain open.
[Staging-image ownership](research/rust-vulkan-ffx-sr-images.md)

The legacy D3D11 Streamline SR owner now resolves the interposer
and DLSS feature functions in Rust. The source comparison matched
the required entry order for owned and borrowed hosts and the
SDK-disabled C++ stubs. Enabled and disabled builds and binary
exports remain to be checked. [Streamline resolution](research/rust-d3d11-streamline-resolution.md)

D3D11 Streamline SR now owns legacy initialization, shared-host
borrowing and the device feature sequence in Rust. The typed
initializer keeps the pinned SDK preferences and the same
feature and log effects. Builds, fixtures and live checks remain open.
[Owned and shared SR startup](research/rust-d3d11-streamline-resolution.md#owned-and-shared-sr-startup)

D3D11 Streamline SR now owns capability reporting and render plans
in Rust. It releases the temporary DXGI interfaces before the SDK
support query and uses the same quality mapping for plans and
frame options. Builds and live checks remain open.
[Support and render planning](research/rust-d3d11-streamline-resolution.md#support-and-render-planning)

D3D11 Streamline SR resource tag selection, extents and shared
D3D12 states now come from Rust. The typed native call keeps the
existing immediate-use lifetime. Builds, fixtures and live checks
remain open.
[SR frame resource tags](research/rust-d3d11-streamline-resolution.md#sr-frame-resource-tags)

D3D11 Streamline SR now selects its frame token and constants
routes in Rust. The shared route preserves the host source and
original viewport, and the owned route keeps the normalized slot.
Builds, fixtures and live checks remain open.
[Frame tokens and constants](research/rust-d3d11-streamline-resolution.md#frame-tokens-and-constants)

Anomaly's Rust motion owner now checks the existing Standard and AVX
profile guards in the same order. The native leaf supplies the profile
and the guarded reader. FidelityFX Vulkan SR and FG also share one
Rust alias table. Pinned formatting, locked Windows Cargo metadata and
whitespace passed. Build, link and runtime checks remain open.
[Motion admission](research/rust-anomaly-motion-stage.md)

Anomaly now admits motion windows and selects the MSAA refusal in
Rust. Native leaves provide the same guarded position, view and
frame facts. The motion target leases, history and capture sequence
match in source. Device and game checks remain open.
[Motion window admission](research/rust-anomaly-motion-stage.md#motion-window-admission)

Anomaly now admits successful stock shader creation and reads its
guarded interface and filename in Rust. The native detour still
supplies its own return address and stack to the compiler-frame
check. Source checks matched the verified offsets and shader
pack. Builds, fixtures and live shader checks remain open.
[Stock shader creation](research/rust-anomaly-motion-stage.md#stock-shader-creation-admission)

Anomaly's original motion draws and zero-motion replay now run on
native frames. Rust still owns admission, resource pins and state
restoration. Native scope guards also restore state during C++
unwind. The updated motion device fixture and game checks remain
unrun.
[Native draw calls and restoration](research/rust-anomaly-motion-stage.md#native-draw-calls-and-restoration)

The standalone NGX shim publishes its module and 12 resolved symbols
from Rust while the C wrappers call NGX from the same native frames.
The Vulkan-to-D3D12 FG port moves transfer, barrier and placeholder-clear
plans into Rust for both XeSS and FSR4. Source, script syntax, pinned
format, locked Windows metadata and whitespace checks passed. Link,
binary export, fixture and runtime verification remain open.
[NGX loader](research/rust-vulkanstory-ngx-loader.md) ·
[Vulkan FG transfers](research/rust-vulkan-dx12-copy-plan.md)

The native FidelityFX Vulkan FG owner now resolves startup dispatch
and owns the unpublished capsule in Rust through resource and
capture setup. It closes the capsule on refusal before WSI
registration. Source checks matched the lookup order and public
declarations. Build, fixture and live verification remain open.
[FidelityFX Vulkan FG startup](research/rust-vulkan-ffx-fg-copy-plan.md#fidelityfx-vulkan-fg-startup)

The shared Vulkan-to-D3D12 FG owner now resolves the device and
instance dispatch, admits the required format features, and cleans
up refused startup in Rust for both XeSS and FSR4. Source checks
matched the native bind and query order. Builds and fixtures for
this port remain open.
[Graphics admission](research/rust-vulkan-dx12-copy-plan.md#graphics-admission-and-dispatch-setup)

XeSS and FSR4 now share Rust admission for Vulkan Present input and
the binary and timeline wait plan. The native calls keep the typed
Vulkan result and submit records. The new fixture checks the refusal
and maximum-wait paths. Builds, fixtures and live checks remain open.
[Present admission and waits](research/rust-vulkan-dx12-copy-plan.md#present-admission-and-waits)

The common Vulkan-to-D3D12 importer and FG graphics leaves call typed
Vulkan procedures from Rust. FSR4 and XeSS wait for the consumer timeline
and Vulkan source fences before replacing depth and motion on a render
extent change. The shared capture owner now provides the readback adapter
directly from Rust. The shared C++ device fixture remains. Source checks
compared the Vulkan names and private table layouts,
and locked Windows metadata resolved. Compile, link, fixture and device
checks remain open. [Vulkan D3D12 interop](research/rust-vulkan-dx12-interop.md)

Shared texture capture now performs Vulkan and D3D11 readback in Rust.
Vulkan publication requires proof for the exact group or retired command
buffer. D3D12 and Vulkan share the legacy metadata and artifact names carry
view and generation. Source and ABI checks passed. Builds, fixtures and
device checks remain open. [Shared capture port](research/rust-shared-capture-port.md)

The standalone FSR3 bridge now binds the SDK and owns the SR, FG and
swapchain transactions in Rust. It keeps the active FG and swapchain
paired through disable, drain, UI clear and checked retirement.
Native leaves still build the FidelityFX and Vulkan descriptors.
All 21 export signatures matched in source. Link, binary exports,
device and external host presentation remain unverified.
[Standalone FSR3 ownership](research/rust-vulkan-fsr3-legacy-sr.md)

Anomaly's Rust geometry owner now checks the selected profile's image
size, layout sites, detour entries and unique prefixes. The native
profile data, guarded read and typed trampolines retain the same
Standard and AVX contracts. [Geometry admission](research/rust-anomaly-native-geometry.md)

Anomaly's SR driver now chooses the order of the Standard and AVX
stage guards in Rust. It uses the existing checked-image reader,
native profile tables and stage callback ABI. Source comparison
matched the first refusal names, call order and private record layout.
The SR hook contracts for both profiles remain unrun on this port.
[SR guard admission](research/rust-anomaly-sr-stage.md#sr-guard-admission)

The [migration plan](rust-migration.md) records the baseline and cutover order.

Rust owns CPU input receipt replay and FG frame selection. The dispatcher
admits bounded callback reordering under its existing lock. FG selects a
frame only when its session, generation and pacing and simulation timestamps
match without ambiguity. The C++ dispatcher forwards and receipt callbacks
are removed. Source review checked the public dispatch and the private FG
platform layout. The original dispatch fixture still calls the C interface,
and new Rust cases cover the policy. Build and fixture runs remain open.

The D3D11 host owns failure and sequence capture budgets under its Rust
graphics lease. The former C++ policy leaves and their private callbacks
are gone. The three-failure cap and four-frame sequence still defer to
active capture requests. Independent Rust cases cover those rules in place
of the retired C++ fixture. Source review checked the two failure call paths
and the private platform table. The Rust cases and native builds remain unrun.

The combined AC7 and Loader carriers read and write the twelve shared
sharpening keys through the Rust settings store. Reads use the same float
parser and range checks as the Runtime. Failed writes attempt to restore
the previous key values and log one error. Source review traced both carrier
links. Build, export comparison and an AC7 settings round trip remain open.
The carriers also use the same store for `DLSSPreset`. Their existing
`RSF_DLSS_PRESET` fallback and preset validation still apply at startup.
The Runtime callback host uses the same write function.
Within each Runtime instance, the store serializes section rewrites,
partial updates and multi-key reads. A layout or preset key write can no
longer interleave with the store's section read and replacement. Source
review checked that the lock covers only file operations and formatting.

Anomaly's Rust bootstrap now installs DirectInput observation before
requesting the engine's guarded `CInput::OnFrame` slot. It handles refusals
and logging, and the native callbacks install the hooks. Source review
traced cold entry and quiesce through the existing plugin session ABI.
The updated bootstrap fixture remains unrun.

The native Anomaly motion facade is linked only by fixtures. The FG
depth input leaf uses the existing guarded image read for `HW.pBaseZB`.
If the read fails, the Rust stage skips adapter depth capture through
its existing missing-view path. Source review traced both call paths.
Build and device validation remain open.

The D3D11 to D3D12 SR bridge retries a refused frame fence signal on
the stream that issued the work. It flushes a D3D11 retry before the wait
and keeps the session and resources alive until retirement is proved.
The regression covers both D3D11 signal points and the D3D12 submission
signal. It remains unrun.

Rust now owns the optional NVAPI Reflex audit hook transaction. It pins
the native modules before enabling the hooks. A partial enable disables
the batch and retains its trampolines, so an already-entered call keeps
its original. Source review checked the native call table. The new
rollback fixture and SDK validation remain unrun.

Anomaly's SR stage lends its logger through the private prepare call.
The native twin-allocation host uses it for that synchronous operation
and the Rust configuration keeps the stored callback. The private
binding was regenerated. Source review and formatting checks passed.
Native validation remains open.

The FidelityFX version parser feeds the Rust selector. The unused
native selection loop was removed. The shared HLSL compiler helper
now releases compiler blobs before unloading the module even if
copying bytecode throws. Source review checked callers and lifetime
order. Native builds remain unverified for these changes.

The Direct3D bridge checks resize admission before configuring the
provider and publishes the accepted count for strict native D3D12.
Refused oversized requests leave lazy creation pending, and an
outstanding application-buffer lease prevents changes to FG options.
The updated resize fixtures remain unrun.

The Direct3D presentation bridge keeps replacement admission closed
while an unpublished owner drains. It advances the provider generation
after successful retirement. A failed drain keeps the owner retained
and prevents a successor switch from releasing its outstanding input
leases. This also covers initialization that created a provider before
the facade could be published. Source review checked publication order.
The updated retirement fixtures remain unrun.

The native D3D12 overlay retains the value for its outstanding fence
event. A wake before that value keeps the event armed. Device removal
allows failed-path retirement without waiting for a wake and still
refuses slot reuse. The updated retirement fixture remains unrun.

The FidelityFX 4 Vulkan SR bridge now owns provider admission, frame
validation, submission and checked retirement in Rust. The Runtime
and standalone bridge use the same compatibility archive. Rust checks
the public record size first and pins the effective SDK provider to
the selected ID and version family. Source comparison matched the 15
bridge signatures. Both links, the CPU fixtures and device behavior
remain unverified. [Bridge cutover](research/rust-vulkan-fsr4-sr-cutover.md)

Anomaly's window subclass reports delivered input messages only while
the observer admits callbacks. In-flight reports hold the same lease as
native observations, so quiesce waits for them. The subclass continues
to forward window messages. Source review checked the callback path.
The synthetic admission case remains unrun.

The Streamline Vulkan bridge tracks FG options, swapchain changes and
resource release in Rust. The interposer lifecycle guard serializes
these SDK calls with shutdown. A swapchain change during an SDK call
invalidates the previously applied cache revision. Reflex and PCL bind,
options and state queries use the same guard. Rust normalizes the legacy
Reflex mode and interval before the typed SDK call. Rust also leases
the bridge's current SDK token against shutdown. Reflex sleep leaves
older Present markers free to run, and the latency ledger owns the
indexed frame associations. SDK callback diagnostics use Rust atomics
for verbosity, pending Present errors and worker timeouts. Shutdown
clears the pending error; the timeout count spans interposer sessions.
Source comparison matched all 45 C export signatures. The fixtures,
builds and SDK behavior remain unverified.

Anomaly quiesce now releases the adapter's private FG input pass once
native callbacks and scopes have drained. It clears camera and input
history while preserving the monotonic source counter. Runtime keeps
ownership of its copied resources. Source review checked the handoff
and retry order. The updated fixtures and live quiesce remain unrun.

Anomaly quiesce waits for an admitted FG frame even between native
callbacks. The existing engine End callback can still close that frame
and its Runtime binding after observation admission closes. Source
review traced the paired callbacks. The updated retirement fixture is
unrun, and live quiesce remains unverified.

Anomaly closes any accepted FG window scope before the engine resets its
device. The interrupted render frame cannot prepare more inputs or open
another window scope. End still reports the real engine boundary, and a
Present before reset cannot clear the next frame's history reset. Source
review traced the reset hook and scope calls. The updated synthetic
fixture remains unrun.

Direct3D latency retirement now checks the dispatcher session before
retiring SDK tokens or advancing completion. An earlier session or
zero source cannot affect the current one. Source review traced the
session admission and retirement paths. The updated CPU fixture remains
unrun.

The XeSS Vulkan/DX12 bridge uses one Rust session owner for creation,
startup, per-frame presentation, completion and checked retirement.
The integrated Runtime and standalone bridge compile the same source.
Rust also owns diagnostic capture admission, source IDs and readbacks.
Retirement publishes only after the bridge's queue proof. It closes
capture admission before unregistering SDK callbacks without the lock.
Native leaves retain typed SDK and graphics effects, fingerprint
checks and file output. Rust aggregates the optional XeSS CPU and GPU
timing samples and uses the original native stderr format. The
integrated Present checks record sizes before ABI versions. Source
comparison matched all 27 C exports and checked the private call table.
The timing and guard-page fixtures, both build paths and device
behavior remain unverified.

The FidelityFX 4 FG bridge used by Vulkan now owns admission, frame
validation, queue and fence sequencing, telemetry and checked retirement
in Rust. Its typed SDK calls and retained descriptors stay in the native
binding. The bridge and outer Vulkan owner share the private record
definitions. Source review checked call order and the optional build
guard. The CPU contracts, builds and device paths remain unverified.

The Streamline Vulkan bridge owns its interposer lifecycle in Rust. It
holds the verified file through admission and keeps the plugin paths,
feature selection, shutdown result and Vulkan resolvers for their
respective lifetimes. The Runtime and standalone Windows GNU bridge link
the same lifecycle crate. Its Windows bindings are gated. Overlapping
start and shutdown return busy, and unload can be retried when the
lifecycle is idle. Source comparison matched all 45 C export signatures. The
synthetic fixtures, both build paths and SDK runs remain unverified.

The D3D12 render host refuses slot reuse after device removal even when
the existing context matches the requested extent. Its wait, capture and
slot selection share the removal check. The D3D12 capture drain also
uses the shared fence wait. FG leases, readbacks and OpenGL share the
Rust fence state check. Source review traced all three outcomes and
the updated render-host fixture remains unrun.

Shared D3D12 waits now require the requested fence value after an event
wake. Rust handles stale wakes and device removal within the original wait
budget. The OpenGL bridge and D3D12 render host use the same wait,
and the OpenGL owner refuses the device-removal fence value in its fast
path. Source review traced the C++ callers and their Rust linkage. The
stale-wake and removal fixtures remain unrun.

Vulkan presentation now honors `NO_FG` and `AMBIGUOUS_ID` when the
explicit output record supplies those flags after prepare. A matching
source is still consumed by the ordinary Present. The identity-only
prefix remains accepted. Source review traced the shared gate into
FidelityFX, DLSS and XeSS. The updated prefix and submission fixtures
remain unrun.

Shared overlay input collection now validates the caller's record and
builds the idle or active input snapshot in Rust. The native callback
supplies the Win32 cursor sample. Non-finite frame deltas become zero
before reaching the panel. Source review traced collection order and
matched the public entry point. The new synthetic case remains unrun.

The callback render host refuses overlapping lifecycle calls, including
stop reentered from an adapter policy callback. A refused start keeps the
host if quiesce or stop refuses. After both succeed it releases any SR
contexts and overlay used by startup callbacks before freeing their log
context. The updated callback and module lifetime fixtures cover both
DLL and embedded entry points. Source and formatting checks passed.
The fixtures and device paths remain unrun for this revision.

The Runtime admits one GameSDK session per loaded plugin module. A duplicate
prepare refuses before entering the plugin's callbacks. The module reservation
follows the DLL lease through failed prepare, quiesce and stop. Source review
traced the lease transitions. The extended lifetime fixture covers duplicate
prepare with a prepared session and with a retained cleanup handle and remains
unrun.

The presentation bridge publishes the graphics device and upload queue as
one snapshot and clears it during retirement. The graphics query checks
the caller's size field before writing the full record. Source review
traced publication and retirement. The updated short-buffer and retired
owner fixture remains unrun.

Anomaly's motion stage now locates foliage constants and validates their
buffer ranges in Rust. It borrows the stock reflection from the native
shader preparation record. Source comparison matched first-name lookup,
tree and detail size rules, and optional constant handling. The existing
contract and device fixtures remain unrun for this revision.

The Rust D3D11 overlay renderer now owns the bindings saved around each
draw and restores them before releasing the COM references. Its narrow
native read and restore calls leave the caller's render targets and UAVs
bound. Source comparison checked the getter and setter order, private
x64 layout, shader bytes and public entry points. Build and device
validation remain open for this revision.

Anomaly retries a refused Observe start with the session that successfully
prepared. If Runtime returns a cleanup handle after refused prepare, Anomaly
retries quiesce and stop before preparing again. It preserves stage setup
and keeps the handle if either cleanup step refuses. The updated synthetic
fixture covers startup and cleanup retry and remains unrun. Source review
matched these calls to the Runtime session phases and existing exports.

The Rust plugin session constructs the versioned lifecycle arguments and
checks the detection result. The C++ bridge catches native plugin
exceptions and returns them as `NOT_READY`. Source review matched the
new private signatures to GameSDK ABI 24. The updated lifetime fixture
covers the lifecycle record headers and DLL release after an unknown
or throwing detection callback. The fixture remains unrun.

The Runtime keeps a refused plugin quiesce in progress. Start cannot
report success while the plugin may have partially retired, and
stop still requires quiesce to finish. The DLL remains leased through
the retry. Plugin sessions now use the shared Rust module lease with
the existing dependency search flags. An unrun fixture covers a
started plugin refusing quiesce and then retiring successfully.
Source review checked the state transitions and public ABI.

The Rust Anomaly FG adapter checks the Present event size and counts
completion only for the accepted window's source and swapchain.
Device reset clears borrowed SR identity and camera history before
the engine tears down its targets. The existing Runtime remains
responsible for submitted GPU leases. Source review matched the
bridge's event identity to the adapter window. The new synthetic
case and device reset behavior remain unverified for this revision.

The Anomaly frame-generation port moves typed DXGI probes and the policy
fixture to Rust. D3D11 FG table version 4 takes normalized resources
with the SDK pass identity. Runtime tag metrics and adapter eligibility
are reported separately. Skipped input updates close their source
without assigning a simulation-end timestamp. Source, binding and
formatting checks are recorded in the [N08 follow-up](research/rust-anomaly-fg-stage.md#n08-source-follow-up).
Build and device verification remain open.

Anomaly observer quiesce now suspends the overlay before retiring the
render stages. It closes DirectInput capture and hides the window
panel. The next accepted Present performs the Runtime reset on its
graphics owner, and a restarted session can reuse the configured
service after that reset. If the engine sends no further Present,
the hidden renderer remains retained. An unrun synthetic case covers
suspension and the deferred reset. Live teardown remains open.

The Runtime overlay keeps a device reset pending until its stop call
succeeds. Present and reset share panel admission, so a refused stop
cannot open a new renderer. Anomaly keeps the previous chain identity
and retries the reset before admitting a new chain. Unrun fixtures
cover both refusal paths. Source review checked the existing notify
ABI and its Present-thread call order.

Anomaly observer lifecycle and renderer status calls now return busy
when a startup or teardown callback reenters the observer. The status
output is written only after a complete snapshot. An unrun fixture
covers lifecycle and status admission while the owner is locked.
Source review checked the callback path without changing the ABI.

The Rust Anomaly observer now records completed teardown steps.
After a refusal it resumes with the first unfinished owner and
keeps the session available until callbacks drain. A stop after
successful quiesce leaves the retired stages alone. An unrun fixture
covers busy input and native callbacks, a motion refusal after SR
retires, and a successful retry without repeating SR teardown.
Source review checked the callback order. Live validation remains open.

Anomaly's Rust SR stage now closes render admission when hook removal
is refused. It restores an open reduced-resolution window and keeps
the renderer resources for a removal retry. Late callbacks recheck
admission under the stage lock before using those resources. The
updated but unrun fixture covers refusal, stock callbacks and
successful retry. Source review checked hook and resource order.
Live teardown and restart remain open for this revision.

The Rust D3D12 FSR session now owns the optional FSR4 compatibility
lease. It resolves the SDK before acquiring compatibility and holds
the lease until after context and native binding cleanup, then unloads
the module. The standalone bridge keeps its existing C++ facade over
the same acquisition. Source review checked success and refusal order
without changing the compatibility implementation. Build and FSR4
runtime validation remain open for this revision.

D3D12 reconstruction, Vulkan reconstruction and D3D12 frame
generation now choose FidelityFX providers through one Rust selector.
The selector honors the requested opaque ID, ranks revisions within
the requested major version and keeps the first SDK row on a tie.
Native queries supply the same parsed records, including for the
FSR4 capability probe. Source review matched the private layout and
the existing zero-ID handling at each caller. The selector fixture
and SDK provider selection remain unverified for this revision.

The Rust D3D11-on-12 overlay now resolves and retains the system
device-creation export. Shared-panel startup and stop require the
same D3D11 device, so a wrapped overlay cannot claim or stop a panel
owned by another device. The wrapped owner also refuses a short
intent output. The private ABI was regenerated, and the updated
synthetic fixture covers a foreign-device start and stop. Build and
device validation remain open.

Anomaly's DirectInput and DXGI probes now transfer their counted
System32 DLL references to the Rust hook registry. Discovery and
unqueued creation refusals release the reference. Once a batch can
activate, Rust keeps the reference with its trampolines. The shader
compiler uses the same adapter module lease. Source review checked
the transfer and refusal order. Live hook installation and rollback
remain unverified for this revision.

The D3D12 FSR and XeSS frame-generation bindings now own their
SDK module references in Rust. The shared owner releases the typed
native bindings before unloading FidelityFX, XeSS FG or XeLL. The
FSR4 capability query uses the same module lease. Source review
checked loader and resolver signatures, refusal cleanup and the
feature-disabled stubs. The Windows builds and SDK checks remain
open for this revision.

The shared D3D12 Streamline host now owns its interposer reference
and SDK directory in Rust. Callers borrow the same module through
the existing host entry. Native disposal calls SDK shutdown and
clears diagnostics before Rust releases the module. Startup and teardown
refuse overlapping calls. An unrun fixture covers disposal before
unload on failed startup. Source review checked the private table
and module lookup. Build and SDK validation remain open.

The legacy D3D11 Streamline session now owns its interposer reference
and preference paths in Rust. Native code keeps signature verification,
export resolution and the typed SDK calls. The module, device and
borrowed paths stay alive until native shutdown and diagnostic cleanup
return. Vulkan and Direct3D use the same Rust SDK module lease.
Source review checked the owned and shared-host paths and the private
callback table. Build and SDK validation remain open for this revision.

The native D3D12 DLSS registration now acquires and retains the
Streamline module in Rust. It checks the retained engine module's
path before using its exports and keeps the plugin search path alive
through SDK shutdown. Shared-host registrations still borrow the
host's module. Native leaves keep the existing signature checks,
typed exports and SDK calls. Source review matched the private table
and checked the owned, engine and shared paths. Build and SDK
validation remain open for this revision.

The Rust FidelityFX Vulkan owner now records successful disable and
drain before it starts releasing SDK objects. A teardown retry skips
that work and resumes release. Public drain no longer dispatches to
an SDK swapchain that may already be destroyed, and retiring owners
no longer take submission notifications. Updated synthetic fixtures
cover the drained retry and refused cleanup after partial creation.
Source review traced the native dispatch. The revised fixtures and
device teardown remain unverified.

The Rust Vulkan FG dispatch retains a partially created provider
for cleanup without admitting new rendering or latency work. The
provider and chain outputs still carry the existing destroy-retry
contract. The Godot consumer keeps those outputs and reports FG
available only after successful creation. An unrun dispatch fixture
covers a retained owner refusing frame work and latency attachment.
Source review checked the retry path and public ABI headers.

The Rust D3D11 and D3D12 hosts refuse overlapping creation before
installing any hooks or callbacks. D3D12 publishes the frontend only
after filling the service table, and a refused install allows a
later retry. D3D11 returns busy if an installation callback tries
to create the host again. An unrun D3D12 fixture covers contention
and refusal. Source review traced both install paths. The native
D3D12 render host remains process-lifetime under its existing ABI.

The Rust D3D12 overlay owns the UI instance and panel DLL lease,
calls the panel's frame and texture APIs directly, and publishes only
after renderer and input startup succeed. A refused startup drops
the unpublished panel, renderer and fence. Failure retirement still
waits for the graphics fence and input uninstall before releasing the
UI and GPU resources. The native leaves retain the D3D12 calls and
window input bridge. The private ABI was regenerated and reviewed.
Build and device validation remain open for this revision.

Anomaly's overlay finishes discovering hook targets before it
registers any. It shares the Rust cleanup for never-enabled hooks
and the native MinHook calls with the engine hooks. A refused create
removes only the new, unqueued entries. A refused activation retains
the original trampolines for callbacks that may already be in flight.
An unrun fixture case covers a late discovery refusal and a refused
second create. Source review checked callback publication and the
private table layouts. Build and live overlay validation remain open.

Rust now decides when Anomaly's invalidation replay can run, keeps
the original OM and pixel shader bindings alive, and restores them
after the draw. The D3D11 leaves and the existing device fixture use
the same replay transaction. A missing depth state now refuses the
replay before it can bind the default depth-writing state. Source
review matched the COM reference and restore order, including its
shader class instances. The device fixture and live replay remain
unverified for this revision.

Anomaly's Rust motion device now holds the constant-buffer operations
from its admitted Runtime shader pack. The native global API copy is
gone. Standalone fixtures still use the typed D3D11 leaves. Source
review checked that Runtime refusals still propagate and release any
returned allocation, and that uploads still bind the correct shader
stage at b13. Device recreation and shader reload remain unverified for
this revision.

Anomaly geometry retirements now go straight to the Rust motion
stage's queue. The stage drains its own queue before updating draw
history. Stopping motion closes admission, discards pending keys, and
resets history so a later start cannot use snapshots from before the
stop. The native geometry fixture retains its callback seam. Source
review matched the smaller private motion table and traced lock
ordering between geometry retirement and motion quiesce. Actual
hook disable and restart remain unverified for this revision.

The Vulkan latency host now releases the process pacing claim. An
AMD device retirement releases it after outstanding calls drain if
AMD still holds it and no Streamline pacing call is active. A live
XeLL or Reflex claim survives AMD retirement. The host also checks
the claim again before starting Streamline pacing and releases it
after successful full shutdown. The CPU fixture covers AMD retirement
with XeLL holding the claim. Source review traced both teardown
paths and their lock order. The updated fixture and device behavior
remain unverified.

The Rust Anti-Lag owner supplies the active Direct3D API and the
generated-frame attachment decision to the AMD leaves. The native
driver no longer keeps a second API value. If a session reset cannot
disable pacing, Rust retires the SDK context under its exclusive
lease. The SDK storage keeps a stable address for DXGI private data.
The existing CPU fixture covers API routing, generated-frame
attachment, and the refused-disable path. Source checks covered the
SDK and stub paths. The updated fixture and device behavior remain
unverified.

Rust now preflights and applies Anomaly's jitter correction to the
position reconstruction constant. The native hook supplies the verified
engine layout and guarded reads, writes and buffer lookup. Stages
sharing the same vector still apply the correction once. Partially
overlapping vectors are refused before any staging write. The existing
reconstruction fixture now covers that case and calls the same Rust
implementation as the hook. The fixture and in-game reconstruction
remain unverified for this revision.

Rust now owns the Anomaly SR host's cached D3D11 pass, current reduced
target batch, and retirement counters. The swap host keeps one opaque
handle. It borrows the depth texture from the current batch and uses
the same shader variants and draw bindings. Source review traced pass
reuse, refusal and teardown through the swap host and matched the
private Rust and C++ declarations. Build and runtime coverage remain
open for this revision.

Rust now retains the guarded Anomaly motion shader frame and original DXBC
through variant preparation and reload. Both paths borrow the same owned
bytes. An unreadable DXBC range is refused before Runtime preparation can
compile, update its cache or create a replacement. The fixture facade uses
the same frame decoder. Native code supplies the exact profile layout,
guarded reads, and typed reflection and shader calls. Source review matched
the new private table to the Rust layout and confirmed the public shader
service and game ABI headers are unchanged. Build and live shader reload
coverage remain open for this revision.

Rust now owns the DLSS-G compatibility wrapper lease, capacity
and status. Native code keeps the existing compatibility checks and
effects. Release shares the transition guard, status reads use one
snapshot, and diagnostics run after the update guard is released.
An overlapping update leaves the current state intact. The
third-party compatibility sources are unchanged.

Rust now serializes D3D11 observer install and uninstall across
vtable probing, callback registration and hook retirement. The native
probe still supplies the same slots. Busy or reentrant lifecycle
requests return the existing not-ready result. Creation callbacks
stay serialized without holding the registration lock, and teardown
waits for their capture updates. Nested notifications are skipped.

Rust now owns the Streamline SDK log registrations used by legacy SR
and the D3D12 host. The D3D12 callback no longer dereferences the
native host. Teardown drains the listener before unloading the
interposer. SDK logs skip a busy listener, and a lifecycle request
from a diagnostic callback can retry after the callback returns.

Rust now owns D3D12 sharpening admission, pipeline leases, scratch
replacement and the three descriptor slots. The caller still retires
each slot before reuse. Native leaves preserve the descriptor layout,
barriers, copy order, shaders and constant packing. The internal
facade keeps its run and counter calls, and an inactive pass performs
no GPU work.

The D3D11, D3D12 and Vulkan sharpening bridges share one mapping from
the public options to the native constant-packing inputs. D3D11 refuses
immutable outputs before dispatch because the post-pass copy requires
a writable destination.
[Direct3D CopyResource requirements](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-id3d11devicecontext-copyresource)

Rust now owns the D3D11 sharpening device and resource cache,
image admission, filter selection and dispatch counter. A failed
scratch view allocation leaves no incomplete texture in the cache,
so the next frame can retry. Native leaves keep the Direct3D calls
and the existing shaders and constant packing. Vulkan and Direct3D
share the Rust depth-guide fallback. The earlier depth-mode color
fix is unchanged.

The integrated ABI audit found a D3D11 snapshot layout mismatch.
The Rust class-instance arrays now use the SDK's 253 entries, with
matching native and Rust size and offset checks. The public state
storage is unchanged. The audit's source evidence and the remaining
link checks are recorded in the [migration plan](rust-migration.md#source-audit-checkpoint).

Rust now owns resizing and replacement of the legacy D3D11 motion
resolver. A refused allocation keeps the previous resolver and
generation. Native code supplies the existing create and destroy calls.

Rust now owns Anomaly's fixture shader include search and buffers,
plus the live compiler and reflector module leases. Includes still
try `r3` first, preserve Windows path construction and refuse sizes that
do not fit the compiler's count. Live recompiles still use the loaded
engine compiler, with the System32 reflector kept alive until pack
retirement. Native D3D and engine include calls keep their signatures.

Rust now owns the D3D11 observer's on-disk module image and PE64 byte
lookup. Truncated section tables and virtual section tails are refused.
Only successfully parsed files are cached. Native hook setup uses
the Windows module extent and the same route and trampoline code.
The public observer ABI is unchanged.

The D3D12 and standalone Vulkan FSR4 paths now share one Rust INT8
compatibility owner. The device lease, exact SDK fingerprints and
entry-byte checks retain their existing scope. The standalone bridge
now links the Rust library for the Windows GNU target. Native calls
still perform COM checks and install the guarded hook. Source checks
and open verification are recorded in commit `2618f9f`.

Rust now owns the D3D12 Streamline host and DLSS-G provider, including
host admission, frame tokens, Reflex/PCL state, generation options
and retirement. Native leaves keep the typed SDK and COM calls.
The host checks optional PCL entry points and synchronizes borrower,
capability and sleep counter access. The public host declarations
and disabled stubs are unchanged. Source checks and open verification
are recorded in commit `aac0c56`.

Rust now owns D3D12 FidelityFX and XeSS super-resolution provider
sessions, including SDK module lifetime, quality planning and frame
dispatch. The typed bridges keep SDK calls, validation and the pinned
FSR4 compatibility behavior. A successful SDK result without a usable
context or XeSS fallback mask now refuses initialization. The public
provider entries and disabled behavior are unchanged.

Rust now owns D3D11 frame-tap hook state, substitutions, draw
classification and retained references. Plan replacement and uninstall
drain admitted hooks before releasing their state. Control requests
from a callback refuse if they would wait for another thread, leaving
the active plan and installation intact. The tap uses the shared Rust
texture-role classifier. Native detours keep the published C signatures.
[Frame-tap source review and open validation](research/rust-frame-tap-cutover.md)

D3D12 and Vulkan readbacks now use one Rust implementation for capture
names, aliases, preview views and scale. The existing names, description
labels and motion units are preserved. Vulkan rejects overflowing
capture sizes before allocating or recording a GPU copy and owns the
failure notification for any ticket it reserved. The public capture
headers are unchanged.
Vulkan SR builds its capture metadata in the same Rust owner as the
public SR metadata entry. The typed bridge binds the capture platform.

Rust now owns native D3D12 DLSS SR device registration, context
lifetime and frame ordering for owned, shared and engine-borrowed
Streamline handles. Rust frees viewport resources only for a context
that reached SDK option setup. The typed bridge keeps the camera,
exposure and tag values. Context and registration handle allocation
failures return the existing initialization error. The public and
Streamline host declarations are unchanged.

Rust now owns the Reflex diagnostic listener, source history, log
budget and sync snapshot. Native NVAPI detours still forward each
original call before recording it and keep the existing log format
and hook installation. Listener callbacks run outside the audit
state lock, so reentrant report or stop calls do not deadlock the
listener. The diagnostics-disabled path is unchanged.

Rust now owns the D3D12 FidelityFX and XeSS frame-generation sessions,
including shared provider status, identity and capture retirement.
FidelityFX retries a refused resize recreation and drains the final
SDK readbacks. XeSS rolls back partial XeLL settings, checks the
requested fence completion and retains contexts after refused teardown.
Vendor calls and disabled stubs stay native. The public provider ABI
is unchanged. Commit `4c661b3` records the source checks and open tests.

Rust now selects Vulkan DLSS SR's capture resources and roles, retains
the planned dimensions, and checks camera matrices before passing
them to Streamline. Output capture still precedes sharpening and
publication. The native bridge keeps Vulkan recording and the SDK
matrix conversion. The public SR ABI is unchanged.

Rust now owns D3D11 state snapshots, their COM leases and the full,
independent, depth and draw restore paths. The native bridge keeps the
typed context calls. The public and private save/restore signatures
are unchanged. Source checks covered the shared layouts and binding
order. The standalone Anomaly link still needs build verification
with its Rust archive and the shared Graphics Rust archive.

Rust now owns shared D3D11 texture dump readback, preview encoding,
statistics and publication. The native bridge retains WIC, file and
D3D11 calls. Input capture and Anomaly staging use the same pixel-stride
table. The four dump C signatures and the header are unchanged, and
the finite ranges for the two preview channels are tracked separately.
Source checks and the remaining verification are recorded in commit
`1f3bb2e`.

Rust now owns the legacy Streamline SR session, including its owned and
borrowed host routes, device leases, viewport option cache and the
token, constants, options, tags and evaluate order. The native bridge
keeps SDK types, entry points and signature verification. Replacing a
registered D3D11 device releases the previous lease, and diagnostics
no longer read stale driver fields when requirements are unavailable.
The public declarations and SDK-disabled stubs are unchanged.

Rust now owns D3D11 constant-buffer readback admission, the staging
lease and the caller's byte copy. The native bridge checks the
resource type before reading its descriptor, and the Rust path
checks both device identities. Deferred contexts report map failure
before a copy can enter their command lists. The public C signature
and result values are unchanged.

Rust now owns the D3D11 depth rescale transaction and the depth replay
frame. The port keeps the original shader and native draw calls,
including the order of depth and stencil writes. A replay with a
missing depth resource or a refused state snapshot leaves the context
untouched. The depth scale and replay signatures are unchanged, and
the replay header documents the existing diagnostic codes. Source
checks and the remaining verification are recorded in commit `02c26ac`.

Rust now owns D3D11 colour transport and fidelity, including resource
leases and dispatch order, using the shared view cache and context
restore helper. A refused resize leaves the last encoded target
intact, and leased views survive further cache lookups. The two C
headers and both HLSL programs match the previous implementation.
See the source checks in commit `50f4378`.

Rust now owns the D3D11 UI layer ring and identity registry. The layer
keeps its sharing retry, format choices and clear order. Forgetting a
recycled identity preserves the recency of the surviving entries, so
the next insertion evicts the oldest one. The 17 C signatures are
unchanged, and the header comments clarify the borrow lifetimes.
See commit `164885d` for the source checks and the unrun regression.

The shared texture-role classifier is now a single Rust C entry point.
It still evaluates the caller's rules in order and rejects short
records before writing a verdict. The render-size fallback uses
widened aspect arithmetic so large extents cannot overflow. D3D11
regions, D3D12 SR and the classifier share the same DXGI depth format
values. The frame tap and the standalone fixture retain their C calls.

Rust now owns the 32-slot D3D11 constant-buffer twin cache. It preserves
write recency and rejects stale matches against the live buffer's
descriptor. The upload runs without the cache lock and publishes its
pairing only after a successful write. Forgetting the original during
an upload prevents publication. The native bridge checks the context
device before mapping the buffer. All five C signatures are unchanged.

Rust now owns D3D11 scene promotion, including stand-in texture leases,
rebuild admission, substitution order, gate selection and fallback.
The same fullscreen pass seeds scene colour on the recombine route.
The native bridge creates typed views and issues the direct copy when
the stand-ins have the same format. Promotion refuses foreign source
textures during allocation, and the direct copy requires an immediate
context on the promotion device. The public C headers are unchanged.
Source checks covered the ABI bindings and both substitution routes.

Rust now owns the shared D3D11 fullscreen pass and Present blit through
one handle. Foreign and deferred contexts are refused, and automatic
viewport sizing uses the RTV's mip. The native draw takes an independent
full context snapshot to restore UAVs, shader linkage and predication
even inside another graphics scope. This replaces the narrower snapshot
for this pass. Source review confirmed the shader and public C headers
are unchanged. The nested-scope regression and device validation
remain open.

Rust now owns the D3D11 motion resolver's outputs, parameter admission
and source views. Decode and resolve share view caching, legacy
parameter reads and the native context-save and dispatch sequence.
Resolve keeps each selected view alive through dispatch even if a
later lookup evicts its cache entry. The optional depth layer still
falls back to scene depth, and older records retain zero jitter.
Source review confirmed both shaders and public C headers are
unchanged. Build, regression and device validation remain open.

Rust now owns the shared D3D11 motion decoder's resources, parameter
admission, constant packing and two-entry source view cache. It reads
only the legacy prefix when the caller supplies a short parameter
record, and a failed view creation leaves the selected cache entry
intact. The native dispatch keeps the full context save and restore.
Source review confirmed the decode HLSL and public C headers are
unchanged. Build, regression and device validation remain open.

Rust now owns the D3D11 overlay renderer, including pipeline leases,
atlas replacement, geometry buffers and draw admission. D3D11 and D3D12
share atlas bounds and actual index checks. The D3D11 bridge rejects
foreign and deferred contexts, preserves the game's predication and
leaves render targets bound. Source review confirmed the HLSL is
unchanged. Public C headers are unchanged. Build, regression and device
validation remain open.

Rust now decides which overlay key and button events reach the game
and owns the input counters. System keys and releases the game is owed
still forward. Focus loss also clears the toggle latch, so a release
that happened in another window cannot suppress the next press. The
diagnostic pointer coordinates come from one render sample. The Win32
bridge still decodes messages and chooses the native return value.
Public C records are unchanged. Build and regression test execution
remain open.

Rust now owns overlay cursor capture policy and its remembered clip,
frozen position, logical ShowCursor count and suppression counters.
The native bridge supplies typed cursor reads and effects. Closing the
panel balances the ShowCursor calls made on open and restores the
recorded clip for a live window. A refused position read leaves no
frozen position. Public C headers are unchanged. Build and runtime
validation remain open.

Rust now owns installation, forwarding and retirement for the seven
overlay cursor detours. The cursor switches to the native entry after
disable, and any caller still using the trampoline delays removal.
A failed removal remains tracked for retry after the window detaches.
The C++ bridge keeps typed User32 and MinHook calls. Public C signatures
are unchanged. Build and regression test execution remain open.

Rust now owns shared overlay window attachment, the original window
procedure, the attachment logger and the game's held input state.
Attach publishes the forwarder before the subclass can receive messages
and restores the previous bookkeeping if the swap fails. Detach keeps
the original available to an in-flight message and clears the logger
after cursor cleanup. C++ supplies the window procedure and cursor
adapters. Public overlay input headers are unchanged. Build and runtime
validation remain open.

Rust now accumulates overlay pointer positions, mouse buttons, wheel
deltas and touch gestures. It scales client coordinates to the
presented image, preserves short taps through the next render sample,
and clears unfinished contacts on capture or focus loss. The native
bridge decodes Win32 records and samples the cursor outside the input
lock. Public overlay input records are unchanged. Build and regression
test execution remain open.

Rust now owns shared overlay visibility, the Insert key latch, focus
edges and the 250 ms debounce. Visibility requests share one decision
and cursor transitions run in order, including requests made during a
callback. The bridge reports a refused subclass restore and retires
cursor hooks when the window has already closed. The public overlay
input ABI is unchanged. Build and regression test execution remain
open.

Rust now copies the D3D11 observer options, dispatches creation and
Present callbacks, and fills the public status record. Creation callbacks
stay serialized. Each Present uses one registration for its begin and
completion, and retirement clears that registration before hook removal.
The C++ adapters translate COM arguments and contain callback exceptions.
The Rust options and status layouts are generated from the unchanged
public header. Build and runtime validation remain open.

Rust now sequences D3D11 observer dumps, names outputs, writes constant
buffers and owns the temporary motion decoder. The native layer reads
buffers, encodes textures and runs the typed D3D11 decode operations.
Constant bytes are counted only after a successful file write. The
existing suffixes and motion parameters are preserved, and the observer
passes the full UTF-8 prefix to the native encoders. Public C headers
are unchanged. Build and runtime validation remain open.

Rust now owns D3D11 observer hook admission, original targets,
activation, rollback and retirement. A refused required Present hook
rolls back both creation hooks. Slot restoration checks that the
observer still owns the slot. A failed restoration retains the original
target for retry, with observer callbacks inactive. Native leaves still
prepare the DXGI trampolines and make the MinHook and memory-protection
calls. Public C headers are unchanged. Build and regression test
execution remain open.

Rust now owns the D3D11 observer's retained textures, latest constant
buffers, size histogram, device/context pair and queued dump requests.
Dump writers receive a retained snapshot for the duration of the
presenting-thread call. A late context for a replaced device stays with
its original snapshot and cannot enter the new device's cache. The
presenting device probe retries after refusal, and uninstall clears
unserved dump requests. The size histogram uses its existing CSV layout
and reports write failures on UTF-8 paths. Public C headers and dump
names are unchanged. Build and regression test execution remain open.

Rust now owns D3D11 observer Present sequencing, recursion, in-flight
counts and paired output dimensions. Application completions must
match an active call before they can change frame or uninstall state.
Successful uninstall clears the cached size so the next attachment can
identify the presenting device. Native leaves still select and invoke
the COM trampoline. C++ exceptions from those calls are contained at
the bridge. The observer C ABI is unchanged. Build and regression test
execution remain open.

The D3D11 observer now owns its temporary probe window and COM
references in Rust. Failed hardware creation releases any partial
references before WARP starts. The typed Win32 and D3D11 calls stay
in native callbacks. The updated fixture remains unrun.
[Observer startup probe](rust-migration.md#d3d11-observer-startup-probe)

The D3D12 observer now admits factory, device, queue and swapchain
attachments in Rust. It leases the queried interfaces until
publication and selects Present from the same accepted window.
The typed COM calls and original detours remain native.
[D3D12 observer attachment](rust-migration.md#d3d12-observer-attachment)

Rust now owns D3D12 diagnostics dispatch, validation message budgets,
stack formatting and the one-time device-removal report. The logger
and its user context are published together and copied before calling
the sink. Counters saturate once their budget is exhausted. DRED
startup, removed-device lifetime and the bounded breadcrumb walk
now also run in Rust. SDK leaves provide typed D3D12 reads and
activation calls. Public declarations match in source. Build,
fixture and device-removal verification remain open.
[D3D12 device-removal diagnostics](rust-migration.md#d3d12-device-removal-diagnostics)

The D3D12 observer now keeps its process-lifetime owner, hook slots,
trampolines, forwarding depth and submission serials in Rust. The
trampoline is published before the native hook can dispatch. Native COM
trampolines and diagnostic callbacks remain in Graphics, which links
the Rust owner for standalone and runtime callers. The observer C ABI
is unchanged. Build and runtime validation remain open.

The FSR3 D3D12 capture drain now snapshots pending readbacks in Rust
before waiting for the SDK's submitted Presents and the recording queue.
Only that batch can publish when the wait completes. A late recording
stays pending, and a device-removed fence cannot authorize a readback.
The synthetic contract was extended for both cases. The public capture
ABI is unchanged. Build and test execution remain open.

The public D3D12 frame dump API now returns the Rust readback owner
directly. Rust checks the frame ABI, admits requests, polls completion,
and refuses destruction while copies remain. Creation now reports
allocation failure instead of returning a reader without an owner.
The DLL and linked consumers use the same public C signatures and
typed D3D12 callbacks. Build and runtime validation remain open.

The Direct3D FG CPU path now uses Rust for its Win32 completion event,
window ownership checks, input age and clocks, and bounded render-thread
sampling. The wait still pumps sent messages when requested. The sampler
resumes the render thread before module lookup or formatting, and module
names are written as UTF-8. The production callbacks keep the existing
private table layout and the public C ABI. Build and runtime validation
remain open.

Anomaly's cold entry and native observer now share Rust executable
identification. Both check the profiled filename before streaming the
file into Windows SHA-256 with full file sharing. The observer still
rechecks the executable before it admits the loaded image and verifies
the native hook bytes. The executable and shader paths share lowercase
digest formatting. Public C ABI signatures are unchanged. Build and
runtime validation remain open.

The D3D11 and native D3D12 overlay hosts share one Rust panel loader.
It resolves the override or adjacent DLL, binds the five exports, and
keeps one process-lifetime reference per published module. An aborted
D3D12 setup releases its temporary reference after destroying the panel.
Both hosts now accept the same path lengths and refuse an oversized
override. The public overlay ABI is unchanged. Build and runtime
validation remain open.

Rust now owns Anomaly's startup and FG overlay controls and the INI path
used by their setters and the SR panel. The ten controls keep their
order, ranges, defaults and restart behavior. FG still reloads capture
settings only after a successful save. Bootstrap, SR, FG, motion and
overlay share the same Win32 INI calls for the Anomaly section. The
public plugin ABI is unchanged. This increment was reviewed at source
level. Build and runtime validation remain open.

The Anomaly motion loader now holds the Rust pack directly. Rust
assembles patch requests and keeps the include buffer alive until
the Runtime copies it. An opened engine include is closed even when
allocation or copying fails. The native bridge verifies the engine
include table, calls its guarded methods and supplies D3D reflection.
The public shader patch ABI is unchanged. Build and runtime
validation remain open.

Rust now reads and admits Anomaly's motion shader pack, validates the
Runtime shader API and owns the admitted manifest and session together.
A refused pack closes any session the Runtime returned. Anomaly and
the Runtime share generated definitions for the public shader C ABI.
The native bridge still supplies the engine's guarded include and
compiler calls. Build and runtime validation remain open.

Anomaly now uses one Rust path to borrow the carrier's Runtime module
and resolve its exports. Bootstrap, SR, FG, overlay, capture and shader
patching use that path. Callers still check the API table they request
and report missing services at their own boundary. The shared lookup
keeps by-name binding and takes no additional DLL reference. Build and
runtime validation remain open.

Anomaly's native SR host now retains a single opaque Rust draw pass.
Rust selects the round-trip or upscale pass and reuses it for the
same device and stage. A replacement is published after creation
succeeds. A refused creation leaves the frame on its existing fallback.
The production host and the native fixtures share the draw-view
builders, including the temporal input order. Build and runtime
validation remain open.

Rust now builds and owns Anomaly's complete SR twin batch. It reuses
the existing color, depth and scratch allocators and publishes borrowed
views only after every required allocation succeeds. On refusal, the
partially built batch drops before the frame falls back to stock. Rust
also owns the view release order before the native reference-count
diagnostic. The batch and single-target facade share the conversion
from a target view to engine fields. Build and runtime validation
remain open.

Rust now writes Anomaly's SR jitter burst files. The native bridge maps
the staging textures and reports their DXGI row layout. Rust validates
the pitch and address range, writes the same packed rows and releases
each map on exit. Failed writes remove their partial files, and the
summary reports a write failure. The staging copy checks device
ownership and restores the original D3D11 predication state after the
copy. The private bindings were regenerated. Build and runtime
validation remain open.

The Anomaly input frame hook and SR position binder share one Rust
vtable slot owner. It checks image bounds, publishes the original
before installing the detour, and uses the native guarded
compare-and-swap. If page protection cannot be restored, the slot
owner keeps the original protection for retry and leaves the detour
inactive. The SR teardown now reports a refused slot restoration
instead of discarding that result. The input hook also skips frame
reporting when the guarded counter read fails. Build and runtime
validation remain open.

XeSS Vulkan SR now uses the Rust SDK module owner shared with
FidelityFX. Rust resolves the absolute payload path, owns the loaded
module and pins it after the native entry table is validated. The pin
preserves SDK-owned parts of the Vulkan device creation chain if the
host retires the session before destroying the device. The native
bridge only resolves typed entries and makes SDK calls. The public C
headers have no changes. Build and runtime validation remain open.

FidelityFX Vulkan super resolution and frame generation now share one
Rust SDK loader. It takes an absolute UTF-8 runtime directory and keeps
the module alive through context retirement and native leaf release.
The native bridge uses one typed definition of the common SDK exports.
If SR creation rollback fails, the current ABI has no owner to return.
That path retains the native leaf, device dispatch and module for the
process lifetime rather than unloading live SDK code. Build and runtime
validation remain open.

The FidelityFX Vulkan owner now registers and unregisters WSI in Rust
through a typed native call. It publishes the route after registration
and keeps the SDK module and admission if unregistration fails. The
special case where the corrected SDK reports another device's table
can retire because this owner's hooks are already gone. Build and
runtime validation remain open.
[WSI ownership notes](research/rust-vulkan-ffx-wsi-owner.md#explicit-registration-and-retirement)

The FidelityFX Vulkan owner now records swapchain and effect context
ownership independently of the SDK result. When the SDK returns a
context alongside an error, Rust uses the existing checked teardown
path and retains the owner if destruction refuses. A replacement
chain without a context stays quarantined because the SDK destructor
requires that context. Creation checks the setup ABI and Vulkan
structure tag before reading the swapchain descriptor. The pinned
provider source publishes contexts on success. The partial-result
handling covers defensive SDK returns and has no build or runtime
verification recorded.

Rust now prepares native D3D12 overlay draw calls from the shared panel
layout. It resolves atlas texture IDs, clamps scissors to the
backbuffer, and verifies both the index range and the vertices
referenced by each draw. The old range check covered the index slice
but could accept an index whose base vertex put it outside the vertex
buffer. Native code records the validated D3D12 commands without
keeping a copy of the borrowed layout. The private bindings were
regenerated. Build and device validation remain open.

Rust now caches the native D3D12 overlay pipeline by backbuffer format
and owns each slot's vertex and index upload buffers. A submitted slot
keeps its pipeline reference through fence retirement. Rust checks
geometry byte counts before asking the native leaf to allocate or map
buffers. Texture uploads and geometry uploads use the same native
allocation call. The private bindings were regenerated. Build and
device validation remain open.

Native D3D12 overlay startup now rolls back a partially created
renderer and releases its UI instance and unpublished module. Rust
owns the renderer after successful startup. A fault disables drawing.
The owner releases resources once the submitted work has retired,
any armed wait has signalled and the input hook can be removed. If
execution succeeded and the first fence signal failed, a later Present
on the same device and queue can retry the signal. It never resubmits
the command list. The private bindings were regenerated. Build and
device validation remain open.

The native D3D12 overlay now keeps its completion fence and wait event
in Rust. Rust orders command execution, signalling and slot publication.
The slot becomes reusable only after the fence proves completion. If a
wait fails, the existing disabled state retains the event because the
GPU can still signal it. The overlay also checks the capacity of a
non-null intent output before writing it. The private bindings were
regenerated. Build and device validation remain open.

The Anomaly temporal driver builds its Runtime setup and motion decode
parameters in Rust. It takes the stage's existing configuration directly
and copies each vendor directory once into the driver that owns it. The
adapter capability masks, engine metadata and motion validity constants
come from the same private header in both languages. The native facade
still provides the exception-guarded log sink and the versioned Runtime
lookup. The private bindings were regenerated. Build and runtime
validation remain open.

The Anomaly SR stage now retains the temporal target set directly in
Rust. It reuses the existing D3D11 target allocator and passes borrowed
views to the native draw calls and existing pass inputs to the Runtime
driver. It releases old targets when their device, dimensions or
format change or when the engine twins retire. A refused allocation
still draws the bilinear fallback. The native host retains only the
seam's borrowed draw views during the call.
The private bindings were regenerated. Build and runtime validation
remain open.

The Anomaly SR stage retains the motion shader view in Rust. The cache
reuses a view for the same device and texture, and releases it when the
device or twin resources change or the stage shuts down. The previous
native cache recorded the texture even when view creation failed, so it
never retried that texture. The new stage caches only successful views
and uses the existing empty motion input on a refusal. The D3D11 view
creation helper is shared with the other adapter passes. The private
bindings were regenerated. Build and runtime validation remain open.

The Anomaly SR stage now owns its four D3D11 timestamp slots and their
device and query references in Rust. It records the existing world and
seam boundaries, polls without flushing, and keeps abandoned intervals
pending until the disjoint query retires. An abandoned interval counts
as skipped and contributes no timing sample. Native code only issues,
creates and polls the typed queries. The private bindings were
regenerated, and build and runtime validation remain open.

Anomaly SR, motion, geometry and native observation share Rust checks
for fixed and extended profile signatures. The SR first-person check
also verifies the relative call targets and their ordering in Rust.
Native profile tables supply the existing expected bytes and the common
exception-guarded copy supplies the mapped bytes. The Standard and AVX
baseline fixtures reached hook installation with matching call bytes
outside the declared image. The new guard rejects those ranges before
reading, and the fixture now requires a guard refusal at that call site.
The replacement has no build, fixture or in-game verification recorded.

Rust now owns the Vulkan DLSS-G proxy handle from creation through
retirement, including descriptor admission, transfer-usage selection,
image count and rollback after capture registration refusal. The
previous synthetic path accepted a successful enumeration with zero
images. The new fixture verifies refusal and exactly one proxy destroy.
MSVC Release passed nine Vulkan CPU and ABI contracts, and the four
rebuilt DLL and embedded consumers kept their baseline exports. The
Streamline-disabled DLL passed the export contract and direct stub
probe. All 413 locked Rust tests, formatting and Clippy passed.
Live Vulkan DLSS-G activation and rendering remain open.
[Proxy creation evidence](research/rust-vulkan-dlss-fg-cutover.md#proxy-creation-ownership)

The shared capture owner now writes packed CPU readback rows and their raw
JSON sidecars in Rust. The native leaf still encodes the PNG preview.
The C ABI fixture reproduced a stray sidecar on the previous writer by
blocking the raw path with a directory. The new writer creates the
sidecar after the raw write succeeds and removes its raw file if the
sidecar write fails. Existing files and directories are preserved, and
the PNG preview is still attempted. The success case matched the
existing byte layout and JSON exactly, including the source row pitch.

MSVC Release passed six CPU and ABI contracts, including the capture
metadata fixture and both Vulkan readback CPU fixtures. Runtime, AC7,
Loader and Minecraft matched every baseline export name and ordinal.
All 413 locked Rust tests, formatting and Clippy passed on Rust 1.94.1.
The D3D12 readback fixture linked. Device and game behavior for this
change remain unverified.

The shared D3D11 SR pipeline now admits and publishes surfaces in Rust.
Resource shutdown calls the existing Rust owners directly in the same
release order. Shutdown closes surface admission before clearing the
game-facing configuration. The focused Rust fixture reproduced stale
surface publication when admission closed at the end of teardown and
passed with the corrected ordering.

All 413 locked workspace Rust tests, formatting and Clippy passed on
Rust 1.94.1. MSVC Release passed six focused CPU and ABI contracts. A
direct Runtime DLL call verified idle surface refusal and repeated
resource release. Runtime, AC7, Loader and Minecraft matched every
baseline export name and ordinal. Device and game behavior for this
change remain unverified.

The integrated workspace passed all 332 locked tests, formatting, Clippy
with warnings denied and the optimized Cargo build on the pinned Rust
1.94.1 toolchain. MSVC 19.52 Release built Runtime and Anomaly with
the same Rust pin and passed nine focused CPU contracts covering C and
C++ overlay clients, frame assembly, D3D11 host, the Anomaly SR API,
Vulkan DLL exports and linked and DLL Vulkan capture. Runtime matched all
381 baseline export names and ordinals, and Anomaly matched all three
with its existing Runtime lookup. No device or game fixtures ran in
this pinned pass. Logs are in `.local/rust-1.94.1-verification/`.

The existing C++ Anomaly DLL builds against the merged runtime. Thirteen
focused MSVC contracts passed in Debug and Release, covering plugin
lifecycle, cold-start refusal, native hooks, the D3D11 host, SR and overlay
API wiring, FG settings, and Standard and AVX input hooks and events.
The SR API and D3D11 host fixtures load the Runtime DLL. The stubbed
engine and overlay fixtures verify the adapter contracts.
The read-only SR evidence verifier also passed against the installed
Anomaly 1.5.3 Standard and AVX executables. It checked 17 function guards,
6 call-site guards, 7 extended guards, 6 first-person sites and 26
render-target slots per profile. This verifies the deployed build
identities and guarded layouts before live testing.

The staged C++ adapter now has a game baseline on revision `8322179a36c7`.
Standard exercised live DLSS and FSR3, DLSS-G 2×, the overlay and
restored game input, native rendering with SR off, and normal game exit.
AVX evaluated DLSS and presented DLSS-G before and after a resize and
exited normally. Release D3D11 and D3D12 sharpening device fixtures passed.
The matched run, evidence paths and remaining acceptance gaps are in
[the C++ adapter baseline](research/rust-anomaly-cpp-baseline.md).

The Anomaly adapter now builds its own Rust static library. Rust builds
the versioned plugin API and matches exact executable names and hashes
for both DLL detection and the bootstrap and observer's native profile
selection. These callers derive their identity list from the existing
native build profiles. The C++ boundary supplies the verified native
hook callbacks. The DLL keeps the same three export names and ordinals
and resolves its single existing Runtime at startup.

The profile-selection, plugin, lifecycle, cold bootstrap, SR API and
both input-hook CPU contracts passed in MSVC Debug and Release.
All 216 locked Rust tests, formatting, Clippy, the four Runtime/carrier
export inventories and the Anomaly export comparison passed. The live
evidence above applies to the earlier C++ adapter revision.

The Anomaly SR-off FG input pass now owns the accepted device, shader
lease, output generations and dispatch constants in Rust. The native
leaf retains the byte-identical HLSL and D3D11 calls. The pass refuses
foreign-device depth and inputs whose subresource layout cannot be
copied into the private depth texture. It keeps the earlier allocation
on a refused resize and exposes outputs only after a successful pass.
The device leaf saves bindings before the copy and disables inherited
predication for the private dispatch.

Seven focused native contracts passed per configuration, including
the Standard and AVX engine hook and input-event contracts. Two Rust
regressions cover the foreign-device check and allocation retry.
Formatting, Clippy and locked workspace tests passed. The Anomaly
export names and ordinals matched the C++ baseline and the FG device
fixture built. The migrated SR-off path still needs device and game
execution.

Anomaly's engine-frame classification, scope hold, history resets and
counters now live in Rust. The native adapter reports the same render,
HUD-less, input-availability and Present boundaries. The current input
decision is available before the capture decision, and camera history
advances only after the frame feeds FG history at End. This prevents
an input refusal or unsuccessful Present from becoming the previous
camera for a later frame.

Eight focused contracts passed in Debug and Release. They cover scope
and menu hold, no input, no Present, device and size changes, and both
executable profiles. Formatting, Clippy, locked Rust tests and the
Anomaly export comparison passed. The FG fixture built in both
configurations. Live SR-off validation remains pending for this port.

The Anomaly geometry registry, retirement queue and draw classification
now run in Rust. A reused visual still receives a new generation and two
live owners still make its identity ambiguous. The Rust owner detaches
each retirement batch before calling the native history consumer. The
CPU fixtures verify that a batch is already invalidated when callbacks
run and that a retirement enqueued during drain reaches the next batch.
Native engine guards, draw scopes and copied geometry inputs keep their
existing call sites.

Eight focused native contracts passed in Debug and Release, including
the Standard and AVX geometry and SR hooks. Formatting, Clippy,
locked Rust tests and Anomaly DLL export parity passed. This cut has
no fresh device or game evidence.

Anomaly's input admission, buffered receipt routing and overlay
DirectInput gate now live in Rust. The native detour retains its guarded
engine byte checks and reports the frame before entering the polling
scope. The worker passed 12 focused CPU contracts in both build
configurations. The merge added a Rust regression for routing and nested
poll retirement. The actual detour still needs live adapter validation.
See [the input cutover audit](research/rust-anomaly-input.md).

Anomaly's overlay hook installation, trampoline registry, and callback
publication now run in Rust. DirectInput and DXGI discovery and MinHook
calls stay in the native bridge. Installation is serialized, and detours
copy their original or callback under the registry lock before calling it.
A refused enable unpublishes only its binding and disables its own batch.
Originals stay available to already-entered detours.

The pinned toolchain passed all 333 locked Rust tests, formatting and
Clippy. MSVC Release passed five focused contracts including both input
profiles, and Anomaly kept all three DLL exports and its by-name Runtime
lookup. The WARP hook fixture built. An isolated Standard game run with
the rebuilt DLL opened and closed the overlay with Insert, navigated the
panel with the mouse and turned the camera after closing it. The last SR
status recorded 5,489 evaluations with no refusals. Steady FG windows
recorded 240 vendor presents per 120 source frames. The engine destroyed
the level, shut down Direct3D and exited with code 0. The test restored the
staged inputs. Logs and hashes are in
`.local/anomaly/staging/live-58fe95a-stage5/evidence/rust-overlay-hook-registry/`.

The same Release DLLs also passed the isolated Anomaly 1.5.3 AVX run. Insert
opened and closed the overlay, the mouse switched to the sharpening tab,
and mouse input turned the game camera after the panel closed. The save
loaded at 1280×720 and resized once to 1600×900. DLSS reached 5,609
evaluations with zero refusals, and steady DLSS-G windows recorded 240
vendor presents per 120 source frames. The engine queued its scripted
quit, destroyed the level and Direct3D, and exited with code 0. All five
staged inputs matched their backups after restoration. Gameplay keyboard
consumption is still unverified. Two opening captures at 1280×720 and
two closing captures at 1600×900 are beside the logs and hashes in
`.local/anomaly/staging/live-58fe95a-stage5/evidence/rust-overlay-hook-registry-avx/`.

Anomaly's guarded native hook batches now use Rust admission, rollback,
and trampoline lifetime management. Engine hooks and the four D3D11
motion detours serialize operations on the MinHook queue. The existing
Standard and AVX byte guards and pinned adapter lifetime still apply.
The worker passed 18 CPU contracts per MSVC configuration and the
pinned Rust checks. Its Release and Debug DLLs retained the original
exports and dynamic Runtime lookup.
[Hook transaction audit](research/rust-anomaly-native-hook-set.md)

The integrated hook and palette build ran the same save on Standard and
AVX. Both resized from 1280×720 to 1600×900, opened and closed the
overlay, and turned the camera with mouse input. The moving-frame logs
record valid hardware-skinned draws and 240 DLSS-G vendor presents per
120 source frames. Final DLSS counts were 1,769 on Standard and 1,889
on AVX, with zero refusals. Both scripted quits shut down Direct3D and
returned code 0. The test restored the five staged inputs. Runtime and
adapter hashes and separate logs are in the `merged-hook-set-standard`
and `merged-hook-set-avx` directories under
`.local/anomaly/staging/live-58fe95a-stage5/evidence/`.

The overlay now activates and rolls back its hooks through the same
serialized MinHook batch as the engine hooks. The registry still owns
its trampoline lookup and callback routing. A refused apply clears
only the overlay binding for that batch and keeps its originals. The
synthetic fixture checks rollback and the retained input batch. All
371 pinned Rust tests and nine MSVC Release contracts passed. The
adapter exports and dynamic Runtime lookup match the baseline.

The first Standard run remained on the loading screen after an input
attempt and timed out. A retry with the same binaries and no input
during loading resized and exited normally, with 1,769 DLSS
evaluations and zero refusals. The AVX run opened and closed the
overlay after gameplay began, turned the camera with the mouse, and
drew all 360 visible overlay frames. It resized to 1600×900, recorded
240 vendor presents per 120 source frames, and exited normally after
2,489 DLSS evaluations with zero refusals. The failed run and both
successful run receipts are under
`.local/anomaly/staging/live-58fe95a-stage5/evidence/shared-hook-batch-*`.

Anomaly's cold entry and startup retry state now live in Rust. The
entry borrows the Runtime already loaded by the thin carrier. A partial
session refuses before changing stage setup, and retrying an unfinished
step keeps the completed options and stage registration. The merged
native leaf uses the Rust input receipt route. The worker passed the
focused startup, plugin and input contracts in both configurations, and
the startup retry regression passed on the merged branch.
See [the cold-entry cutover audit](research/rust-anomaly-bootstrap.md).

Anomaly's draw-time motion history, shader eligibility and b13 packing
now live in Rust. Geometry and motion history share one instance-key
binding. The packer refuses viewport extents whose reciprocals overflow
and writes the invalid buffer. The worker verified all four b13 layouts
with FXC, and its focused CPU contracts passed in Debug and Release.
The shader pack hash matched the staged game baseline.
See [the motion cutover audit](research/rust-anomaly-motion-history.md).

The motion flush now lends the guarded draw's world and bone data to
Rust for the call. Rust classifies the snapshot and copies its palette
only when history accepts the draw. The allocating C++ history adapter
is confined to the geometry fixture. Seven focused Release contracts
passed, including geometry hooks on Standard and AVX, and all 336
locked Rust tests passed on the pinned toolchain. The motion device
fixture built and the Anomaly DLL retained its three exports and
dynamic Runtime lookup.

Anomaly's Stage 5 temporal SR driver, retry state and published
settings now live in Rust. The native Stage still supplies its engine
frame and texture leases through the checked Runtime table. A refused
request can be retried after settings return to the applied state, and
the overlay reports the quality that produced the last accepted frame.
The adapter and runtime share bindings generated from the D3D11 SR,
pipeline and overlay C headers. See [the Stage 5 audit](research/rust-anomaly-sr-temporal.md).

The combined input, cold entry, motion, geometry and Stage 5 cuts
passed 22 focused tests in each MSVC configuration and 220 locked Rust
tests. Four licensed-source cases inside the shader-tool suite skipped.
The integrated FG and SR device fixtures built. The Anomaly DLL kept
its three export names and ordinals and the carrier-loaded Runtime
lookup. Runtime, AC7, Loader and Minecraft kept their Release export
inventories. The Anomaly shader pack matched the staged baseline hash.
Device execution and game acceptance remain open for the merged adapter.

Rust now owns Anomaly's motion-target binding and restore decisions. The
native leaf supplies guarded engine-cache reads and writes and D3D11
target calls. The old binder could leave a borrowed motion RTV in slot 2
after the engine dirty-byte write refused. The new binder updates the
dirty byte before publishing the target and retains the pending cleanup
across a refused close. The new fixture reproduced the stale slot on the
old binder and now checks refusal, retry and restoration. Nine focused
MSVC contracts passed in Debug and Release, all 220 locked Rust tests
passed, and the three Anomaly exports and Runtime lookup matched the
staged baseline. The motion and FG device fixtures built in both
configurations. Live motion-target behavior remains to be verified.

The Anomaly motion device now owns its texture and RTV, the four b13
buffer leases and the upload cache in Rust. It shares the existing
resource lease implementation with the FG input pass. Native D3D11
calls retain the invalidation replay and unchanged pixel shader. A
skipped replay now releases the state references without rebinding the
output merger or pixel shader. Two CPU tests cover a refused RTV and
retry, and cache invalidation when an upload fails. Six focused MSVC
contracts passed per configuration. All 222 locked Rust tests and the
Anomaly DLL export comparison passed. The three motion device fixtures
built in both configurations for later GPU validation.

Anomaly's native observer lifecycle, engine-fact routing and counters
now live in Rust. The native trampoline and SEH guards retain the same
Standard and AVX sites. Begin sends one guarded frame sample to both
the public observation and FG. Quiesce now closes admission and checks
the observer and input callbacks before retiring SR and motion state.
See [the observer cutover audit](research/rust-anomaly-native-observer.md).

The observer and motion cuts passed 13 combined MSVC contracts in
Debug and Release and all 224 locked Rust tests. The merged Anomaly DLL
matches the staged exports and Runtime lookup, and the motion pack hash
is unchanged. Device fixtures built. Live callback retirement and the
Standard and AVX game runs remain to be verified on the merged adapter.

Anomaly's tree and grass history now live in Rust and share frame
continuity rules with the rest of the motion history. The history keeps
two reusable frame tables per view and the prior grass item cap. The
old grass remap flagged a same-frame conflict yet still returned valid
previous rows for that draw. The Rust owner records the whole batch
first and refuses both conflicting groups in that batch. It also withholds
previous rows for a route whose current constants conflict. An already drawn
batch cannot be changed by a later conflict. The producer bindings and
b13 layout follow [the foliage evidence](research/anomaly-foliage-motion.md).

The CPU fixture reproduced the conflicting-row bug on the C++ owner
and passed its refusal, remap and view-history checks with the Rust
owner. Seven focused MSVC contracts passed in Debug and Release. All
224 locked Rust tests, formatting, Clippy and adapter DLL export
parity passed. The three motion device fixtures built. Current-device
and foliage gameplay evidence remain open for the port.

Anomaly's FG stage now runs in Rust. It reports the existing guarded
input, render, HUD-less and Present facts by source ID, uses the shared
resource lease and sends requests through the same Runtime tables.
The source advances camera history only after both inputs and the
paired HUD-less scene passes are accepted. A fake-table regression
checks that ready SR inputs and a successful Present cannot advance
history when there is no backbuffer to hand off. See [the FG stage audit](research/rust-anomaly-fg-stage.md).

The FG merge passed 15 focused MSVC contracts in Debug and Release and
227 locked Rust tests, formatting and Clippy. The FG and temporal SR
device fixtures built, and the adapter DLL kept all three exports and
the carrier-loaded Runtime lookup. Live FG and recovery checks on this
merged adapter remain open.

Anomaly's overlay service now binds and validates the carrier-loaded
Runtime table in Rust. The Rust service owns the adapter options,
configuration and settings callbacks, chain admission and panel
diagnostics. Native COM, stage links and guarded hook calls keep the
same boundaries. A refused hook install now stays refused when the
cold entry retries it. See [the overlay service audit](research/rust-anomaly-overlay-service.md).

Anomaly now connects its Rust overlay directly to the Rust SR and FG
stages. The C++ publication tables and duplicate setting records were
removed. All 382 pinned Rust tests and ten focused MSVC Release
contracts passed. The adapter's three exports and dynamic Runtime
lookup match the staged baseline. The foreground overlay and game
checks remain open for this change.
[Direct stage links](research/rust-anomaly-overlay-service.md#direct-stage-links)

Rust also owns the admitted Anomaly motion manifest, shared by the
text and Runtime pack paths. The native map and per-shader lookup
callback were removed. The merged build passed 385 pinned Rust tests
and 14 focused MSVC contracts. The adapter's three exports, dynamic
Runtime lookup and the pack SHA-256 matched staging. The isolated
AVX run opened 73 allowlisted roots, reported 79 cached replacements,
resized with DLSS SR and shut down cleanly. Standard opened the same
pack in the foreground, resumed DLSS-G after resize and reloaded the
pack through the overlay. DLSS-G stayed active after reload and overlay
close. The same manifest build passed the AVX foreground resize,
Quality to Balanced and back, and overlay reload. All three foreground
runs exited through the engine. Held-key input still needs verification.
[Manifest and AVX evidence](research/rust-anomaly-shader-manifest.md#merged-adapter-verification)

Anomaly now keeps the SR configuration, package-relative vendor
paths and live options in the Rust stage. The native settings mirror
and its refresh and observer callbacks were removed. The merged
adapter passed 401 Rust tests and 16 focused MSVC Release contracts.
Standard verified the default DLSS and XeSS paths. The AVX candidate
opt-out run timed out before the actor update. The merged foreground
retest reached gameplay, resized and exited cleanly. Its native
backbuffer showed the game and overlay with SR and FG unavailable.
[SR configuration evidence](research/rust-anomaly-sr-configuration.md#coordinator-integration-and-game-checks)

The shared Runtime shader patch service now owns pack validation,
transformations, claims, original proof, reflection, DXBC, cache
publication and interception state in Rust. Its public table and
cache key are preserved. The combined build passed 413 Rust tests,
eight focused MSVC Release contracts, the shader CPU fixture and
ten package tests. The Runtime reused the C++ cache unchanged and
all checked export inventories matched. In a cold-cache Standard
run it compiled 79 replacements, resized with DLSS-G active and
exited cleanly. All 165 original cache files were restored and verified.
[AVX and build variant evidence](research/rust-shader-patch.md#avx-cold-cache-and-build-variants)
The AVX cold run also compiled 79 replacements. Its generated
cache matched the Standard filenames and SHA-256 hashes. Debug
and Vulkan Streamline-disabled Release each passed seven native
contracts, the shader CPU fixture and the previous-cache DLL probe.
[Integrated shader patch evidence](research/rust-shader-patch.md#integrated-runtime-and-cold-cache-standard-run)

The overlay and FG merge passed 17 focused MSVC contracts in Debug
and Release, including the hidden-window overlay API contract and the
input gate. All 233 locked Rust tests, formatting and Clippy passed.
Bindings regenerated byte for byte. The Anomaly exports and motion
pack matched the staged baseline. The overlay hooks and FG device
fixtures built. Live panel input, settings changes and presentation
remain open on the merged adapter.

Rust now tracks Anomaly's motion window identity and the pending action
for the next immediate-context draw. A repeated scene window keeps its
target for the same engine frame, view and reset generation. A device
change invalidates the published target. Scene boundaries and a later
flush retire any abandoned action before it can affect a different
pass. The four draw detours share one native replay and restore path.
SR-off FG now admits object motion only for the same engine frame, main
view and extent. When the target is stale it uses the existing camera
motion fallback. Removing the frame check failed the new regression.

The motion window and FG changes passed 11 native CPU contracts in
both MSVC configurations and 236 locked Rust tests, formatting and
Clippy. The adapter kept the same three export names and ordinals and
dynamic Runtime lookup. The FG device fixture built. Live draw
retirement and SR-off FG still need device and gameplay validation.

Rust now owns Anomaly's shader caller-frame decoding, manifest admission,
cache identity and variant compatibility. The engine still creates the
stock shader first and the Runtime still proves the original bytecode
before serving a production variant. The previous tessellation check
accepted a duplicate interpolant in place of a distinct required field.
The Rust check requires each field once. See [the shader loader audit](research/rust-anomaly-shader-loader.md).

The shader loader merge passed 11 CPU contracts in Debug and Release,
including the motion and shader-tool suites. All 236 locked Rust tests,
formatting and Clippy passed. Export names and ordinals, dynamic Runtime
lookup and the rebuilt motion pack matched the staged baseline. The
motion and FG device fixtures built. Live and reload validation are
tracked separately.

The isolated Standard game checkpoint at `9b304d1` loaded an autosave
at 1280×720 with SR off. The adapter reported 21,930 level frames
with accepted FG inputs and no `no_inputs` refusals. DLSS-G remained
inactive in the recorded status. Windows refused foreground input and
the isolated process had to be stopped after a window-close request.
Generated frames, live overlay and input checks, and normal teardown
remain open. See [the Standard checkpoint](research/rust-anomaly-fg-stage.md#standard-game-checkpoint).

Anomaly's SR swap owner now runs in Rust. It owns view and twin
admission, the engine field transaction and restore, jitter and mip
bias, and the sun-cascade suspension token. The previous rollback
left render-size dimensions and viewport on a refused sun cascade.
The Rust rollback restores output size. The merged owner keeps its
published view separate so the native seam can read the twin
generation during the call. See [the SR swap audit](research/rust-anomaly-sr-swap.md).

The swap merge passed 16 focused MSVC contracts in Debug and Release
and 248 locked Rust tests, formatting and Clippy. The private binding
regenerated byte for byte, and all three adapter exports and Runtime
lookup matched the baseline. SR and FG device fixtures built. Live
fallback and swap restoration remain open on the merged adapter.

The Anomaly D3D11 SR owner now validates the twin and temporal target
allocations, holds the COM leases and sequences the round-trip,
prepare and reinsert passes in Rust. The empty motion texture upload
checks row pitch before allocation. The C++ facade uses the swap
owner's target-field record, and D3D11 draw leaves retain the original
state scope and shader bytes. See [the D3D11 SR audit](research/rust-anomaly-sr-d3d11.md).

The combined D3D11 and swap ports passed 17 focused MSVC contracts in
Debug and Release, and all 256 locked Rust tests, formatting and Clippy.
The adapter's three exports and Runtime lookup matched the baseline.
Both embedded SR shaders matched the source and staged binary byte for
byte. SR and FG device fixtures built. The new device path still needs
execution and live SR acceptance.

The Anomaly overlay now shows the FG scope hold and saves changes before
applying them to the Rust FG policy. Changes take effect on the next
source frame and preserve the reset after a scope hold. The two options
that select FG startup diagnostics and development runtimes are marked
as requiring a restart. The startup log reports SR and FG choices
independently, including the FG-only setup seen in the Standard checkpoint.
Nine focused MSVC contracts passed in both configurations. All 257 locked
Rust tests, Clippy, formatting and adapter export parity passed. Live
overlay changes and generated presentation remain open.

The refreshed Release build at `7a21a20` preserved the exact export
names and ordinals for Runtime (381), AC7 (256), Loader (273) and
Minecraft (267). Nine focused contracts passed, including the DLL and
embedded SR policy and callback-host forms, the D3D11 Present C11 and
C++ consumers, and the Vulkan and shared shim export checks.

Anomaly's SR stage driver now runs in Rust and uses the existing Rust
swap, D3D11 and temporal owners. The driver owns stage lifecycle and
admission, the H1 and seam order, capture and burst bookkeeping,
published camera and jitter, options and status. Native leaves retain
the guarded reads, trampolines and D3D11 and engine calls.
See [the SR stage audit](research/rust-anomaly-sr-stage.md).

The SR stage merge passed 26 focused MSVC 19.52 contracts in Debug and
Release, 268 locked Rust tests, formatting and Clippy. The two SR fixture
warnings reproduced on MSVC 19.44 are fixed, and both fixtures built
and ran there. The adapter DLL exports, dynamic Runtime lookup and
rebuilt motion pack matched the baseline. The device fixtures built.
The Rust driver still needs device and live Standard and AVX evidence.

The Anomaly overlay now changes FG input capture enablement, the start
frame, count and interval while the game runs. It saves the options
before applying them. The Rust schedule keeps the accepted source ID
and consumes a slot only for a successful dump. Re-enabling capture
starts a fresh run. An older in-flight dump cannot spend that run's
budget. The output directory is read at startup.

The scheduler regression, nine focused MSVC contracts in both
configurations, and all 269 locked Rust tests passed. Formatting,
Clippy and the exact adapter export comparison passed. The FG device
fixture built. Captured images and the live panel controls need game
verification.

Anomaly's SR render sizing, mip-bias arithmetic, jitter sequence and
publication, and burst settling rules now run directly in Rust. The
native fixtures call those implementations through the private ABI.
The old phase-count conversion failed the extreme extent regression.
The Rust calculation caps the floating-point count before conversion.
Thirteen focused MSVC contracts passed in both configurations, all
269 locked Rust tests passed, and the Anomaly DLL exports matched.
The four native arithmetic fixtures also passed MSVC 19.44 syntax
checking with warnings denied.

The Runtime and panel now share capacity for 32 adapter-defined
options. The incoming Anomaly motion port brings the combined setup
to 25 options. The old Runtime rejected that count and the old panel
imported only 24. The new contracts cover the 25th entry and the upper
bound. Three overlay DLL contracts passed in each MSVC configuration,
all 270 locked Rust tests passed, and the rebuilt panel and all four
Release export inventories passed verification. Use the Runtime and
panel from this build when exercising the full Anomaly option list.

Anomaly's geometry and motion stages now own the render scopes, draw
facts, shader variants and linkage, draw retirement and capture timing
in Rust. Native leaves retain the verified engine guards, trampolines
and graphics calls. The geometry regressions cover a flush in a
different phase, nested first-person restoration, and unreadable
nested view identity on both profiles. Motion invalidates linkage when
a variant changes and ties capture completion to the admitted main-view
frame, context and resources. See [the geometry audit](research/rust-anomaly-native-geometry.md) and [the motion stage audit](research/rust-anomaly-motion-stage.md).

The geometry and motion merge passed 35 focused MSVC contracts in
both configurations and 273 locked Rust tests. Four licensed shader
cases skipped inside the shader-tool suite. The adapter exports,
dynamic Runtime lookup and rebuilt motion pack match the baseline.
Read-only geometry, SR and FG evidence checks passed against both
copied executables. The motion, SR and FG device fixtures built.
Moving-game imagery, active providers and live scope transitions
remain open.

The shared D3D11 presentation bridge now owns admission, backend
selection and switching, frame slots, Present sequencing, resize,
status and retirement in Rust. The native facade retains the DXGI
object and calls the graphics and SDK leaves. Lazy creation publishes
the provider before a fullscreen refusal can leave the bridge without
an owner. Early Present errors now complete the notification and fault
the facade. The merged queue and command-list queries hold the read
lease through the native lookup and refuse during retirement.
See [the D3D11 presentation audit](research/rust-d3d11-present-cutover.md).

D02 moved native shared texture and fence calls, public table validation,
and owned-chain dispatch admission and reference handling to Rust. Setup
rejects nonfinite unit scales, lazy creation retries after a host-load
refusal, provider publication uses one snapshot, and a fenced nonblocking
Present may be retried. The C ABI clients and device fixtures remain linked.
The source and binding review is recorded in the
[D02 follow-up](research/rust-d3d11-present-cutover.md#d02-follow-up).
Build, fixture and device verification are pending.

The scoped owner check now queries the caller's live COM receiver for
its canonical identity and compares that address with the published
facade. It rechecks publication after the query. The synthetic scoped
contract unpublishes the alias during lookup and verifies refusal.
All 371 pinned Rust tests and eight focused MSVC Release contracts
passed. The linked bridge fixture built and its device run is pending.

Rust now owns the application buffer batches and the physical backbuffer
lease through Present. It publishes a complete batch and releases a
failed D3D12 resize's partial allocation. Failed GPU drain preserves
the outstanding resources. The worker passed 377 pinned Rust tests
and 21 MSVC Release contracts. Its eight PE export inventories matched.
[Presentation buffer audit](research/rust-d3d11-present-buffers.md)

The merged build passed 377 locked Rust tests, formatting and Clippy,
plus 13 focused MSVC Release contracts. All eight Release export
inventories matched, and the Anomaly DLL retained its three exports and
dynamic Runtime lookup. The isolated Standard run used the new Runtime
and resized from 1280×720 to 1600×900. It drew all 389 visible overlay
frames, recorded 240 DLSS-G vendor presents per 120 source frames,
and completed 3,089 DLSS evaluations with zero refusals. The engine
shut down Direct3D, exited with code 0 and the test restored the staged
inputs. Logs and hashes are in
`.local/anomaly/staging/live-58fe95a-stage5/evidence/merged-present-buffers-standard/`.

Rust now also owns the physical DXGI swapchain and its creation queue as
one pair. A live lease defers provider replacement, and reentrant DXGI
queries can borrow the pair without holding its lock across native calls.
The merged build passed 381 pinned Rust tests, formatting, Clippy and
eight focused MSVC Release CPU and ABI contracts. All eight Release
export inventories matched. The Anomaly DLL retained its three exports
and dynamic Runtime lookup. The merged pair still needs a game and
device run for resize, fullscreen and provider transitions. Worker and
integration logs are in `.local/rust-1.94.1-verification/`.
[Physical swapchain audit](research/rust-d3d11-present-physical.md)

The isolated Standard clone then reached gameplay on `35f99ca`,
resized from 1280×720 to 1600×900 and shut down through the engine.
The final full SR status sample reported 689 DLSS evaluations and zero
refusals. The process exited with code 0 and the staged inputs were
restored. Windows kept it in the background, so DLSS-G never became
active. Foreground input, generated-frame continuation after resize,
AVX, fullscreen and provider transitions still need checks on the
merged owner.
[Standard retest evidence](research/rust-d3d11-present-physical.md#merged-standard-game-retest)

The combined bridge build passed 15 focused CPU and ABI contracts in
Debug and Release, and all 290 locked Rust tests, formatting and Clippy.
Both configurations match the reference Runtime, AC7, Loader and
Minecraft export pairs, and the Anomaly DLL retains its staged exports
and dynamic Runtime lookup. The production-linked bridge and shared
surface device fixtures built. Real queue joins, generated presentation,
resize and teardown still need device and game evidence.

Anomaly now builds its leased Stage 5 SR pass and selects its reset in
Rust. SR and FG share the X-Ray camera and clip-mapping implementation.
The native seam supplies the guarded engine facts and texture pointers
for the synchronous Runtime evaluation. The native fixtures use the
same Rust pass builder. See [the SR pass audit](research/rust-anomaly-sr-pass.md).

The merged camera and pass port passed 27 focused MSVC contracts in
Debug and Release and 291 locked Rust tests. Clippy, formatting and
binding regeneration passed. The pass and reconstruction fixtures
passed syntax checks under MSVC 19.44 with warnings denied. The
adapter DLL exports, Runtime lookup and motion pack match the
staged baseline.

The offscreen SR acceptance run at `58fe95acdca6` passed the
round-trip, jitter, decode, thin-motion and prepare/reinsert fixtures
on hardware. DLSS, FSR3, FSR4 and XeSS each evaluated all test frames
in the vendor switch fixture, including the return to DLSS. The
offscreen DLSS run logged the expected absence of a Present callback.
The staged artifacts and fixture receipts are recorded separately
from game results. See [the device acceptance results](research/rust-anomaly-sr-pass.md#offscreen-device-acceptance).

The same build ran Anomaly 1.5.3 Standard and AVX from an isolated save.
Both reported 569 DLSS evaluations with zero refusals at the last status
sample. Both resized from 1280×720 to 1600×900, saved three SR and final
color captures, and exited through the engine's quit path with code 0.
The pipeline sidecars contain the evaluated jitter, backend and
clip-to-previous matrix. The general capture headers lack that camera
metadata. Both feeds reached the FG provider, which stayed inactive.
The original C++ adapter also reported inactive FG with the merged
Runtime in an isolated Standard comparison. A foreground Standard run
then confirmed DLSS-G 2× before and after resizing and saved live
sharpening changes through the overlay. Disabling SR restored native
rendering and exposed an FG depth admission defect. Stage 5 now admits
native depth for the exact main frame that prepared with SR off and
never swapped its targets. The fix kept DLSS-G active in AVX with SR
off, and the overlay restored DLSS SR and switched through FSR3 and
FSR4. First-use FSR4 selection took about 32 seconds.

Forcing a jitter refusal in Standard exercised bilinear recovery
before and after a resize. The scene and HUD rendered at both output
sizes. The camera responded to game mouse input and stayed still
during overlay mouse handling. The same test reproduced repeated
DLSS resize refusals before the first evaluation. The pipeline now
releases resources at resize only after a DLSS evaluation attempt.
The foreground Standard retest completed that resize in one attempt
while still drawing the fallback and exited normally. Gameplay
keyboard consumption remains open. The thin carrier and guarded
trampolines give the adapter process lifetime. The lifecycle fixture
covers quiesce and stop, and the in-game runs cover normal engine exit.
See [the game results](research/rust-anomaly-sr-pass.md#scripted-anomaly-153-runs).

The Stage 5 capture reports the guarded engine frame ID in its leased
pass. A successful evaluation can fill the selected capture header's
camera and backend through the existing Runtime result call. Both C
linkage forms passed the CPU checks for matching identity, extent and
successful evaluation. Four live Standard captures also matched the
DLSS sidecar across a resize. The same run confirmed mouse input after
closing the overlay and recorded panel keyboard handling. Gameplay
keyboard delivery remains unverified. See [capture identity at evaluation](research/rust-anomaly-sr-pass.md#capture-identity-at-evaluation).

The SR and motion stages share one Rust frame-dump binding. An isolated
Standard run preserved SR capture priority, and AVX saved motion and
G-buffer position when motion capture owned the service. Both resized
and quit normally. Motion headers now carry the reset generation
already checked by the collector. The AVX retest captured generation
0 before resize and generation 1 afterwards.
See [the capture binding results](research/rust-anomaly-sr-pass.md#shared-capture-binding).

The foreground Standard run verified live XeSS SR with DLSS-G, FSR3
and XeSS frame generation, FG Off, and the return to DLSS-G. Provider
switches exposed a wrapped diagnostic delta. The Anomaly report
now compares valid totals within one bridge generation.
The foreground retest confirmed both transition reports and stable
present counts after each switch.
See [the live provider switches](research/rust-anomaly-sr-pass.md#live-provider-switches).

The merged Release sharpening fixtures passed on D3D11, D3D12 and
Vulkan. The D3D11 fixture checks that Depth RCAS and Depth DAS preserve
source channel ratios and alpha in HDR and when output clamping applies.
See [the sharpening results](research/sharpening.md#depth-mode-color-preservation).

The first source inspection found that the public frame sequencer opened a rendered
frame and assigned its provenance under separate locks. Direct callers could read
or retire the slot between those operations. The FG session adds its own outer
lock. The sequencer now publishes the frame and source timestamp together.
Built the Release FG session fixture with MSVC and passed the focused `fg_session`
CTest. This exercises sequencing semantics. The race was identified by source
inspection. The isolated build omitted the optional corrected Vulkan Streamline
runtime because the local receipt disagrees with the present capture header.

SR backend selection now runs in the `rsf-orchestrator` Cargo static library.
The two public C entry points forward to one Rust implementation. Release MSVC
passed `gpu_sr_policy`, `gpu_sr_policy_embedded` and `anomaly_sr_runtime_api_contract`.
PE inspection confirmed that only the DLL policy fixture imports the runtime.
All 381 export names and ordinals in the rebuilt DLL matched the previous local
Release DLL. This compares the two artifacts on this machine. The focused crate
format and Clippy checks and locked workspace tests passed with Rust 1.100.0.
Cross-build, device and in-game validation of the port remain pending.

The frame sequencer and the two GPU policy exports now implement their public C
entry points directly in Rust. A native export list serves the DLL and embedded
consumers. The FG session fixture now calls the built DLL and checks rejection of
a short sequence-state output. Release MSVC passed that fixture, both GPU policy
fixtures, and the Anomaly SR runtime API contract. Debug MSVC passed the FG
session and embedded policy fixtures. All 381 Release exports matched the local
baseline by name and ordinal. A synthetic comparison against the previous local
DLL matched results and returned state for 30,000 seeded sequencer operations.
The locked Rust workspace tests and orchestrator Clippy check also passed.

The workspace now includes `rsf-game-sys`, generated from the C plugin entry
point and its transitive types. The sequencer imports rendered-frame provenance
from it. Bindgen 0.72.1 with libclang 18.1.1 reproduced the Windows x64 bindings
byte for byte. Cargo compiled their layout assertions, the workspace passed
formatting, Clippy and 75 tests, and the four focused Release fixtures passed.

Rust now owns plugin DLL lifetime, the stable copy of host services, preparation
and all session transitions. A nonblocking reader/writer lock admits concurrent
status calls and refuses overlapping transitions. Small C++ callback guards
contain exceptions from native plugins before control returns to Rust.
The new DLL fixture passed in Debug and Release MSVC, covering reentrant calls,
short input, ABI mismatch, exceptions during preparation, partial ownership,
busy stop and eventual module unload. Both Anomaly DX11 build probes refused
preparation in a foreign process with a cleared handle and unloaded adapter.
These checks exercise admission and teardown without activating game rendering.

FG provider settings and D3D11 frame assembly now run in Rust. The settings
owner retains the UTF-16 INI path and uses the same Windows profile functions.
All 3,084 mask/fallback combinations in a synthetic comparison against the
previous local DLL matched for startup, Auto resolution, memory-only save and
status. The native settings fixture also checked disk persistence.

Frame assembly now rejects short outputs, including a zero capacity, before
writing. The old assembler accepted them and wrote an entire backend frame.
Both calls in the D3D11 pipeline and the native SR caller set the full size.
The shared binding generator takes the frame layout and provider mask from the
C headers. Debug and Release MSVC passed frame assembly, FG settings, the
embedded GPU policy and the Anomaly SR API contract. The rebuilt Release DLL
matched the baseline's 381 export names and ordinals. These are CPU/ABI checks.

The bounded render-link table now lives in Rust. Its native fixture calls the
runtime DLL and checks competing binds, cross-thread lookup, stale generations
and late release after key reuse. It exercises the public contract synthetically.
The table, frame sequencer and plugin sessions share fallible handle allocation.

On the D3D11 SR path used by Anomaly, Rust now claims a pending history reset
before calling the evaluator and restores it on refusal. The previous success
path could clear a second reset requested during evaluation. A regression test
requests that second reset inside the callback and checks the following frames.
The temporary reset copy also reports its actual prefix size for extended input
structures. Debug and Release MSVC passed render links, FG session, plugin
session lifetime, embedded policy and the Anomaly SR API contract. All 76 Rust
workspace tests passed. The Release DLL retains all 381 baseline export names
and ordinals. No device or game run was performed for these changes.

The D3D12 SR session now owns FSR and XeSS contexts, copied SDK paths, and
frame history in Rust. The C creation entry wires the existing provider tables
and shared frame validator into that owner. A provider reporting success without
a context leaves the current backend active. Warm-up rejects a stale output
extent without dispatching or consuming frame identity.

Release and Debug MSVC passed the SR session, FG session, frame assembly,
embedded policy and Anomaly SR contract fixtures. The Release AC7 carrier
exported the SR session functions without importing the runtime DLL. The
current C++ Anomaly adapter built in the same configuration. The Release
runtime retained all 381 baseline export names and ordinals, and all 76
Rust workspace tests passed. Device and in-game validation remain pending.

The generic FG session now owns chain switching, provider lifetime and frame
sequencing in Rust. It shares the frame ledger with the public sequencer and
uses the existing C predicates and D3D12 fence query. The transition keeps state
unavailable while host callbacks run, so a producer called from `resume`
receives `NOT_READY` without deadlocking. The pending chain and resources stay
owned until the provider reports fence completion and the host releases its
references.

Debug and Release MSVC passed the FG and SR session fixtures, the embedded
policy fixture and the Anomaly SR API contract. Release also built the linked
AC7 carrier and C++ Anomaly adapter. All 381 runtime export names and ordinals
matched the baseline. Rust workspace tests and Clippy passed. The generic FG
session is currently exercised by fixtures. Anomaly uses the separate native FG
path, whose device and in-game validation remain pending.

The native CPU reservation ledger and predecessor Present outcomes now live in
Rust. Rust validates reservation transitions and promotes a reported poll
timestamp only after bind proves which frame consumed it. The existing CPU
dispatcher still owns the frame lock and calls the sink outside it. The
latency input fixture links the same CPU library as the runtime.

Debug and Release MSVC passed six focused fixtures, including the Standard and
AVX Anomaly adapter input contracts through the rebuilt DLL. These fixtures
exercise the event path with graphics activation disabled. The C++ Anomaly DLL
and linked AC7 carrier built in Release. All 381 runtime export names and
ordinals matched the baseline. Rust workspace tests and Clippy passed.

The D3D11 FG table and embedded AC7 carrier now share one Rust render-pass
dispatcher and one CPU callback entry. The dispatcher preserves each native
service's acceptance and ordering. Scoped capture rejects an undersized frame
and restores the outer scene identity after the synchronous call. The thin
native bridge supplies the existing services and the DLL table.

Debug and Release MSVC passed both D3D11 FG ABI fixtures, both Anomaly input
contracts and the Anomaly SR contract. The C11 fixture now checks game ABI 24,
which already existed on the branch. Both linked AC7 builds succeeded. The
Release runtime retained all 381 export names and ordinals. The 77 Rust workspace
tests include a synthetic regression for nested capture and short frames.
Device and gameplay validation remain pending.

The per-thread D3D11 scene registry and presentation-path selection now run
in Rust. The native bridge still queries canonical COM identity and the
current backbuffer dimensions. The registry retains numeric identity only,
and refuses to replace an incomplete live record on a slot collision.
A synthetic regression covers incomplete, direct, sampled and ambiguous paths.

Five focused D3D11 and Anomaly ABI fixtures passed in both Debug and Release.
The linked AC7 carrier built in both configurations. All 381 Release DLL
export names and ordinals matched the baseline, and all 78 Rust tests passed.
The D3D11 presentation device fixture built in Release. Its device path and
in-game verification remain pending.

Rust now owns generic FG resource and fence leases. The lease table and
FG session share the native D3D12 fence query. The CPU-only fault fixture
checked reference balance, both fences' failure paths, and refusal to reuse
an occupied slot. The native WARP fixture built in Debug and Release.
The fault fixture and five related CPU and ABI fixtures passed in both
configurations. The integrated Debug PE exports match the worker's verified
baseline of 381 runtime and 256 carrier names and ordinals. The older Release
carrier artifact predates four receipt-routing exports already present at
the migration baseline. WARP execution and gameplay testing remain pending.

Native D3D11 SR and explicit FG capture now share Rust camera assembly and
history continuity checks. Each path keeps its own successful-frame history.
The history compares depth-domain layout and accepts projection changes within
that layout. It preserves 32-bit rollover and also follows consecutive 64-bit
native frame IDs without confusing them with their truncated values.
The native render configuration is published from one Rust snapshot.

Rust now owns native D3D11 region preparation, depth-domain validation and
temporal-mask sequencing. The three HLSL literals match the pre-port bytes.
The worker compared 100,000 deterministic depth preflights against the
previous implementation in each build configuration. Eight integrated CPU
and ABI fixtures passed in Debug and Release, and the auxiliary and rendered
FG host device fixtures built. Device execution remains pending.

The D3D11 overlay service now owns its configuration, adapter callbacks,
capture authorization, settings application and status in Rust. The existing
panel remains the input and draw frontend. A settings callback can read back
the applied request. A write attempted during an active apply receives
`RSF_ERROR_BUSY`, so it cannot deadlock or replace the outer write.
The integrated bindgen recipe reproduced the overlay bindings byte for byte.

Nine integrated overlay, settings, D3D11 host and Anomaly input fixtures
passed in Debug and Release, alongside all 100 Rust tests. The C++ Anomaly
adapter and both linked carriers built. The runtime retained 381 baseline
exports, including overlay table ordinals 144 and 145. The two carrier export
lists also matched. The overlay device fixture built in Release. Device,
visible-window and gameplay checks remain pending.

The shared D3D11 overlay host now owns the panel, renderer, device,
draw admission, texture sequencing and failure state in Rust. A
layout failure latches until stop and stop waits for an in-flight
draw. The integrated build passed 395 pinned Rust tests and 12
focused MSVC Release contracts. All checked PE export inventories
matched, and Anomaly kept its three exports and dynamic Runtime lookup.
The merged Standard game run drew the panel through resize, saved
SR Off and On, and switched DLSS-G to FSR3 and back on the same game
facade. AVX drew the panel through resize, saved both directions of
the DLSS and FSR3 SR switch, and returned gameplay mouse input after
the panel closed. Both profiles exited cleanly. The Vulkan
Streamline-disabled build passed 13 focused Release contracts and
preserved the four checked export inventories and stub results.
The enabled integrated Debug build passed 11 focused contracts and
matched the Runtime, AC7, Loader, Minecraft and Anomaly exports.
Eight matched game captures checked the actual SR color, depth,
motion and reconstructed output on both Anomaly profiles. The AVX
set includes the FSR3 evaluation after switching from DLSS.
[Matched evaluation captures](research/rust-overlay-host.md#matched-sr-evaluation-captures)
Two later AVX capture runs matched 32 evaluation and Present records.
The consecutive burst captured two camera cuts with history reset on
each cut, followed by successful evaluations and HUD-composited output.
[Camera cut evidence](research/rust-overlay-host.md#matched-camera-cuts-on-avx)
The merged AVX pan run captured two smaller turns with DLSS history
retained. Both matched Presents carried a complete image and finite
submitted motion across the active extent. The engine quit cleanly.
[Camera pan evidence](research/rust-overlay-host.md#camera-pans-without-history-reset)
Four later AVX frames with an unchanged camera carried small
nonzero motion in a window region through the producer, SR handoff
and actual DLSS submission. All 16 evaluations matched their Presents.
[Unchanged-view motion evidence](research/rust-overlay-host.md#motion-at-an-unchanged-camera-view)
An AVX held-item capture retained genuine first-person motion from
the low-depth producer through DLSS. Two changing hand silhouettes
had unchanged camera matrices. All 16 evaluations matched their
Presents and the game quit cleanly.
[First-person animation evidence](research/rust-overlay-host.md#animated-first-person-inputs)
The AVX follow-up disabled motion generation and kept SR active.
The matched capture verified zero fill for all first-person pixels
and reconstructed world motion on both sampled turns. The engine
quit cleanly and the original settings were restored.
[Zero-fill fallback evidence](research/rust-overlay-host.md#first-person-zero-fill-with-missing-motion)
The temporary AVX pointer-bound Forward test kept the captured eye
position stationary and left the player indoors. It preserved the
autosave and restored the original bindings.
[Movement attempt evidence](research/rust-overlay-host.md#pointer-bound-movement-attempt)
D3D11-on-12 wrapping, fullscreen and held-key gameplay remain open.
[Shared overlay host evidence](research/rust-overlay-host.md#integrated-runtime-and-standard-game-run)

Rust now validates the split Vulkan SR C ABI and tracks the producer and
continuation submission lease. The older ABI 2 frame prefix still zero-fills
the appended image, and a refused request retains the original command
buffer. A provider failure preserves its returned continuation. The existing
native provider owns Vulkan and D3D12 objects and GPU completion.

The enabled-stub, disabled-provider and DLL split contracts passed in Debug
and Release with three related Vulkan and Anomaly fixtures. The C11 split
header, runtime and linked carriers built. All 101 Rust tests and Clippy
passed. The Release runtime and both carriers retained their verified
381, 256 and 273 export names and ordinals. Device execution remains pending.

The main Vulkan SR path now shares Rust frame-prefix normalization with the
split path. Rust validates the setup, fallback image pair, frame identity,
camera and image leases, and tracks history and SDK device-proc registration.
The provider and native GPU commands still run through the existing backend.
The synthetic checks cover proc aliases and final deregistration, and verify
that XeSS can use an actual extent different from its plan when its camera
and images agree.

Six Vulkan, latency and Anomaly ABI fixtures passed in Debug and Release.
All 103 Rust tests and workspace Clippy passed. Both linked carriers built
and retained their export lists, alongside all 381 runtime exports.
Device and in-game verification remain pending.

Rust now owns Vulkan FG dispatch across DLSS, FFX3, FSR4 and XeSS. It admits
one primary swapchain, stores the selected provider and generated count, and
retains the owner when cleanup needs another attempt. The provider-specific
Vulkan, DX12 and SDK operations and the existing XeLL borrow gate stay behind
the private native callbacks. Creation now refuses success without a provider
handle, and a failed drain reopens latency borrows when retirement permits it.

Five Vulkan CPU and DLL ABI contracts passed in Debug and Release. The
synthetic providers exercised partial creation, retryable cleanup, failed
drain and retirement during an in-flight XeLL sleep. All 103 workspace Rust
tests, formatting, Clippy and byte-for-byte binding regeneration passed.
Release export names and ordinals matched for the Runtime and both carriers
(381, 256 and 273). Device and in-game verification remain pending.

The Vulkan presentation binding and CPU borrow now live in Rust. The same
owner arbitrates provider create, drain and destroy against checked SDK
shutdown. The lock covers admission only. SDK calls and XeLL sleep run
outside it. Busy teardown refuses immediately, failed provider destruction
retains the exact binding, and a failed checked shutdown can reopen borrowing
when no provider retirement is pending. The former private C++ lifecycle
callbacks are gone.

Synthetic coverage exercises refusal while XeLL sleep is in flight, shutdown
attempted inside a provider drain, and exact-owner retirement. All seven
focused Vulkan contracts passed in Debug and Release, and all 120
workspace Rust tests, formatting, Clippy and binding regeneration passed.
Release export names and ordinals matched for the Runtime and both carriers
(381, 256 and 273). Device and in-game verification remain pending.

The Vulkan latency service now checks reported submission and Present
boundaries in Rust against the shared CPU ledger and the exact session and
source. A report emits one admitted marker or returns the existing duplicate
or refusal result. A duplicate CPU begin cannot reopen a presented source.
The legacy Present path retains a newer reported render when an older frame's
Present ends, and a repeated begin leaves the active Present identity intact.
The native SDK calls remain in their existing ordering scopes.

Six focused Vulkan, native CPU and ABI contracts passed in Debug and Release.
All 120 Rust workspace tests, formatting and Clippy passed. Release exports
matched the Runtime and both carriers (381, 256 and 273). Device and in-game
verification remain pending.

Vulkan Streamline token identity, marker deduplication and pacing admission
now live in Rust. Source IDs outside the SDK's 32-bit range are refused.
A delayed cancellation only clears its own session and source. Input with
an unrelated window message is rejected before emitting a controller marker.
The native SDK options and sleep run outside the state locks. While they
borrow the current token, a competing acquisition or SDK shutdown cannot
invalidate it, and older tokens remain available for Present markers.

Five focused Vulkan cases passed in Debug and Release, including a synthetic
Reflex sleep with a concurrent older Present, checked shutdown and token
acquisition. The available Runtime build omits the optional native Streamline
package, and the latency contract exercises that code path with an SDK double.
All 120 Rust tests, formatting and Clippy passed. Release export names and
ordinals matched the Runtime and both carriers. Device and game validation
remain pending.

The Vulkan Anti-Lag device binding, options, frame keys, counters and
retirement now live in Rust. Driver calls borrow the device outside the
state lock. Checked shutdown refuses while a call is active. The device
destroy notification drains those borrows while allowing already-owed
Presents to proceed. Input and cancel use the reported session, and the
limiter shares the Direct3D Anti-Lag interval conversion.

Five focused Vulkan and Godot contracts passed in Debug and Release, with
12 additional Release repetitions of the Anti-Lag fixture. The stubs
exercise feature-chain reuse, shutdown refusal, retirement during a
blocked input call and an older Present during that retirement. All
120 Rust tests, formatting and Clippy passed. Runtime and both carrier
Release exports matched. Device and game behavior remain unverified.

Vulkan Anti-Lag also admits device queries and binds an AMD device in
Rust. The driver leaves still inspect the Vulkan feature chain, preserve
the caller's feature copy, resolve the actual created device's update
entry point and marshal the SDK update. Unsupported hardware and missing
entry points keep their established C ABI results.

Seven focused CPU contracts passed in Debug and Release, including AMD
device admission, Godot creation fallback and Anomaly SR and D3D11 host
wiring. All 212 locked Rust tests, formatting, Clippy and binding
regeneration passed. The four Release export inventories matched.

Rust now owns the Anti-Lag feature-chain edit and checks the actual
created device's extension and feature before binding its update entry
point. The Vulkan leaves report driver support and typed create facts.
The chain scan checks termination before changing a caller node. The
CPU fixture rejects a cyclic chain with the input links intact and
still reuses an existing feature node. [Vulkan structure chains](https://docs.vulkan.org/refpages/latest/refpages/source/VkBaseOutStructure.html)

All 377 locked Rust tests, formatting and Clippy passed with Rust
1.94.1. The MSVC Release Anti-Lag, native latency and Godot creation
contracts passed, and binding regeneration was byte-for-byte stable.

Vulkan PCL registration, queue fencing and input fact collection now live in
Rust. The owner binds the actual accepted input thread and keeps its old
queue untouched while rebinding. It admits only the registered thread's
exact ping ID after its queue fence, reports actual dequeue timestamps
through the shared CPU dispatcher, and clears the published registration
on shutdown. The CPU entry checks the declared size before reading its
event version.

Six focused Vulkan, Godot and CPU contracts passed in Debug and Release.
The PCL fixture uses real Win32 queues and an SDK double to verify rebind
failure, stale pings, queue limits and preservation of the engine's quit
message. All 131 Rust workspace tests, formatting and Clippy passed.
Release export names and ordinals matched the Runtime and both carriers.
Device and in-game validation remains pending.

Rust now owns Vulkan latency initialization state, ABI admission, options,
pacer selection, CPU routing, SDK token leases, counters and checked shutdown.
The native shim retains the C exports, Streamline and XeLL calls and the
native WSI resolver and timing calls. Older Presents still advance during
provider sleep. Status reports a single captured owner, and shutdown holds
both presentation and AMD retirement reservations until cleanup completes.

Five focused Vulkan and Godot contracts passed in Debug and Release, covering
provider leases, Streamline token validation and AMD retirement with SDK
doubles. The Runtime and both carriers retained their Release exports. All
131 locked Rust tests, formatting and Clippy passed. The real optional
Streamline package, device execution and games await validation.

Rust now owns the DLSS Vulkan Present capture ledger. It associates each SDK
token with the copied session, source, view and resource generation, refuses
ambiguous or out-of-range identities, and accepts dedicated fence completion
only for the submitting queue family. A device drain proves submitted work
complete. If a failed recording leaves an unsubmitted readback, the owner
keeps it out of the legacy pixel flush and releases it on teardown. Native
code still issues the Vulkan copies and consumes the original Present waits.

Five focused Vulkan contracts passed in Debug and Release, including the
synthetic DLSS SDK and WSI fixture. All 144 workspace Rust tests passed,
including three capture ledger regressions. Formatting, Clippy, binding
regeneration and Runtime and carrier export parity passed. Real captured
pixels, GPU synchronization and in-game behavior remain unverified.

Rust now tracks Vulkan DLSS FG recording order, submitted command lists,
HUD-less admission, and the last accepted source. A failed recording that
reuses a slot keeps the last accepted history intact. An older Present
cannot reopen a consumed source, and an unmatched Present leaves unrelated
recordings eligible. The native path still records the GPU work and waits
the actual queue and SDK fences.

Five Vulkan and DLL ABI contracts passed in Debug and Release. The DLSS
fixture covers ordered Present and duplicate refusal with SDK and WSI
doubles. All 146 workspace Rust tests, formatting, Clippy and binding
regeneration passed. Live driver and game behavior remain unverified.

FidelityFX and DLSS Vulkan FG now use the same Rust recording and Present
ledger. FidelityFX still tracks capture-only guides while interpolation is
Off. Reused command addresses fence only the newest recording, and discarded
copies no longer establish image layouts. Both providers retain independent
native SDK calls and fence waits.

Six focused Vulkan contracts passed in Debug and Release. The FidelityFX
provider built with the pinned headers. The shared ledger regression covers
address reuse, layout invalidation and duplicate notification. All 161 Rust
tests, formatting and Clippy passed. Vulkan device and game validation
remain open.

The shared D3D12/Vulkan importer now owns NT handles, Vulkan semaphore and
image imports, device-local memory selection, and retained D3D12 image
references in Rust. It checks adapter identity before import, publishes only
bound resources, and keeps them live when destruction lacks proof that both
queues drained. Vulkan descriptor and driver calls stay in the native leaves.

Six focused Vulkan contracts passed in Debug and Release. The synthetic
interop fixture covers partial failures, missing handles or resources, memory
type refusal and retryable teardown. All 192 locked Rust tests, formatting,
Clippy and binding regeneration passed. Runtime, carrier and Minecraft
Release exports matched. Real D3D12/Vulkan import and game behavior await
device validation.

The DLSS and FidelityFX Vulkan FG paths now share Rust ownership of their
private images and source and consumer fences. A failed image replacement
releases the partial allocation and preserves the committed image. A failed
completion signal keeps the slot unavailable for reuse. Native leaves still
issue every wait, reset, allocation and queue submission.

Seven focused Vulkan contracts passed in Debug and Release, including
FidelityFX failed-bind rollback, split submissions and duplicate refusal.
All 192 locked Rust tests, formatting and Clippy passed. Device timing and
in-game behavior remain unverified.

The shared Vulkan FG planner now checks the stored source's interpolation
veto and accepts typed recording slots across the three submission owners.
FidelityFX and DLSS-G share source admission and bounded image enumeration.
The native Vulkan and external D3D12 paths use one image-barrier ABI.
Source review, generated bindings and pinned formatting are complete for
this follow-up. Builds, fixtures and device checks remain open.
[Shared FG and WSI follow-up](research/rust-vulkan-ffx-fg-copy-plan.md#shared-fg-admission-submission-and-wsi-follow-up)

The shared frame-capture owner now keeps requests, exact capture identity,
metadata, ticket publication and GPU completion bounds in Rust. It closes
new reservations at the first matched Present and lets pending readbacks
retire by ticket. The existing graphics readback and encoder calls stay
native. Metadata strings are escaped before writing the existing JSON
schema.

The integrated Release build passed ten focused contracts, including
capture metadata, the C++ D3D11 host and the synthetic Vulkan DLSS fixture.
All 15 JSON files from the capture fixture parsed. All 161 locked Rust
tests, formatting and Clippy passed. Runtime, both carriers and Minecraft
kept their export names and ordinals. The three graphics readback fixtures
built. Device and game validation remain pending.

The FSR4 Vulkan presentation owner now manages its bridge, imported
resources, source recordings, Present, and checked retirement in Rust.
Its source fences prove capture completion independently of the imported
Vulkan/D3D12 timeline. Superseded recording groups cannot publish.

The combined Release build passed nine focused Vulkan contracts, including
FSR4 state, bridge, shared importer and ordinary FidelityFX and DLSS
submission cases. All 192 locked Rust tests, formatting and Clippy passed.
Runtime, carrier and Minecraft exports matched. The worker also verified
the feature-disabled build. Device and game execution remain pending.
See [the FSR4 Vulkan FG audit](research/rust-vulkan-fsr4-fg-cutover.md).

The later FSR4 FG setup port moves adapter selection by LUID, D3D12
command setup, and the HWND swapchain plan into Rust. The native capsule
keeps its COM references and the typed descriptors borrowed by
FidelityFX. Source comparison matched setup, refusal, and retry order.
The build, contract, and device checks for this port remain open.
[Graphics setup](research/rust-vulkan-fsr4-fg-cutover.md#dxgi-and-d3d12-setup-ownership)

Vulkan XeSS FG now shares its swapchain, source leases, submission ledger,
capture retirement and checked teardown with FSR4 FG in Rust. The shared
allocator preserves the engine swapchain's usage flags and tracks them in
its image cache. Each provider still uses its own DX12 bridge. The XeLL
sleep gate lets an older real-frame Present and its markers proceed while
a newer frame sleeps.

Thirteen focused CPU contracts passed in MSVC Debug and Release, including
the shared FSR4 and XeSS routes, the DX12 importer and sibling Vulkan SR
providers. All 207 locked Rust tests, formatting and Clippy passed. Runtime,
both carriers and Minecraft Release exports matched. The linked XeSS
device fixture built. The worker verified SDK-disabled linkage in its
isolated build. Device and game execution remain pending.
See [the XeSS Vulkan FG cutover audit](research/rust-vulkan-xess-fg-cutover.md).
V06 moved integrated ABI admission, interpolation count policy and diagnostic
publication into the bridge's Rust session. The source port and remaining
shared overlay dependency are recorded in [the V06 audit](research/rust-vulkan-xess-fg-v06.md).

Vulkan DLSS FG now owns proxy admission, exact source state, private GPU
leases, capture groups and checked teardown in Rust. It shares the FG
frame ledger and requires both the host queue proof and the Streamline
input completion lease. An abandoned recording cannot publish capture
pixels, and an unprovable SDK completion keeps its resources alive.
Capture detaches before the SDK feature is freed.

The merged branch passed 14 focused CPU contracts in Debug and Release,
including enabled Streamline leaves driven by synthetic callbacks and the
Anomaly SR and D3D11-host contracts. All 212 locked Rust tests, formatting
and Clippy passed. The two stub Runtime DLLs passed ABI admission and
null-teardown checks. All four Release export inventories matched.
The corrected Streamline runtime receipt was unavailable. SDK execution,
GPU completion, capture pixels and gameplay remain unverified.
See [the Vulkan DLSS FG cutover audit](research/rust-vulkan-dlss-fg-cutover.md).

The Vulkan DLSS-G API error latch now lives in Rust. The native callback
forwards the SDK code. The owner consumes it when it reports the Present
attempt, including when options and the Off fallback both fail before
WSI. The synthetic lifecycle fixture verifies that this failure cannot
taint the next successful Present. The callback supplies no frame or
viewport identity, so the status reports the latest SDK code.

All 371 locked Rust tests, formatting and Clippy passed on Rust 1.94.1.
Four Streamline-enabled MSVC Release contracts passed, and the Runtime
and linked export inventories matched. The SDK-disabled Runtime
passed its DLL export contract and returned `NOT_COMPILED` with cleared
outputs and no callbacks for a valid DLSS-G setup. These are CPU and
ABI checks. Logs are under `.local/rust-1.94.1-verification/`.

Vulkan FidelityFX 3 FG now owns source admission, slot reuse and
checked teardown in Rust. It fences GPU consumers even if Present
returns an older asynchronous error. Refused creation cleanup retains
the owner for retry, and partial retirement closes frame admission.
FFX, DLSS, FSR4 and XeSS now share one Rust reader for the explicit
Present identity prefix. The native SDK and Vulkan call boundaries
remain in place.

The merged branch passed 16 focused CPU contracts in MSVC Debug and
Release, including the repaired FFX reset fixture, both capture linkage
paths and the C++ Anomaly SR and D3D11 host. All 216 locked Rust tests,
formatting and Clippy passed. The four Release export inventories
matched. SDK initialization, device behavior and game testing remain
open. See [the FidelityFX Vulkan FG cutover audit](research/rust-vulkan-ffx-fg-cutover.md).

Rust now also owns the FidelityFX 3 Vulkan WSI route. Each native
callback borrows its admitted leaf until the callback returns.
Retirement closes admission and retains the leaf while a borrow
is active. A successor can register after native close unregisters
the current route. The worker passed 12 enabled and three disabled
MSVC Release contracts and matched the Runtime and three linked
export inventories. The callback-retirement regression is synthetic.
[FidelityFX WSI ownership audit](research/rust-vulkan-ffx-wsi-owner.md)

The FidelityFX generation callback now reads the exact configured frame
record from the Rust WSI route. Rust publishes the record only after SDK
configure succeeds and counts only successful dispatches. The native callback
still supplies the SDK reset flag and records the generated image. All 379
locked Rust tests, formatting, Clippy and seven focused MSVC Release
contracts passed. The native fixture rejects a mismatched frame and checks
that SDK dispatch failure leaves the count unchanged. All five checked DLL
export inventories matched by name and ordinal. Live FidelityFX generation
and capture remain unverified for this change.
[FidelityFX callback follow-up](research/rust-vulkan-ffx-wsi-owner.md#generation-callback-source)

Rust owns the opt-in Vulkan timing CSV path, shared output and per-module
record limit. FidelityFX scopes, interop waits and Reflex pacing use the
same logger. The file test verified the header, row format and 2,048-record
limit. The merged build passed 382 pinned Rust tests, formatting, Clippy
and eight MSVC CPU and ABI contracts, including Vulkan latency. Runtime
and linked export inventories matched. The logger has yet to collect
timing from a live Vulkan frame on this build.

The MSVC Release configuration without the corrected Streamline Vulkan
SDK also built the merged core. All ten focused CPU and ABI contracts
passed, including the DLL export and embedded FG12 and render frontend
contracts, the FidelityFX WSI fixtures and the Anti-Lag feature chain
fixture. Runtime, AC7, Loader and Minecraft retained all 381, 256, 273
and 267 export name and ordinal pairs, respectively. A probe against the
rebuilt Runtime DLL confirmed that Streamline Vulkan initialization
returns `NOT_SUPPORTED`. DLSS-G creation returns `NOT_COMPILED`,
clears both outputs, calls no host callbacks, and accepts null
destruction. The CMake build, CTest results and DLL probe are under
`.local/rust-1.94.1-verification/integrated-streamline-disabled-05facc0/`.

The integrated Vulkan owner build passed 379 pinned Rust tests,
formatting, Clippy and 13 focused MSVC Release contracts. The Runtime,
AC7, Loader, Minecraft and Godot export inventories matched, and the
Anomaly DLL kept its three exports and dynamic Runtime lookup. The
rebuilt Godot add-on matched all 37 runtime manifest hashes and
registered its classes in a headless process with the graphics bridge
inactive. This load check did not exercise a Vulkan device. Logs are in
`.local/rust-1.94.1-verification/`.

The ordinary Vulkan SR owner now manages FidelityFX context selection,
staging, dispatch, history, output publication and checked teardown in Rust.
The existing provider boundaries and SRVK ABI remain intact. FSR2/3
readbacks require command retirement and proof that the copy ran. FSR4 FG
readbacks retain their separate submission-group proof in the shared helper.

The combined Release build passed 14 focused Vulkan contracts, including
both readback paths, split SR, XeSS and the DLL export check. All 196 locked
Rust tests, formatting and Clippy passed. Runtime, both carriers and
Minecraft exports matched. Device and in-game validation remain open.
See [the Vulkan SR cutover audit](research/rust-vulkan-sr-cutover.md).

Split Vulkan FSR4 SR now owns frame admission, shared-image imports,
producer and continuation identity, history and teardown in Rust. It uses
the same D3D12/Vulkan importer as the other interop callers. A refused SDK
evaluation still returns the resumed command buffer for the host's spatial
fallback. Once checked SDK teardown starts, the owner closes frame
admission and retains resources when cleanup refuses.

The integrated build passed ten focused CPU contracts in MSVC Debug and
Release, including the FSR4 state test, both split stubs and the shared
importer test. All 206 locked Rust tests, formatting, Clippy and binding
regeneration passed. Runtime, both carriers and Minecraft Release exports
matched. The isolated worker also built linked Godot and verified the
FSR4-disabled Runtime. Device synchronization and game behavior remain open.
See [the split Vulkan FSR4 cutover audit](research/rust-vulkan-fsr4-sr-cutover.md).

Rust now owns standalone Vulkan snapshot admission and its pool, command
buffer, fence and pending submission. Each recorded copy has an exact
group and publishes only when its accepted submission completes. End or
submit refusal discards that group's readback. A timeout retains the
source lease. The shared Vulkan reader and encoders stay native.

The pre-port DLL fixture published poison pixels for two refused copies.
The merged branch passed nine focused CPU contracts in Debug and Release,
including the linked and DLL regression fixtures. The four Release export
inventories matched. The worker also compiled the C header and device
fixture and verified the optional capture-disabled DLL. Real GPU and game
capture remain unverified.
See [the capture submission cutover audit](research/rust-vulkan-capture-submit-cutover.md).

Vulkan XeSS SR now owns SDK and context lifetime, render-size admission,
private input and output images, temporal reset, captures and teardown in
Rust. It uses the Vulkan image allocator shared with DLSS SR and FG.
A refused evaluation clears history and leaves the caller's output for
spatial recovery. Captures require proof that the retired command buffer
executed the copy.

Ten focused CPU contracts passed in MSVC Debug and Release, including
XeSS range and allocation checks and the sibling Vulkan providers.
All 211 locked Rust tests, formatting and Clippy passed. Runtime, both
carriers and Minecraft Release exports matched. The worker also built
the linked Godot DLL and verified the header-disabled Runtime. GPU and
game validation remain open.
See [the XeSS SR cutover audit](research/rust-xess-vulkan-sr-cutover.md).

The Vulkan DLSS SR owner now manages its SDK lease, preset, temporal history
and private images in Rust. It shares Vulkan image allocation with FG and
publishes the engine output only after accepted SDK evaluation. A refused
SDK release keeps the owner available for retry. Readback publication uses
the command-retirement proof from the shared Vulkan capture helper.

Six focused Vulkan contracts passed in Debug and Release. The synthetic
DLSS fixture compiles the enabled SDK headers and covers failed evaluation,
history reset, command retirement and retryable release. All 204 locked
Rust tests, formatting, Clippy and binding regeneration passed. Runtime,
carrier and Minecraft Release exports matched. The ordinary Runtime uses
the optional Streamline stub in this build. Device and game validation
remain open.

The shared Vulkan SR owner now calls the Rust DLSS owner directly. The
private C++ forwarding table and six forwarding functions were removed.
The shared owner supplies the normalized frame and performs the public
ABI admission once. The enabled native leaves still handle SDK calls
and Vulkan commands. The Rust routing fixture checks image usage,
preset routing, recovery after a refused evaluation, command retirement
and retryable teardown.

Seven focused Vulkan and Anomaly CPU contracts passed in MSVC Debug
and Release. All 211 locked Rust tests, formatting, Clippy and binding
regeneration passed. The four Release export inventories matched.

The OpenGL pipeline now keeps its shared surface and SR lifecycle, frame
identity, fallback and FG requests in Rust. Native callbacks handle OpenGL,
DXGI and the presentation child window. A rejected SR rebuild remains
retryable, and overlapping graphics calls return `NOT_READY`.
Output calls read only the supplied size before writing. An interrupted
close after retiring the chain refuses new graphics work and can retry cleanup.

The integrated MSVC Release build produced the Runtime, both carriers,
Minecraft DLL and the OpenGL shared surface fixture. Five focused native
contracts and all 110 workspace Rust tests passed, including seven synthetic
OpenGL cases. Formatting, Clippy and binding regeneration passed. Export
names and ordinals matched for the Runtime, both carriers and Minecraft.
The OpenGL device fixture and the game remain unrun.

Rust now also owns the OpenGL and D3D12 sharing context and textures.
A refused D3D12 completion signal retains the slot and its leases. The
next begin or drain retries the signal and hands the textures back to
OpenGL once the queue accepts it. Recovery never executes the closed
list twice. Nine new synthetic cases and six focused MSVC Release
contracts passed in the worker build. The matching Release binaries
kept their export names and ordinals. The device fixture built and
device recovery remains unverified.
[OpenGL shared surface audit](research/rust-gl-shared-surface-cutover.md)

OpenGL sharing now admits the WGL entry points and Win32 extensions
in Rust, then acquires the D3D12 device for the exact WGL adapter.
Typed graphics calls remain in native leaves. Source checks matched
the lookup and adapter setup order. The expanded fixture, binary
linkage and device checks remain open.
[Context setup](research/rust-gl-shared-surface-cutover.md#context-capability-and-adapter-setup)

The integrated build passed all 371 locked Rust tests, formatting,
Clippy and eight MSVC Release contracts. The test gate skipped the
OpenGL device fixture. Runtime, AC7, Loader, its two aliases, Minecraft,
Godot and Anomaly retained their export names and ordinals.

Rust now owns D3D12 FG source identity, copied resources and retirement.
Unity confirms its recorded sources at the flush event before the DXGI
presenter consumes them. The C ABI and existing command, SDK and DXGI calls
stay available to shared and embedded consumers. OpenGL and D3D12 FG use the
same generated Rust definition of the setup contract.

The integrated Release build produced the Runtime, both carriers, Minecraft,
Unity Mono and the C++ Anomaly adapter. Eight focused contracts passed,
including linked and DLL FG12 ABI cases, lease faults and XeLL concurrency.
The Runtime, both carriers and Minecraft matched their export baselines.
All 112 workspace Rust tests, formatting, Clippy and binding regeneration
passed. The worker also built the optional Godot target in Debug. Device
and game validation remain pending. See [the D3D12 cutover audit](research/rust-d3d12-fg-cutover.md).

The generic D3D12 SR owner now holds its vendor and COM resources, validation
cache, history, capture, sharpening and reinsertion state in Rust. Accepted
normalized depth and motion pass directly to the Rust FG12 owner on the same
recording list. The normalization shader is byte-identical to the original.
The motion diagnostic refuses extents whose sample boxes would exceed the
texture, and a vendor success without a context fails initialization.

The integrated Release build produced the Runtime, both carriers, Minecraft,
Unity Mono and the C++ Anomaly adapter. Seven focused contracts passed,
including DLL and embedded SR12 and FG12 clients. All 131 Rust workspace
tests, formatting and Clippy passed. Release exports matched the Runtime,
both carriers and Minecraft. Three D3D12 device fixtures built and await
execution. See [the D3D12 SR cutover audit](research/rust-d3d12-sr-cutover.md).

The generic D3D12 render host now owns its session, settings, SR lifecycle,
submission slots, scene and Present matching, and status in Rust. The single
settings read preserves existing Auto and Dynamic FG values for FG12 while
the frontend and overlay continue to report the enabled flag. The native
exports retain the vendor and platform calls.

The integrated Release build passed eight focused contracts, including DLL
and embedded frontend, SR12 and FG12 clients. All 160 locked workspace
tests, formatting and Clippy passed. Runtime, both carriers and Minecraft
retained their Release exports. The Unity D3D12 fixture built. Real device,
observer, vendor and game behavior remain unverified.
See [the D3D12 render-host cutover audit](research/rust-render-host-cutover.md).

Rust now owns the shared callback render host, including plugin start and
stop, renderer configuration, SR contexts, callback leases, scene and
submission routing, settings and overlay control. The adapter still supplies
engine identity, spatial policy and its real leased render passes. The native
leaves retain the same COM, SDK and observer operations.

The integrated Release build passed 11 focused contracts, including the
callback host, render frontend, SR12 and FG12 through both linkage forms.
All 192 locked Rust tests, formatting and Clippy passed. Runtime, both
carriers and Minecraft kept their export names and ordinals. The Unity
D3D12 fixture built. Device and game execution remain pending.
See [the callback-host cutover audit](research/rust-callback-host-cutover.md).

The native D3D12 overlay now admits and retires its Present slots in Rust.
It waits only on the oldest submitted overlay fence when its ring is full,
checks the requested value after waking, and keeps the native completion
event alive if the wait times out. The Rust owner also controls visibility,
startup timing and failure state. The native renderer keeps its existing
shader, GPU resources and atlas draw calls.

Seven focused frontend, overlay and ABI contracts passed in Debug and
Release. The D3D12 overlay shader is byte-identical. All 162 locked Rust
tests, formatting, Clippy and binding regeneration passed. Runtime,
carrier and Minecraft Release exports matched. The backpressure device
fixture built. Device execution and game validation remain pending.

The native D3D12 overlay also owns atlas replacement, partial-update bounds
and texture retirement in Rust. A texture becomes current only after its
upload is recorded. Each slot holds the textures and upload buffers used by
its commands until the slot's fence is complete. Replacing or freeing an
atlas entry therefore leaves earlier in-flight draws with their own leases.
Native code issues the existing D3D12 copies, barriers and draws. The shader
is byte-identical.

Two fake-COM atlas cases passed with all 206 locked Rust tests, formatting,
Clippy and reproducible bindings. The six focused overlay, frontend and
domain contracts passed in MSVC Debug and Release. Runtime, AC7, Loader and
Minecraft Release export names and ordinals matched. The backpressure
fixture built in both configurations. Device and game validation remain
pending.

The D3D11-on-D3D12 overlay now owns attachment, startup visibility,
wrapped-backbuffer leases and stop in Rust. Drawing is restricted to the
device and queue used to create the interop object. A refused draw still
releases the wrapped resource and flushes the immediate context. The native
leaves retain the existing D3D12 render-target state on acquire and release.

Seven focused frontend, overlay and ABI contracts passed in Debug and
Release. The synthetic lease case covers refused draw cleanup and a second
queue on the same device. All 177 Rust tests, formatting, Clippy and binding
regeneration passed. Runtime, carrier and Minecraft Release exports
matched. The relevant graphics fixtures built. Device and game validation
remain pending.

Rust owns native D3D11 FG transfer slots, leases, shared-host SR evaluation,
HUD-less capture, Prepare, Present and retirement. Prepare now refuses a
contended slot as `NOT_PREPARED`. It shares the existing Rust scene and
status owners, including the former native snapshot contract. The native
CPU pacing and marker path still supplies the same timing facts.

The integrated Release build produced the Runtime, both carriers, Minecraft,
the C++ Anomaly adapter and both D3D11 FG device fixtures. All 16 focused
CPU and ABI cases passed, including Anomaly Standard and AVX input, D3D11
Present and FG tables, and D3D12 embedded linkage. All 118 workspace Rust
tests, formatting, Clippy and binding regeneration passed. Runtime, both
carriers and Minecraft export names and ordinals matched. The device
fixtures were built. GPU ordering and in-game behavior remain unverified.

The generic D3D11 host now owns session admission, callback leases,
presentation binding, SR lifecycle and reduced-frame recovery in Rust.
A refused reservation leaves the previous ticket available for its exact
retirement acknowledgement. The native exports retain the COM, DXGI,
vendor and untranslated GPL capture calls. Public and private layouts
have native and Rust layout assertions.

The integrated Vulkan-enabled Release build produced the Runtime, both
carriers, the C++ Anomaly adapter and both D3D11 host device fixtures.
Ten focused contracts passed, including both Anomaly input profiles, and
all 141 Rust workspace tests and Clippy passed. Release exports matched
the Runtime and both carriers. Device and in-game validation remain open.
See [the D3D11 host cutover audit](research/rust-d3d11-host-cutover.md).

The native SR evaluator supplies a per-call ordering receipt to the
Rust D3D11 host. Unknown leaf failures keep recovery resources leased.
The focused host regression verifies that an unordered receipt keeps
the reduced-frame ticket active. The public ABI remains stable.

Rust now owns the shared D3D11 SR pipeline, including provider sessions,
dispatch, history, quality and backend switching, and output and layer
lifetime. Resize clears a new target before publishing it. A refused
resize keeps the previous target. The native calls retain the original
shader passes and vendor operations.

The integrated Release build passed 13 focused contracts, including the
C++ Anomaly SR ABI and both input profiles. All 176 locked Rust tests,
formatting and Clippy passed. Runtime, both carriers and Minecraft kept
their export names and ordinals. The D3D11 SR device fixtures built.
Device and in-game validation remain pending.
See [the D3D11 SR pipeline cutover audit](research/rust-d3d11-sr-pipeline-cutover.md).

Direct3D CPU frame timing, routed markers, receipt accounting and pacing now
live in Rust. Each marker carries the provider generation that admitted its
source. Reflex status remains readable during sleep, and Anti-Lag 2 render
markers can complete while a later input update blocks. The Vulkan and
Direct3D paths share CPU fact lookup and limiter conversion.

All 19 focused CPU and ABI contracts passed in Debug and Release, including
both Anomaly input profiles and both Vulkan latency fixtures. All 141 Rust
workspace tests, formatting, Clippy and binding regeneration passed.
Release exports matched the Runtime and both carriers. Real provider
switching, device timing and game behavior await validation.
See [the Direct3D CPU cutover audit](research/rust-native-fg-cpu-cutover.md).

Rust also owns the Direct3D render hitch diagnostic's interval,
sample limits and thread handle. A late close leaves a different
submission's interval intact. The native CPU contract checks the
Windows sampling leaf on its own test thread.
See [the render hitch diagnostic](research/rust-native-fg-cpu-cutover.md#render-hitch-diagnostic).

The Vulkan DLSS present-capture owner now keeps queue-family command
resources, exact submission waits and retirement in Rust. The native
callback still packs the Vulkan calls and real WSI arguments. The
synthetic WSI contract checks that the copy consumes the original waits
and that the following Present carries none. The default and
Streamline-enabled Release builds matched the Runtime and embedded
export inventories. The enabled build accepted a freshly rebuilt
receipt from the pinned SDK, loaded the Runtime DLL in the export
fixture, and passed the focused synthetic Vulkan contracts.
See [the present capture audit](research/rust-vulkan-dlss-present-capture.md).

DLSS-G Present capture now checks the real image copy contract and
both WSI enumeration results in Rust. An out-of-range or missing image
leaves the original Present waits in place and skips recording. The
expanded fixture and live capture remain unrun.
[Image selection](research/rust-vulkan-dlss-present-capture.md#real-present-image-selection)

The shared Vulkan readback owner now holds selection, pending buffers,
completion proof and publication in Rust. A shared watermark cannot
complete an exact group or command retirement. Destruction fails any
remaining capture ticket and releases its readback after the caller's
drain. The regression checks that this path never maps the readback.
See [the Vulkan readback audit](research/rust-vulkan-readback-owner.md).

The shared D3D12 readback owner now manages selection, pending identity,
buffer and fence leases, completion and publication in Rust. SR12,
FG12, FSR3, Godot and the Runtime capture API use the same owner.
Destruction fails any remaining capture ticket after the caller drains
its work. The worker passed 20 new synthetic cases, 13 focused MSVC
Release contracts and seven Debug contracts. The Runtime and four
linked consumers retained their Release export inventories. The
device fixture built and its device execution remains pending.
[D3D12 readback audit](research/rust-d3d12-readback-owner.md)

The integrated hook, motion and readback build passed 362 locked Rust
tests, formatting and Clippy, plus 20 focused MSVC Release contracts.
The Godot and Unity sharpening fixture again compiles and passes. It
checks the original Vulkan SR frame offsets and the appended colour-bias
field at byte 384 in the 424-byte record. The SDK headers match the
baseline, Runtime and four linked consumers retained all export names
and ordinals, and Anomaly retained three exports with dynamic Runtime
lookup. Worker and integrated logs are under
`.local/rust-1.94.1-verification/`.

The Vulkan sharpening owner now admits passes, selects the filter and
holds command slots through retirement in Rust. The native leaves
reuse the existing SPIR-V and constant packing. The CPU fixture
reproduced a zero-height dispatch on the old owner and passed with
the Rust refusal. The integrated SDK-enabled Vulkan CPU contracts
and export inventories passed, including the 398 exports of the
statically linked Godot extension. Offscreen device checks verified
the exact readback bytes and all 30 sharpening dispatches.
See [the readback device results](research/rust-vulkan-readback-owner.md#offscreen-device-acceptance) and
[the sharpening device results](research/rust-vulkan-sharpening-cutover.md#offscreen-device-acceptance).

The D3D11 translucency owner now keeps its pass order, mask and cloud-depth
leases, device identity and publication in Rust. Cloud association uses
the reported session, rectangle and pass order. It refuses late composites
and holds the current depth in a separate slot from a new copy.

The integrated Release build passed seven focused contracts, including
both translucency ABI linkage forms and the C++ Anomaly SR API. All 204
locked Rust tests, formatting and Clippy passed. Runtime, carrier and
Minecraft exports matched. The HLSL literal matched the original byte for
byte, and both D3D11 device fixtures built. Device and game validation
remain pending. See [the translucency cutover audit](research/rust-native-translucency-cutover.md).

The native D3D11 exposure cache now lives in Rust and shares the fullscreen
pass owner with output reinsertion. It borrows a valid 1×1 R32 scalar
directly, converts supported float vectors with the existing copy shader,
and returns no exposure for unsupported or failed inputs. A failed SRV
creation preserves the previous cache entry. The format and sample checks
have a synthetic fixture. The conversion itself remains device-unverified.

Eight focused SR, FG, host and Anomaly input fixtures passed in Debug and
Release. All 89 Rust tests and workspace Clippy passed. The reinsertion
device fixture built in both configurations. The Release runtime and AC7
carrier kept all 381 and 256 baseline export names and ordinals.

Rust now owns auxiliary D3D11 SR allocation, capture, redraw, publication and
rollback. An unsuccessful readable-backup reservation releases the Main
reference before returning. The fake COM regression covers that refusal and
a subsequent reservation. The numeric colour HLSL matches the pre-port bytes
with SHA-256 `aaa2e98ef182b920c795050b022b02cad81a518abfbd1e08d1af560b4269cea7`.

Ten focused fixtures and all 90 Rust tests passed on the integrated branch.
The VS2026 Debug and Release builds included the DLL, both linked carriers,
the auxiliary fixture and rendered FG host fixture. Release export counts
remain 381, 256 and 273 with matching names and ordinals. The worker's VS2022
attempt stopped at C2026 in the existing sharpening shader string.
Device and game execution remain pending.

The native D3D11 SR evaluation sequence now runs in Rust. The native table
supplies the existing region, mask, pipeline and publication kernels. Rust
validates output extents before subtraction, scopes the scene identity and
graphics bindings, and commits history only after successful reinsertion.
The explicit FG capture path still invokes its separate existing entry.

The explicit D3D11 FG input path now sequences region preparation, motion
decode and resolve, and capture in Rust. It retains decode/resolve state for
the same device and extent and restores the immediate-context bindings.
An explicit refusal still invalidates earlier held inputs for that exact
source. The native bridge contains the existing graphics and capture calls.

Eight focused SR, FG, D3D11 and Anomaly fixtures passed in Debug and Release.
The rendered FG host device fixture built in both configurations. Release
runtime and AC7 carrier exports matched the verified 381 and 256 baseline
names and ordinals. Device and gameplay execution remain pending.

The cross-API SR bridge now owns the same-adapter D3D12 queue, command slots,
shared textures, fences and provider session in Rust. A timed-out warm-up
holds one pending submission and its provider resources. The bridge refuses
operations that would reuse or retire them until it observes ordered fence
completion or device removal. The native glue reads each COM output only
after the call that initializes it.

The integrated Debug and Release builds passed ten focused SR and Anomaly
ABI fixtures and built the SR bridge, backend switch and temporal device
fixtures. All 99 Rust tests passed, including transfer ordering and pending
warm-up retention. The Release DLL and both carriers retain 381, 256 and
273 baseline exports. Device and in-game execution remain pending.

The D3D11/D3D12 shared texture and fence factory now owns COM references
and NT handles in Rust. The native calls retain the same resource flags,
and the Rust owner releases COM interfaces before closing their handle.
Rejected peer opens leave no published object. The old source guard
omitted nine typeless or partially typeless formats. The new guard
refuses all 26 listed by the pinned Windows SDK before calling the
device. [DXGI format reference](https://learn.microsoft.com/en-us/windows/win32/api/dxgiformat/ne-dxgiformat-dxgi_format)

The pinned toolchain passed 335 locked Rust tests, formatting and Clippy,
including checks for short requests, typeless formats and cleanup on a
refused peer open. Six MSVC Release CPU and ABI contracts passed.
Runtime, AC7, Loader, Minecraft and Godot retained all export names and
ordinals. The shared surface device fixture built.

The isolated Anomaly Standard run resized from 1280×720 to 1600×900 with
the new Runtime. After the game took foreground, DLSS-G reached 240
vendor presents per 120 source frames. The last DLSS status reported
1,289 evaluations with zero refusals. The engine shut down Direct3D and
exited with code 0. The game test restored the staged inputs. Logs and
artifact hashes are in
`.local/anomaly/staging/live-58fe95a-stage5/evidence/rust-shared-surface/`.

Eight focused host, frame and Anomaly ABI fixtures passed in Debug and
Release, and all 91 Rust tests passed. The linked AC7 carrier and rendered
host device fixture built in both configurations. Device and game execution
remain pending.

Seven focused frame assembly, D3D11 host and Anomaly fixtures passed in
Debug and Release. All 88 Rust tests and workspace Clippy passed. The Release
runtime and linked AC7 carrier kept their 381 and 256 baseline export names
and ordinals. Device and in-game validation remain pending.

The native window scope stack and Present pre/post pairing now run in Rust.
The scene and window modules use the same generated ABI types. The CPU-only
window fixture covers nesting, overflow, mismatched end, COM aliases, thread
isolation, Present pairing, and scene revalidation. It also checks that a
copied scope reports the prefix size the runtime actually stored.

The integrated window, FG lease, D3D11 FG and Present ABI, and both Anomaly
input fixtures passed in Debug and Release. The eight fixtures use CPU-only
paths. All 78 Rust tests and Clippy passed. Debug runtime and carrier exports
matched the verified baseline by name and ordinal. Direct-backbuffer device
and live game verification remain pending.

The native FG status reader and overlay now use Rust to merge current
capabilities with the last eligible completed Present from the same provider
generation. The existing status assertions run against the linked Rust core,
and the FG session fixture still exercises the runtime DLL. Seven focused
status, session, window and Anomaly fixtures passed in Debug and Release.
The linked AC7 carrier built in both configurations. All 78 Rust tests and
workspace Clippy passed. No device or game run was performed.

The native CPU dispatcher now shares one Rust lock with the reservation ledger.
CPU facts, input receipts, pending messages, and marker bookkeeping use that
owner. Native sinks run after the lock is released. The small C++ bridge keeps
the Windows pacing lease and calls the existing receipt replay check under its
original license. Short structures are refused before borrowing the whole
record. Copied v1 events and receipt packets advertise their actual size.

The integrated Release DLL and both carriers match the worker's verified
baseline at 381, 256, and 273 export names and ordinals respectively. The C++
Anomaly adapter built. Fourteen focused tests passed in Debug and Release,
covering both Anomaly input profiles, receipt and reservation contracts,
native Vulkan latency with mock providers, and D3D11 window and ABI paths.
Device and in-game verification remain pending.

The D3D11 alternate SR adapter now assembles the FSR and XeSS frame record
and owns exposure settling, motion-resolve reuse and resource generation
in Rust. The existing graphics kernels still produce the dense motion and
depth. The conversion keeps previous-minus-current displacement in render
pixels, and the bridge receives the textures under the existing synchronous
lease. The Rust exposure fixture covers 30-frame settling, immediate auto
fallback and retention of the previous policy after a refusal.

Five focused SR and FG ABI fixtures passed in Debug and Release, and all 79
Rust tests and workspace Clippy passed. The linked AC7 carrier and Anomaly's
SR temporal device fixture built. The Release runtime and carrier matched
their verified 381 and 256 export names and ordinals. Alternate-backend device
and in-game evaluation remain pending.

Rust now owns the per-thread FG scene scope and the scalar identity bound by
an adapter. A pass carrying its own source keeps that identity. An anonymous
pass can borrow the bound source identity while retaining its original leased
textures. D3D11 SR restores the enclosing FG scope after evaluation. The
native host fixture verifies those rules and nested restoration.

Seven focused D3D11 and Anomaly fixtures passed in Debug and Release.
The linked AC7 carrier built in both configurations. Device and in-game
execution remain pending.

Rust now owns D3D11 sampler selection, its bounded cache and the retained COM
references. Sampler bias and FG leases share COM reference management.
The sampler prepares every replacement before publishing bindings or the
applied mask, so a late failure preserves earlier ownership.

The shared live settings store now persists the existing INI keys in Rust.
Decimal text uses the UCRT in the current locale, and sharpening defaults
come from the public C contract. A partial sharpening write attempts to
restore the previous profile values. Windows returns those values without the
original quote spelling. The native render host still reads its FG mode and
some other keys directly.

The integrated Debug and Release builds passed nine focused settings,
overlay, Anomaly input, D3D11 host and FG lease tests. All 86 Rust tests and
workspace Clippy passed. The sampler WARP fixture built in both configurations.
The Release runtime and AC7 carrier match the verified 381 and 256 export
names and ordinals. Device and gameplay tests remain pending.

Rust now owns the shared pacing selection, the process-wide Windows event
lease and Direct3D Anti-Lag 2 lifecycle and counters. The existing driver
seam still makes the vendor calls. The named event coordinates the DLL and
embedded runtime copies, and the migration preserves the caller's ordering
and locking around vendor updates.

Six focused latency, reservation, D3D11 ABI and Anomaly input fixtures passed
in Debug and Release. The mock provider fixtures covered pacing arbitration,
duplicate updates, failure and graphics API ownership. The Release DLL and
both linked carriers matched the 381, 256 and 273 baseline export names and
ordinals. All 86 Rust tests and Clippy passed. Live vendor and game execution
remain pending.

Rust now owns D3D11 composition capture and native output reinsertion.
Composition completion requires matching frame identity and the same D3D11
stream. Reinsertion prepares both outputs before publishing them and restores
the caller's bindings through the existing graphics kernels. Output and
sampler owners share COM lifetime and D3D11 device checks.

The integrated native reinsertion, presentation bridge and sampler device
fixtures built in Debug and Release. Ten focused D3D11, Anomaly input,
settings and lease fixtures passed in both configurations, alongside all 87
Rust tests and workspace Clippy. Release runtime and both carriers retain
381, 256 and 273 export names and ordinals. Device and in-game tests remain
pending.

## Fallout 4 AE native render callbacks, 16 November 2026

User priority is now fully native Fallout4.exe without F4SE/ENB first, with
explicit direct-executable/module-absence proof recorded for PID64940. Compatibility
is deferred until native validation passes, not dropped. No further ENB trials
are scheduled in the native phase.

The latest [native SR dropout trial](research/fallout4-sr-dropout-20261116.md)
deployed commit4b2fa36 in PID47776 without F4SE/ENB. The user rejects repeated
SR dropout and jitter. Final counters are3824 evaluated/778 refused;668 logged
failure transitions return to success,575 on the next native frame. Bounded
rows confirm temporal/spatial recovery switching. The first detailed failure
is a predecessor-Present timeout followed by a missing shared token. Separate
mode0/copied-joining0 refusals expose a possible same-cycle snapshot race;
neither finding explains every failure yet. Exit0 occurred before handoff
completion with no root close action, so method is unverified. All21 original
destination states were independently restored, including ENB; custom INI is
unchanged. No native SR/FG/latency or moving/shader acceptance follows.

The complete Community Shaders ownership inventory and independent Claude
inventory now cover pinned revision e0e7115 and all four consumed dependencies.
The [reconciled inventory](research/fallout4-community-shaders-inventory-root-20261116.md)
records supporting passes, shader changes and recovery beyond the SR call.
The user-supplied [Fallout4 address-library check](research/fallout4-address-library-20261116.md)
adds the actual1.11.240 database, both Ryan-rsm-McKenzie upstream references and
12 matching AE call targets. Version-specific IDs confirm the previous1.11.221
offset correction; no production offset or F4SE dependency changed.
The [shader/buffer audit](research/fallout4-full-shader-buffer-audit-20261116.md)
accounts for3939 native records, including138 unnamed HS/DS, with explicit
motion/history gaps and required producer/binding work. This is offline source
and bytecode evidence, not moving-game or reduced-quality acceptance. Fresh
REA/IDA checks verify triangle production, native order and composite branches.
The shared skill and repository instructions now require this complete reference
and shader coverage before integration completion. The new capture/status
correction and bounded diagnostics passed the full
Release VS2026 gate:224 passed,14 skipped,238 total in118.80seconds, Rust checks
clean. The hidden shared-SR device fixture also passed request retention, actual
result/raster capture, status preservation and the existing reset sequence.
Runtime22830370... and EntryBE2E0BBF... are built, not yet game-tested.

The pre-native-Poll intention correction passed the full Windows Release
VS2026 gate: 224 passed, 14 skipped, 238 total in 102.79 seconds, with Rust
checks clean and the extended production-helper handoff fixture passing in
0.01 seconds. Entry F7B95E79... was deployed in native-only PID56312. The adapter
reports a genuine selected poll intention while the core owns its wait;
strict protection expires before actual native polling. Adoption and native
FG/latency acceptance remains incomplete.

That run admits actual bound/paced/completed source snapshots and FG tags,
but the user rejects flicker and visual defects. The final retained log has
1243 active/inactive transitions,852 out-of-order and270 mutex skipped-present
warnings. All three captures remain partial with present_result-3 and no final
colour, despite successful SR evaluation. This is failed visual validation,
not FG acceptance. Normal World-to-MainMenu-to-QuitDesktop exit0 and all21
original file states were verified; controller restoration completed without
cleanup error. The [final-log audit](research/fallout4-bound-cpu-marker-audit-20261116.md)
separates these observations from the still-unproved failing source predicate
and split-image writer. Native reduced-frame recovery is now built and generic-device-tested, but
undeployed at this freeze. Compatibility remains deferred until native validation.

The subsequent [provider ownership audit](research/provider-status-worker-lifetime-audit-20261116.md)
found a generic cross-thread cached-state read and unleased session lifetime,
plus a separate worker-abort mutation race. The copied-status/lifetime and DLSS
invalidation fixes passed the next225/14 Release gate,239 total in112.89seconds,
Rust checks clean, plus explicit concurrent snapshot, hidden SDK marker and
shared-SR recovery fixtures. RuntimeE2C4958A... is built, undeployed at this
checkpoint. A missing real panel caused the first shared-SR status check to
fail; a fresh release panel and explicit path resolved that prerequisite, with
the failure retained. Equivalent FSR/XeSS mutation paths remain separate work.
The audit establishes source defects, not their causation of the reported flicker.

The combined reduced-recovery Windows Release VS2026 gate passed224/14,
238 total in113.57seconds with Rust fmt/Clippy clean. Depth-scale, auxiliary and
host device/contract checks passed0.04/0.10/0.07seconds; handoff and camera
contracts each passed0.01seconds. The explicit hidden recovery r4 fixture passed
actual temporal evaluation, real pre-submit failure followed by spatial recovery,
no-camera/frozen-settings recovery and suspended retirement. The hidden marker
fixture passed separately. Runtime95CBC864... and Entry9B5C79DF... are built and
generic-device-tested, not deployed/native recovery accepted. The authority is
`.local/fallout4-research/reduced-recovery-built-20261116.json`; exact hashes and
logs are retained in the [recovery note](research/fallout4-native-reduced-recovery-20261116.md).
Three fixture-only corrections are retained with their failures: shadowed local
rename, cold reservation expectation, and missing reactivation planning. Shipping
artifact hashes did not change across those corrections. PID56312 flicker and
missing full final captures remain rejected; moving/shader/buffer/TAA-history,
stable FG and end-to-end latency acceptance remain open.

The downstream diagnostics were then deployed in native-only PID64776.
Quality1 and return-native captures completed with matched SR0/Present0;
FG stayed inactive and no prospective source reached the new receipts.
The bounded512-row ordering trace spans0.2875seconds and cannot equate
native Present entry with the adapter's later phase arming. Reason4 remains
unlocalized. Normal World-to-MainMenu-to-Desktop exit0 and all21 original
file states were verified without a cleanup timeout on this run.

Native-only PID47344/e83bb70 completed matched SR0/Present0 captures at all
five standard scale settings:1707x1067,1485x928,1280x800,853x533 and return
2560x1600. Reduced captures retain26of40 products including final colour;
native retains16of30. Root stationary observations and four numeric crop/depth/
auxiliary replays pass for these sources, with no near-domain pixels. The
earlier duplicated image remains unresolved; the attempted absolute mouse drag
did not rotate the camera and supplies no moving validation. Foreground1 FG
remains active0/tags0; provider snapshot owner1/paced3/markers9/derived6/order0
is not rendered-source latency acceptance. World to MainMenu to QuitDesktop
exit0 was creation-matched from a retained process object. The controller close
deadline expired, then exact recovery verified21of21 entries/custom unchanged.
See [native scale validation](research/fallout4-native-scale-capture-validation-20261116.md).
The next bounded downstream-phase diagnostics passed the full224/14 Release
gate in122.40seconds with Rust checks clean; EntryD0784E75... is built but
undeployed, with no input/event policy change.

The qualified unbound-renderer poll correction passed the next full Release
VS2026 gate:224 passed,14 skipped,238 total in110.40seconds, Rust checks clean
and extended handoff contract0.01seconds. Input-unbound render ownership can
survive only a generation-only unmatched poll with unchanged native ownership;
future tickets still refuse and bound predecessors retain strict invalidation.
EntryC5D02F9C... is built, not yet deployed. Final capture, native visual and
actual bound-source/FG/latency results remain unproved.

Native-only PID71716/report180644-293 deployed4657522/Runtime22830370... and
EntryBE2E0BBF... . Quality1 actually evaluated1707x1067 to2560x1600 with SR0;
stationary/menu/focus observations were clean, without reproducing or overturning
the earlier user's visual rejection. Both captures contain25of40 products at
actual evaluation sources6226/native6668 and23292/native23733, but remain partial:
matched_present=true/present_result=-3 and no final colour. Scope receipts show
unit/full-output state through Effects/UI for sampled successes. The first CPU
refusal lacks sealed/Present; later five emitted bound-poll fact sequences abort,
without proving runtime acceptance. Native World to MainMenu to QuitDesktop
exited0. All21 staged entries and custom INI were verified restored. This route
does not repair the earlier direct-world close AV. A narrow fallback-source
invalidation correction is under review; SR visual, FG and latency remain open.

Native-only PID66836/report172137-067 deployed c32d8f7/Entry72B274B9...:
module absence and corrected worker vtable/destructor/Run predicates passed.
No RESERVE/BIND/pacing/markers were observed. The user rejected the Quality1
duplicated rectangle without ENB, so ENB is not required to trigger the visual
failure. Reduced SR acceptance is refused. The partial matched capture has
only3 native inputs at full rect after reload and does not represent that
failed reduced frame. Native Quit then Desktop reproduced4CB273/C0000005;
21/21 staged files restored, including ENB, and custom INI stayed unchanged.
Native producer and input-gate investigations continue before compatibility.

The native-only PID64940/report170725-854 predicate trace identified an exact
ServingThread guard defect: vtable255E3B8 has destructorBD72E0 at slot0 and
RunBD7430 at slot1. The adapter had mistaken slot1's address for the vtable.
The correction passed full Release VS2026 verification:224 passed,14 skipped,
238 total in117.36seconds, with clean Rust fmt/Clippy. Entry72B274B9... was then
deployed to PID66836 above; Runtime8570CCC2... is unchanged. Live binding, FG and
latency remain pending. Native module receipts prove F4SE/ENB absent. Closing
reproduced native4CB273/C0000005; all21 staged entries restored and custom INI
unchanged. Deferred foreground0 quality requests do not establish SR acceptance.

Committed0992155 ran PID10792/report164621-778 with Runtime8570CCC2... and
Entry87362E64...: native-scale SR was steady about55fps, but no RESERVE/SAMPLED/
BIND receipts appeared. FGOn/latency1 settings were accepted; FGactive0/tags0,
paced0/markers0 and absent CPU owner do not accept FG or latency. The user
explicitly rejected the duplicated ENB Quality1 scene. The ED9D inner observer
refused before hooks/file on unexpected vtable ownership, separately from that
image defect. The two-row cycle probe was inconclusive while later paused.
`post-close-disposition.json` is authoritative: observation_completed=false,
owned-window-absent refusal, actual exit0 by UNVERIFIED method,20/20 restored and
custom INI unchanged. Root close actions refused; no close AV/native-Quit claim
is made for this run. No failed native predicate has yet been identified.

The worker successor association is now built and contract-tested. The full
Windows Release VS2026 gate (`verify-worker-successor-20261116.log`) exited0:
238 total,223 passed,15 skipped in130.66seconds, Rust fmt/Clippy clean and no
failures. `fallout4_worker_handoff_contract` passed0.01seconds; the additional
skip was `anomaly_fg_dlss_sroff_device`15.34seconds, not activity acceptance.
Six frozen code/fixture/CMake hashes were unchanged. Runtime8570CCC2...,
Entry87362E64... and carrierE4A809E7... identify committed0992155, built over
07afb10; full hashes are in the engine metadata. Live native binding,
FG and latency admission remain pending. ENB-associated reduced native Main
corruption below is a separate unresolved image issue, not resolved by this gate.
See [input ownership](research/fallout4-input-ownership-20261116.md).

The complete reader-padding r2 gate passed223 tests,14 skipped in119.87seconds,
Rust fmt/Clippy clean (`verify-reader-padding-r2-20261116.log`). Retain the first
run's221passed15skipped1failed XeSS inactive/generated_windows check; an
unchanged focused recheck passed6.90seconds before the complete r2 pass.
Earlier scene/input/marker and composed-depth full gates and generic hidden/
scoped device results remain historical evidence.

Root trial `native-raster-20261116-151505-988`, PID70400, used Runtime90E86B69...
and Entry463B1EED... (full hashes in [engine metadata](../games/fallout4/engine.json)).
The semantic-field padding repair was deployed and the native framebuffer
reader accepted. `manifest.json` is complete, matched_present=true,
source4121/native4598, with16 captured images among30 entries. Native world
observation was approximately55fps. The FG-Off capture's HUDless image is
explicitly unavailable; reader acceptance and later live HUDless1 receipts do
not supply that missing image or prove full visual/motion/depth quality.

FG was explicitly requested with foreground1, base54/output54fps, active0 and
zero tags. Same-source prepare logs had inputs1/HUDless1/identity1/matched1/
scene_supported1/continuous1/render_complete1, but cpu_mask1/flags0/paced0/
prospective0, provider_generation2/owner_generation0 and reservation_read0.
This is an absent bound/paced input owner, not a resource failure. The
reserved_bound1 predicate alone is not a BIND acknowledgement on a
nonprospective source. Accepted removed-MSG and marker-order0 remain narrower
facts than prospective input, generated FG or latency acceptance.

The controller completed its capture observation, then timed out120second quit
cleanup. Ordinary AltF4 followed Escape not opening the native menu. The
`trace-native-quit.jsonl` filename does not establish native Quit: the trace
captured baseline4CB273 read0x50 with the exact baseline stack and
NtTerminateProcessC0000005. No normal/native-Quit or new SR crash-cause claim is
made. The original result restored=false is superseded by authoritative
`resumed-restoration-20261116T1420476452502Z-3103b81a21864e34ac33e15f1721a1b1.json`:
exit0,20/20 verified, custom INI unchanged, original user settings SHA13D80689...
restored after sealing the root test mutation. Native prefs/custom were preserved.

The prior PID61128 sustained native/all-quality SR execution telemetry remains
valid within its scope; this newer native-scale capture does not establish broad
quality/performance, HUDless image coverage, FG, latency or full integration
acceptance. Rendering_ready remains0. See [render evidence](research/fallout4-render-integration-20261116.md),
[native inputs](research/fallout4-native-inputs-20261116.md) and
[depth conventions](research/fallout4-depth-domains-20261116.md).
A later shared-host correction preserves temporal history across unchanged
planning calls and records copied depth domains in captures. Its complete
Release gate passed223/14 in128.36seconds; the separate hidden device fixture
verified stable backend reset0 and genuine-change/failure reset1. Runtime8570CCC2...
was then deployed in the two limited Quality1 controls below. See
[planning history](research/native-sr-planning-history-20261116.md).

The committed07afb10 planning-history batch passed the complete Release gate
223/14 in128.36seconds, Rust clean; hidden actual backend reset constants were
1,0,1,0,1,0,1,0. Runtime8570CCC2.../Entry463B1EED... were unchanged between
ENB-on PID64716 source5737/native6211 and wrapper-absent PID71412
source19050/native19505. Both complete captures matched Present/SR0 with
actual reset0 and Quality1 1707x1067 to2560x1600. The ENB-on image has nested
rectangles already in native Main; the no-wrapper active source crop and
final stationary shower image are clean. Crop/depth/auxiliary/reinsertion
numeric audits agree in both. This associates wrapper presence with the
observed defect, not ENB sole-cause proof: save labels/time, camera orientation,
jitter and history differ. Reduced coexistence, moving quality, FG and latency
remain unaccepted. CPU futurehandoff is now built/full-gate-tested as recorded above;
live admission remains pending.

Both root controls used ordinary AltF4 during the close handoff, hit the known
4CB273/null50 C0000005 baseline fault and restored20/20 controller rows.
No normal/native-Quit exit is claimed. The no-wrapper report's authoritative
post-close disposition and separate ENB control receipt confirm restored=true
and exact wrapper SHA AC194BD5... after the trial. See the
[paired reduced-quality audit](research/fallout4-reduced-quality-audit-20261116.md)
for full hashes, comparison limits and artifact identities.

## Skyrim SE experimental reduced-resolution SR, 15 November 2026

The Display creation repair passed the Windows Release VS2026 gate:220 passed,
14 skipped, fmt/Clippy clean (`verify-display-coexistence-20261116.log`).
Baseline909e17e plus the working trial ran through SKSE2.3.1 with Display
Tweaks0.5.25, matching Address Library, ENB, Steam and RTSS in bounded PID60008
(`input-acceptance-20261116-080226`): prepare/start0, exact guarded Display
route and canonical outer/inner admission verified. Final vendor telemetry
reports DLSS SR1280x720 to1920x1080, FG active/generated1,693 tags and
base116/output232FPS, with renderer/SR/FG/vendor results0 and latency effective1.
This accepts bounded vendor generation telemetry, not physical scanout, measured
latency, pickup visual quality or full coexistence. Runtime foreground1 was only
a point sample; no cause for prior inactivity is established. Observation and
CloseMainWindow completed, no Skyrim remained and four restorations were
verified, but exit_code was null. The tested Entry was then permanently deployed
with backup; Runtime and INI were unchanged. Earlier PID29080 prepare0/start-5
remains historical, with its first refusing stage unproven. See
[Display creation ownership](research/skyrim-display-tweaks-creation-20261116.md).

The ABI8 wrapper-routing batch passed the coordinator's Windows Release gate:
219 passed,15 skipped, Cargo fmt/Clippy clean, plus scoped creation and plain
bridge final-Release checks. PID20280
(`input-acceptance-20261116-020919`) completed bounded SR/provider-switch
observation and normal close: exit0, ExitProcess0 with no observed AV, clean
observer detach and four hash-verified restorations. This exercises the ENB
outer-chain path with Steam typed continuation routing; it does not establish
generated FG, visual correctness or complete mod coexistence. Same-source
FG_INPUTS/WINDOW samples1152/1280/1408/1536 have adapter flags0x6, eligible1,
bound1 and refusal0, but the core retains flags22 including NO_FG despite the
matching admission facts. The subsequent explicit-provenance repair passed
`verify-explicit-fg-provenance-20261116.log`: exit0,219 passed,15 skipped and
fmt/Clippy clean. Its focused device regression passed53 sources,39 captures,
37 SR evaluations and33 tags/callbacks, including genuine sticky refusal.
PID52400 (`input-acceptance-20261116-021712`) now retains matching flags6,
continuous1 and valid1, with317 tags; PID47748 (`20261116-022508`) records316
tags. Both exited0 and restored four files. Generated output remained inactive,
so tags do not establish full FG acceptance. The latter foreground experiment
left WindowsTerminal PID35804 foreground and supports no focus conclusion.
See [close-chain routing](research/skyrim-close-chain-routing-20261116.md)
and [Steam continuation](research/skyrim-steam-present-continuation-20261116.md).

The INPUT_TIMELINE connection has bounded game evidence in
`.local/skyrim-research/input-acceptance-20261115-231558`: DLSS FG active1,
Reflex effective1, 333 tags/676 provider presentations, normal exit0 and verified
restoration. This does not establish measured latency, every input path, XeSS FG
or pause/load/menu admission. The latest parity run PID5484
(`parity-acceptance-20261115-234222`) completed F10, exited normally and restored
all four files. Native target85 R8G8_UNORM1920x1080, cropped to1280x720, matches
both R32 reactive/bias masks exactly under clamp(0.1R+G), maximum error0 and
83,422 nonzero pixels. Colour transport is off and source/backend colour match
exactly. Physical client1920x1080 matches output at144DPI. The Survival Mode
modal remained, and FG SceneNotEligible with no tags is expected refusal,
not gameplay FG or pickup acceptance. The earlier PID44520 partial capture had
unsupported native mask dumping; a still earlier trial failed before prepare
with a stale carrier ABI. Both corrections remain in the parity note.

The combined review-fix batch passed Windows MSVC Release verification with
VS2026:220 passed,14 skipped, exit0, Cargo fmt/Clippy clean
(`.local/skyrim-research/verify-skyrim-opus-fixes-20261115.log`).
Optional-mask fallback, CS initializer capture, CPU disable/release retry
handling, resolution restoration, SR-only NO_FG, guarded reads, service snapshot
and hook rechecks, the narrow sampler early-out and shared raw-mask dump repair
are built and covered by that gate plus the bounded resource run above. That
run does not exercise every rollback/refusal or compatibility branch. Optional
architecture proposals remain intentionally out; major-mod coexistence
validation is next and remains unverified.
See [parity inputs](research/skyrim-cs-parity-input-wiring-20261115.md)
and [review fixes](research/skyrim-review-fixes-20261115.md).

Skyrim now reports native pre-HDR SR/FG inputs, a HUD-less scene before UI,
and the window/context lease through actual Present. Host ABI4 wraps the final
swapchain at the guarded outer creation call; frame ABI2 and camera ABI1 stay
separate. The bounded PID50256 game run passed DLSS Quality1280x720 to1920x1080
and FSR activation through repeated prepare/Present/retirement, with normal exit.
Its runtime sample reported106tags,212provider presentations and58/116FPS.
PresentMon could not acquire ETW access, so physical display cadence and latency
are unverified. After validation, the exact tested Runtime/Entry and matched FSR
DLL set was installed with hash-verified backups; settings were preserved.
Automatic game shutdown is now disabled while the user captures.

The completed-frame overlay fix now reports active FG in bounded game runs;
FG Off and SR Off/On checks also passed. Claude's native source reservation,
adoption and binding report observed worker movement without mismatched sources.
These observations do not establish complete input consumption or pacing.
Genuine pre-input pacing, DLSS/XeSS FG,
CS/ENB handover and moving/all-material acceptance remain incomplete. Native
recognition and particular test runs do not change general readiness flags.

The device regression passed51sources, including reduced in-place DLSS followed
by live FSR enablement, with32tags/32callbacks and zero input/pacing/PCL claims.
The earlier232-test gate's two FSR failures were isolated to an old staged DLL;
both cases passed with the matched local4.0.2 DLL. The final Windows Release
gate passed: 220 tests passed, 14 skipped, zero failed; Cargo fmt and Clippy passed.

The third-person draw capture verified actual motion bindings and byte-exact
previous/current bones for three skins, including valid indexed weight ranges.
Claude's offline census now accounts for all16,044 shader containers, including
the233-container tail, against CS latest038a0a04 and the pinned release.
Riverwood captures include Effect motion and correct draw-time rigid history
for carried firewood. Pickup-only ghosting remains unresolved; steady carrying
does not verify the transition. No shader replacement was justified by these
draws. Remaining family/pass coverage is listed in the live draw research.

Independent offline review of the user's F10 frame confirms the native motion
crop, decode and pixel scale. It also establishes real differences from CS in
colour/exposure, motion conditioning, masks and model selection. None is yet a
verified pickup fix; this single frame cannot establish an animation trail.
See [SR reference parity](research/skyrim-sr-reference-parity-20261115.md).

The later poll-entry, consumer-refusal, CS and prospective-pacing batch passed
the Windows MSVC Release gate: 220 passed, 14 skipped, zero failed, with Cargo
fmt and Clippy passing. The latency_host device check was skipped, so this is
not runtime pacing acceptance. The batch implements token acquisition and pacing at RESERVE,
actual poll-entry promotion at BIND, and CS pre-initializer Upscaling exclusion.
That earlier batch left INPUT_TIMELINE disabled; the later bounded connection
and its acceptance limits are recorded above.
CS HDR wrapper forwarding is implemented, but float16 input readers, applied
colour-space facts and its post-composite HUD-less lease remain unfinished.
The final static pre-submit gap was resolved as IOManager's pure field getter.
No game interaction or deployment accompanied this verification batch; evidence
is retained in `.local/skyrim-research/verify-skyrim-prospective-cs-batch-20261115.log`.

See [FG handoff](research/skyrim-fg-handoff-20261115.md),
[core capabilities](research/d3d11-host-fg-capabilities-20261115.md),
[input ownership](research/skyrim-input-ownership-20261115.md),
[live draw coverage](research/skyrim-native-draw-coverage-20261115.md),
[prospective sources](research/skyrim-prospective-source-20261115.md) and
[camera ABI compatibility](research/unity-camera-frame-abi-20261115.md).

The shared SR regression now has actual device evidence: the explicit hidden
D3D11-host check passes eight shared DLSS evaluations and output readback on
the RTX4070, with zero D3D11 debug errors and FG Off. The identical executable
with the saved failing Runtime reproduces backend-9/pipeline-10. This check
is excluded from routine builds/CTest. No game deployment was made. See
[device regression evidence](research/skyrim-se-integration.md#shared-sr-transfer-device-regression-15-november-2026).

Native input sinks and the actual third-person camera vector consumer are now
mapped against the matching executable. A bounded live job-order probe captured
zero generations and establishes no consuming-frame association. Final-scene
metadata now retains the SR camera, convention and screen without borrowed
resources. Focused Release adapter build passed; no deployment or new FG
readiness is claimed. See
[input consumer research](research/skyrim-se-integration.md#input-consumers-and-final-scene-metadata-15-november-2026).

The shared DLSS route now normalizes camera-inclusive motion/depth into
R16G16 pixel displacement and R32 device depth before cross-API transfer.
Actual sentinel presence preserves valid zero vectors. Focused MSVC Release
builds and WARP numeric/format plus frame-assembly checks pass. This correction
is not deployed: the restored working game remains untouched and cold FG
presentation stays disabled. See the shared input correction in
[research](research/skyrim-se-integration.md#shared-dlss-input-normalization-correction-15-november-2026).

Cold FG presentation preparation is built behind the generic D3D11 host ABI2,
with native FRAME/SUBMISSION facts and an adapter window filter. The experiment
exposed duplicate Present interception, then failed combined DLSS SR transfer.
The user-visible SR regression was rolled back: the working pre-bridge DLL pair
is installed, PID10028 has successful DLSS Quality capture/result0, and the
Skyrim source leaves the bridge disabled until verified. No FG readiness is
claimed. See the
[bridge experiment and recovery](research/skyrim-se-integration.md#cold-presentation-bridge-experiment-and-sr-rollback-15-november-2026).

Native update identity now follows the synchronous render invocation into SR,
final scene and a window lease spanning actual Present. The shared D3D11 host
records these existing C ABI facts without enabling generation or pacing.
Live source541/submission541/native711 capture matches the scene/window/Present
path; after-interface bytes equal final Present. Focused Release builds, host
and Skyrim contracts and the updated capture check passed. NO_FG and unknown
screen remain. Worker input, primary submission markers, retained FG inputs and
UI extraction still need implementation. See
[source/Present research](research/skyrim-se-integration.md#native-update-final-scene-and-present-association-15-november-2026).

Native input dispatch now reports delivered keyboard/mouse/controller kinds
through the existing unframed CPU event API. It copies bounded native headers
before dispatch and emits only the kind mask after the original returns.
Worker input is not assigned a guessed source frame. Native task submission,
manager joins and concurrent callback claims are now traced; consuming-simulation
ownership remains open. Focused Release build, input contract and Skyrim plugin
contract passed. Adapter-only deployment accepted startup; positive delivered
input reporting remains unverified. Same-frame QASmoke capture now proves the
interface boundary: HUD absent before, HUD present afterwards, after-interface
bytes exactly matching actual final Present. Menus, UI-layer extraction and
FG source ownership remain open. FG and pacing remain disabled.
See [input ownership research](research/skyrim-se-integration.md#native-delivered-input-kinds-and-task-ownership-15-november-2026).

The adapter now has a guarded native interface boundary hook for F10 research.
It captures the backbuffer before/after native interface drawing using the
existing same-frame capture lease. Focused Release build and plugin contract
passed. Subsequent adapter-only deployment verified the QASmoke HUD boundary
as recorded above, without enabling FG. See the native interface boundary section in
[research](research/skyrim-se-integration.md#native-interface-boundary-capture-for-fg-research-15-november-2026).

The user reports a visible improvement after the direct DLSS motion correction.
Continued audit verified1997 native CPU bone-history samples and four actual
VS b9/b10 GPU buffer pairs against their CPU arrays. Two active Lighting VS
bytecodes consume both histories. All74 small-family archive shaders were
reviewed, including separate grass wind timers and Particle target1 normals.
Final skinned draw ranges/writes and broader material coverage remain open;
the final-draw observation window captured no draws. This research-only
checkpoint required no rebuild or fixtures. See the live skeletal audit in
[research](research/skyrim-se-integration.md#live-skeletal-history-and-small-family-audit-15-november-2026).

The third-person idle/orbit smear and grain on props exposed another defect:
canonical motion was passed directly to DLSS with reversed direction. Comparison
against Community Shaders and NVIDIA's current guide confirms native vectors
need positive Streamline scale. The generic direct DLSS path now converts once;
already resolved and legacy paths retain their existing scales. Focused Release
build/contract passed, and a settled deployed capture verifies motion_scale(1,1).
The user subsequently reported improvement; native wall motion agreement still
does not prove skin or complete temporal quality. See the latest research.

The native sampler path now supplies resolution-derived material mip bias through
a generic runtime-owned sampler cache. A guarded state-flush hook limits changes
to reduced main colour/depth draws and restores native bindings outside that scope.
Comparison/no-mip filters and foreign bindings are preserved. Focused Release
builds and one WARP sampler check passed; the game logs bias−0.584962 at DLSS
Quality1280x720, with a settled complete capture. Moving/all-material coverage,
active shadow/water/refraction and CS/ENB validation remain open. See the native
material sampler correction in [research](research/skyrim-se-integration.md).

The following preserves the preceding jitter correction and its then-open gaps.

User reports of XeSS smearing and blurry DLSS exposed an inverted temporal
jitter sign. The adapter now matches native projected geometry displacement;
auxiliary sampling keeps its previous source mapping. The focused camera
projection check and Release build passed; the fix is deployed. Settled FSR4
Ultra Performance captures have reset0; corrected XeSS/DLSS moving quality
remains unaccepted. Material mip-bias adjustment is still missing and requires
a verified native consumer. Native bone history production and separate b9/b10
uploads are now traced, without claiming live skinned coverage or shader repair.
See the research's temporal-jitter and skeletal-history corrections below.

Reduced DLSS now runs through a transactional auxiliary handoff. Depth/stencil,
normals and motion are reconstructed for native postprocessing and restored at
RendererEnd; underwater-mask retirement preserves the native DOF clear. The
reported quality-change zoom came from stale native per-frame resolution constants.
The adapter now explicitly refreshes those constants and the viewport at the
output/producer handoffs. Same-frame SR/final captures at Quality and Performance
preserve full-view landmark placement at 1920x1080. Off restores ratios, lock and
native TAA. Focused Release builds and the auxiliary WARP fixture passed; no full
suite was rerun. Active water, shader motion coverage, other SR backends, FG,
latency and CS/ENB remain unfinished. See the latest
[research correction](research/skyrim-se-integration.md#reduced-sr-auxiliary-handoff-and-zoom-correction-15-november-2026).

The following records the preceding native-only checkpoint and its failures.

Native captures now establish the auxiliary allocations and show that the DOF
average-depth input aliases a 1x1 mip of the eleven-mip SAO texture. Camera-Z
matches projection-linearized depth, not hardware Z. Dry-room underwater and
refraction buffers are zero; active coverage remains unverified. Generic capture
readback has session/view/frame guards. The reversible depth/stencil primitive
is built and WARP-tested but not connected. Reduced SR remains gated until the
other buffers and their consumer lifetimes are ready. The shared overlay also
has a foreground Insert polling fallback with a shared held-key edge, replacing
the missing route in this host. Startup window/UI binding is independent of SR
scene admission and follows the loading-to-main presentation owner. The adapter
sets DPI awareness before window creation; pointer polling supplies missing mouse
messages. The user confirmed DPI and mouse controls working at 1920x1080.
See [research](research/skyrim-se-integration.md), including the motion/input
coverage audit. The last full gate had 213 passes, 14 skips and the same two FG
failures out of 229, before the final UI fixes. Final fixes were Release-built
and directly game/user-tested; per-edit full fixtures were stopped at the user's
request. The integration, SR at reduced size, FG and latency remain unfinished.

Native resolution production is now controlled at the guarded State updater and
jitter boundary. The runtime prepares a backend size plan on the verified scene
graphics owner before geometry; the adapter applies its ratios and jitter.
A bounded Quality-mode probe rasterized 1141x699 into 1711x1048 allocations.
Viewport observations and raw depth prove geometry fills that active rectangle
and leaves clear depth outside. Reduced modes currently collect inputs through
native fallback; vendor SR is still admitted only at native resolution until
depth/stencil and post-processing auxiliaries are repaired. These modes are
experimental, not completed reduced-resolution SR support.

An early loading-thread startup attempt stalled and was terminated after a
noninvasive stack snapshot. Cold preparation now refuses before binding graphics
ownership, and planning waits for the main scene's identified execution thread.
The first scale attempt also exposed that the native resolution lock is a
nesting counter, not a boolean; lock=1 kept the primary scene full size. The
corrected unlocked scope produced the verified reduced raster. Off restored
current/previous ratios and native lock; native quality/on resumed DLSS with
a complete actual-Present capture. The regression fixture covers cold planning
refusal without touching a graphics context. See the research's resolution section.

Latest resolution-owner gate: 209 passed, 15 skipped and the same two Anomaly
FSR3 FG device failures, out of 226. The extra skip was a foreground-denied
DLSS-G activity fixture; fourteen are default opt-ins. Formatting and Clippy
passed separately. The full gate remains failed.

The exact-build no-SKSE adapter now leases native scene colour, depth, motion,
main camera and actual swapchain synchronously at the guarded pre-HDR call.
The shared D3D11 host evaluates DLSS at native resolution and returns its output
to the R11G11B10 target. The native HDR chain continues, with TAA bypassed only
after successful replacement. Native history is marked for clearing before a
later fallback. This remains experimental and rendering_ready stays zero.

Two bounded QASmoke runs evaluated DLSS successfully. A complete raw capture
joins native/backend inputs, reconstructed output and actual native Present:
1711x1048, standard Z, unjittered camera-inclusive motion, settled reset=0,
source_frame_id=null and NO_FG. Stationary motion is zero throughout; the
reinserted output is finite and differs from its source. This proves evaluation
and reinsertion, not moving-game quality, a speedup or reduced-resolution SR.
The first attempt failed NGX initialization because the project identifier was
not a GUID; correcting the identifier enabled initialization. The first capture
had offset render-ID bookkeeping and a misleading motion preview; the corrected
capture has matching native identity and completes at actual Present.

Host configuration, vendor selection, bindings, format conversion, SDK ownership,
settings and shared overlay remain generic. SR backend/preset, sharpening,
colour transport/correction and capture controls use the existing panel.
The adapter supplies native conventions and accepts only full native extents.
Camera conversion and graphics-free host ABI tests pass; the capture fixture
rejects mismatched render IDs and invented source IDs.

Reduced resolution, portable host startup without DLSS, device recreation/full
retirement, moving/skinned/transparent motion, shader repairs, job-owned input,
HUD-less FG/UI, measured latency and CS/ENB coexistence remain incomplete.
CS activation is still refused until ownership handover exists.
See [research](research/skyrim-se-integration.md) for captures and validation.

Earlier SR checkpoint gate: 210 passes, 14 opt-in skips and the same two Anomaly
FSR3 FG device failures, out of 226 discovered checks. The full gate remains
failed. Skyrim, camera, host, capture, overlay and reinsertion checks passed;
formatting and Clippy passed separately. No Rust behavior was changed.

### Earlier observer checkpoints

The no-SKSE exact-build adapter and generic XInput carrier build on Windows.
A bounded vanilla run prepared/started the runtime session and produced sixteen
readable samples from the native jitter producer, including the expected state
RVA and eight-frame sequence. No SR, FG, shader repairs, latency policy or CS/ENB
compatibility has been implemented or accepted. CS currently refuses activation.
The code-level readiness flag remains zero.
IDA also matched the native VS/PS/CS readers and package initialization; 15,811
original shader containers now have native family/stage/permutation identities.
The saved research database retains named producer functions. Runtime vector
coverage, source replacements and missing-motion corrections remain unimplemented.

The native pre-HDR caller and manager processor are now matched in IDA at
`0x153AC00` / `0x14EDBF0`, with the guarded call at `0x153ADE7`. A bounded
starting-cell run without desktop input produced sixteen pass-through calls:
native main colour is R11G11B10_FLOAT and motion candidate is R16G16_FLOAT,
both 1711x1048. No pixels, vector units or GPU leases have been validated.
The temporary starting-cell custom INI was removed and the game closed gracefully.
HDR, bloom, exposure, native TAA and dynamic-resolution processing are preserved.

Native camera construction/history/binding and main-depth ownership are now
matched in IDA. A further bounded run matched the main camera to its jitter-keyed
cache, read unjittered projection/history/origins and R24G8_TYPELESS main depth,
and exposed zero initial history components. Initial history is not yet validated.
Reference State frame-count/flag names do not
match this executable's jitter floats and cannot establish frame identity.
Original DXBC arithmetic establishes selected written-motion conventions and
literal target-1 repair candidates; live draw/binding coverage remains open.
GPU leases, shader repairs and vendor evaluation are still unimplemented.

The adapter now reports native update begin/end and render-entry CPU facts through
the existing game API. Both bounded runs carried update sources into pre-HDR,
and the runtime accepted the first 32 emitted facts. Gameplay input polling was
observed on seven threads outside that thread-local scope, so worker input-to-
simulation ownership remains unresolved and the runtime uses labeled substitutes.
No vendor sleep or latency policy was added to the adapter or carrier.
Shared SR reinsertion now converts floating HDR formats into offset native targets
and mirrors, preserving linear values, alpha and inherited GPU state. Its WARP
fixture passed; this is not an SR evaluation in Skyrim.

[Research](research/skyrim-se-integration.md) records the exact executable,
current references, original shader inventory, matching CS release/PDB, hook
bytes, deployment and run. [Engine metadata](../games/skyrim-se/engine.json)
distinguishes observation from processing. The next implementation work is GPU
pass ownership, complete temporal inputs, CS ownership handover and shader repair.

Validation: Skyrim ABI and generic/existing Wingman XInput contracts passed.
The latest Release VS2026 gate reached 224 tests but two Anomaly FSR3 FG device fixtures
failed, and reproduced in isolation. Three carrier/Skyrim checks passed in the
focused rerun. Formatting and Clippy passed separately. The full gate remains
failed pending those device failures; no Rust behavior was changed.

## Runtime shader patching for Anomaly, 12 November 2026

Core source patching, .rsfpatch loader/build-inspect CLI, original DXBC proof,
cache/reflection and D3D11 interception are implemented. Anomaly supplies its
engine includer and RSF-authored rules. Native parity: all 454 shader outputs
match the Python merged output byte for byte. Synthetic/WARP: source proof,
refusals, cache and a same-length DXBC instruction edit/readback pass.
The new provider/reload/pack path is not game-tested and nothing was deployed.
Full gate results and remaining acceptance are recorded in
[the gate report](research/runtime-shader-patching-gate.md). Release VS2026:
201 native tests passed, 12 skipped; fmt/Clippy and 73 Rust tests passed.
No readiness flag changed.

## Touch-first overlay panel, overlay ABI 16, 12 November 2026

The shared panel is rebuilt for handheld touch screens from the agreed mockup. It is a
full-height sheet with a tab rail (Upscale, Frames, Sharpen, Game, Display) and a status strip
of what is in effect. Choices are segment buttons, switches are full-width rows, and values use
−/+ steppers that repeat while held and apply once on release. Every target is at least 48
points. Pending and refused states sit next to their control, and backends or providers that
are not offered are named. Overlay ABI 16 adds `stats.ui_preferences`, an opaque layout word
(panel size, side and FPS overlay detail), and `intent.close_requested` and
`ui_preferences_changed`. The overlay hosts close the panel on `close_requested` (the close
button, or a tap that starts and ends outside the sheet). The shared overlay service, the D3D12
render host and Unity save the layout word as `OverlayLayout` in their existing INI section. The
AC7 carrier keeps it for the session only, as it already does for the FPS overlay switch. The
input layer opens and closes the panel on a three-finger tap
(WM_POINTER, or WM_TOUCH when no pointer messages arrive). It tracks the tap while hidden
without consuming touches, and cancels the panel's own contact when a second finger lands.
The render and output resolution in the status strip come from the runtime's stats, not from
a per-level estimate. Rust: 54 overlay tests, including a 44-point touch-target check over every tab, plus
fmt and Clippy. Debug VS2026: 215/215 native tests passed with the rebuilt panel DLL. Not
game-tested or touch-device-tested. Whether Proton/Wine on the Steam Deck delivers three-finger
WM_POINTER or WM_TOUCH contacts is unverified; Insert still opens the panel there. Steam Input
can bind Insert to a back button.

## Anomaly SR Stages 3-5 and foliage motion, 12 November 2026

SR Stage 3 adds Halton jitter to the world and the six first-person projections once each, and
a world-window mip bias, all restored on every exit; a static scene under jitter gives 0.003 px
worst motion in the device fixture. Tree and grass motion history uses a CDetailManager window
hook and the engine's staged VS constants. SR Stage 5 evaluates the shared RSF pipeline at the
seam through a new generic `rsf_get_d3d11_sr_api` export. The generic pipeline gained validity
sentinel handling and a `standard_depth` option for FSR/XeSS (pipeline ABI 5; AC7 unchanged).
Fixtures: DLSS, FSR3, FSR4 and XeSS evaluated 24/24 frames each on the RTX 4070; the D3D11 to
D3D12 shared-host and RDR2 host fixtures passed. Release VS2026: 192 discovered, 181 passed, 11
optional skips, fmt/Clippy. Stages 3-5 and foliage motion are not yet game-tested.

## Anomaly Stage 2 and motion game run, 12 November 2026

At 3ac6cb0, SR Stage 2 and Motion were accepted in game with their counters consistent and 0
faults: scale 1.0 matches stock, 0.67 and 0.5 are registered and only softer, motion vectors are
plausible and the motion target follows render size. PDA, scope, item-attached UI, fluid and a
moving NPC were not exercised; the laptop is CPU-bound, so no GPU gain was measurable. The stock
engine crashes with `r2_ssao_mode` default plus `r2_ssao_blur` even with RSF disabled.
Evidence: `.local/anomaly/wave5-game/result.json`.

## Anomaly SR Stage 2 and motion MRT, 12 November 2026

The ae714a2 game run accepted the overlay panel (seen through adapter captures, click applied
and persisted, relayout on resize) and SR Stage 1 (counters consistent, 0 faults, images
matching stock within run-to-run noise; the glass-pane difference is stock brightness drift).
SR Stage 2 (`SRStage=2`, `SRScale`) renders nine world targets, depth and the viewport at render
size and upscales once at the seam; occlusion queries get a render-size stand-in RT and the SSAO
static viewport is swapped. `Motion=1` recompiles allowlisted shaders byte-exactly, swaps in
motion variants per draw with b13 history, keeps RTV2 bound and replays unpaired occluders as
zero motion. Release VS2026: 183 discovered, 172 passed, 11 optional skips, fmt/Clippy. Stage 2
and Motion are fixture-tested only. [SR seam](research/anomaly-sr-seam.md).

## Anomaly overlay capture and SR Stage 1, 12 November 2026

The d75a881 game run configured and activated the overlay with working DirectInput gating, but
the panel could not be seen: desktop captures are black and F12 is blocked while it is open.
An opt-in presented-frame capture, pointer/apply logging and a deploy that ships the current
overlay DLL were added; the panel now hides SR controls when no SR choice exists. SR Stage 1
(`[Anomaly] SRStage=1`) swaps generic0/generic_temp to FP16 twins from phase_scene_prepare to the
seam and writes the stock tonemap back through an RSF round-trip shader, with guarded hooks and
an opt-in seam capture. Release VS2026: 178 discovered, 167 passed, 11 optional skips,
fmt/Clippy; Rust workspace tests passed. Stage 1 and the panel are not yet game-tested.

## Anomaly Observe run, overlay wiring and SR seam, 12 November 2026

Observe mode ran in Anomaly: exactly-once startup before device creation, all guarded and
geometry hooks installed, 8040 frames with no fault or reset (`.local/anomaly/wave2-observe/`).
Only ~5.5% of draws were identified; the gap is explained and per-category counters, a guarded
draw-time hook and view/first-person context producers were added. The adapter now wires the
shared overlay with DirectInput gating, with SR/FG reported unavailable. The SR seam design
chooses the phase_sunshafts/phase_blur prologue with FP16 render-size targets and an exact
inverse/forward tonemap; all Stage 0 sites are byte-guarded and verified against the exe.
The shared motion decode gained an optional validity channel. Release VS2026: 173 discovered,
162 passed, 11 optional skips, fmt/Clippy. Overlay and coverage are not yet game-tested.
[Progress](../games/anomaly/PROGRESS.md), [SR seam](research/anomaly-sr-seam.md).

## Anomaly wave 1, 12 November 2026

Resumed on request. The geometry producer is audited and built into the adapter with
two registered contracts. The Runtime exports a generic `rsf_get_overlay_api` with DLL-import
and hardware D3D11 fixture tests. The Debug Godot CRT mismatch is fixed at its CMake cache
source. Release VS2026: 168 discovered, 157 passed, 11 optional skips, fmt/Clippy. Built and
synthetic-tested only; nothing was deployed or game-tested. [Progress](../games/anomaly/PROGRESS.md).

## Anomaly checkpoint, 12 November 2026

Implementation was paused for the day. The detailed acceptance matrix, build/device evidence,
installation cleanup, overlay requirement and ordered resumption checklist are in
[`games/anomaly/PROGRESS.md`](../games/anomaly/PROGRESS.md). Full in-game SR/FG and overlay
integration remain incomplete. No RSF DLLs or shader replacements were deployed to Anomaly.

## Anomaly DLL-only integration research, 11 November 2026

Source and primary-vendor documentation review identified the PDA backbuffer-copy producer,
once-per-frame UI guards, separate 3D item UI, and non-presenting scope frames. World/output
resolution separation and a linear-color SR seam still require renderer changes through guarded
DLL hooks; generic scene intermediates are currently 8-bit LDR. Installed DX11/AVX fingerprints
and the DirectInput import identify entry candidates, not working hooks.

RSF has reusable D3D11 SR/FG mechanisms, but the inspected native FG path does not populate a
separate UI resource, and the combined carrier still includes AC7/orchestrator components.
At the research-only checkpoint, no native code, game binary, shader cache or engine source had
been changed, and no Anomaly build, injection, SR/FG or visual acceptance had been performed.
[Evidence, vendor references and proposed first gate](research/anomaly-world-space-ui.md).

### Subsequent native work (in progress)

The Runtime DLL now exports a size/version-checked `rsf_get_d3d11_present_api` table for
22 existing presentation-bridge operations. The adapter can reach the existing owner without
embedding another runtime. C++ and C11 real-import tests are registered in the main CMake test
suite; both passed after a fresh main-preset Windows/MSVC Debug build. The isolated worker's
Debug and Release logs also record two passing contracts per configuration. This proves ABI
access and refusal behavior, not graphics initialization or in-game presentation.

The thin carrier's intentionally malformed runtime/adapter fixture exposed a Windows loader
popup. Library-controlled loads now apply temporary thread-local error suppression and restore
it before adapter entry. An independent native host with process/thread modes initially zero
verified both malformed-image refusals: no dialogs, original modes restored, error 193 logged,
and genuine system forwarding preserved. Evidence: `.local/anomaly/modal-regression/result.json`.

The full Windows/MSVC Release verification subsequently passed: 163 discovered native tests,
152 passed and 11 optional/device-dependent skips, followed by Cargo formatting and Clippy.
The full Debug build failed on a Godot/runtime CRT/iterator-debug-level mismatch; focused
Anomaly/loader Debug contracts passed. Later shader/geometry worker additions still need main
registration and a renewed full gate. Evidence: `.local/anomaly/verify-{debug,release}.log`.
Anomaly SR/FG/PDA/scope/overlay gameplay acceptance remains pending.

CPU temporal-history/planning contracts and shader inventory/staging tests are now registered in
the main suite. Fresh main-preset MSVC Debug execution passed all 17 internal temporal cases,
the expected-failure unknown-selection check, and all eight Python shader-tool cases, including
the local FXC byte-identity regression. CPU history and declaration-only shader staging do not
produce GPU motion or establish a usable SR seam. The reviewed r3 tree contains 449 files;
55 geometry-stage roots are identified, 44 stage roots remain unclassified, and one tree wrapper
has a blocked recursive include. See `tools/anomaly_shader_README.md` for source evidence.

## FidelityFX Vulkan static line width, 8 November 2026

The SDK export recipe now reproduces the two approved zero-to-one line-width
edits from `d590225`, keeping the original source pin and all previous corrections.
Both full-file hashes are checked before either export is patched. Schema 2
receipts bind exact correction/source identities and reject stale or mutated
exports; Godot staging/build identities include the correction.

The complete Release SDK/API runtime built with VS2026. RSF verification passed
(92 executed, 11 optional skips), three actual SDK graphics pipelines passed with
`wideLines` disabled, and 24 FSR2/FSR3 SR frames passed pixel/reset/ownership tests
with zero core/synchronization errors. The initial long-path build failure is
preserved. Built and synthetic/device-tested; guarded WSI/FG, game and VR
acceptance remain separate. [Research, artifact path and limits](research/fidelityfx-vulkan-line-width.md).

## Public FSR4 Vulkan recording API, 8 November 2026

The generic `native_sr_split_vulkan.h` C ABI exposes the existing FSR4
Vulkan/DX12 split-record owner through create/plan/evaluate/checked destroy,
with size/version checks, explicit not-compiled refusal and leased continuation
preservation. Native SR ABI 1 and existing SR/FG/PCL/provider selection remain
unchanged. The Runtime DLL exports the four public calls and keeps the embedded
bridge's legacy entry points private. No integration policy was added.

Built with Windows VS2026 Debug: `eng/verify.ps1 -VS2026` passed (102 discovered,
91 executed, 11 optional skips, fmt/Clippy). C11 and DLL-import contract tests
passed. The public route completed six 426x240 to 640x360 evaluations on RTX 4070
Laptop GPU with INT8 compatibility, six duplicate-frame refusals and zero
core/synchronization validation errors. An initial default-adapter create refusal
is preserved separately. Built, synthetic-tested and GPU-fixture-tested;
no game, visual-quality, stereo, FG, RTSS or headset acceptance. DLSS retains
single-owner/viewport-one SR. [Contract, evidence and limits](research/public-fsr4-vulkan-api.md).

## Latency owner in the overlay, 30 October 2026

On Intel the overlay greyed out Reflex and the frame limit because both sat under the provider's
`low_latency_available`, which the XeSS provider never set, although it already runs XeLL sleep and
markers and the orchestrator passes the frame limit to XeLL's `minimumIntervalUs`. The XeSS
provider now reports availability once its XeLL context is bound. Both XeSS and DLSS providers
count frames their pacing owner accepted (`rsf_fg_status.latency_paced_frames`). Overlay ABI 14
adds `latency_pacing_owner` and `latency_paced_frames`. The panel shows the Reflex radio only for
Reflex and shows a live XeLL status line for XeLL, with the frame limit enabled for either. The count
is evidence that the SDK accepted sleep calls, not a latency measurement. Rust 41+19 tests,
Clippy and fmt with the pinned 1.94.1 toolchain (rustup installed on this device); C++ 84/84.
The save-and-reboot round trip for the refresh fix passed: after an in-game apply and normal exit
`system.xml` holds `12000/100` at index 5.

## RDR2 on Intel: settings-change crashes and the 30 Hz boot, 30 October 2026

Three faults surfaced once SR ran on the Arc B370; each has a log or dump behind it.

- **VSync toggle crash (ERR_GFX_STATE).** New per-step Present logging showed
  `physical GetBuffer failed 80070057` right after the game resized its chain to 2 buffers. With
  runtime switching the application renders into `render_buffers`, so the physical chain now
  keeps its created count (`ResizeBuffers(0, ...)`). XeSS FG's proxy had kept an index past the new
  count. Change in `runtime/presentation/src/d3d11_present_bridge.cpp`.
- **Upscaling-preset crash.** Crashpad dump `0e3dc899...`: AV in `ffx_fsr2_api_dx12_x64.dll+3163`
  calling `GetDesc` through `RDR2.exe+5730b08`, reached from the RSF native-FSR fallback. The
  game's FSR library is 2.2 (`r_input_opaque_only`, `rw_output_autoreactive`,
  `ffxFsr2ResourceIsNull`), whose description extends past the 1,352-byte 2.1.2 prefix with
  `enableAutoReactive` and `colorOpaqueOnly`. The fallback passed a truncated stack copy; it now
  passes the game's description and only sets the prefix `reset` byte after skipped frames.
- **30 Hz at every boot.** The panel's 120 Hz DXGI mode is `31550000/262912` (index 5). RAGE saves
  the rational clamped to 262144, so `system.xml` held `262144/262144`; the game then fell back to
  mode 0 (30 Hz). This also happens with `Enable=0`, so it is a base-game fault. Boot requests
  were read from the new bridge log: saved `12000/100`, `120/1` or the exact rational all produce
  `31550000/262912`. The RDR2 settings hook now stores a centihertz rational when the value exceeds
  the limit and replaces an already clamped value with the desktop refresh. Offsets: video group
  +124, numerator +148, denominator +14c, from `system.xml` member order with `ReflexSettings` +38
  as the existing calibration. Game-tested at boot from the broken file: the chain was created at
  `31550000/262912`; the save-and-reboot round trip then stored `12000/100`.
- The bridge also kept the application's fullscreen refresh, scanline order and scaling in
  `CreateSwapChainForHwnd` translation and reports them from `GetFullscreenDesc`. It used to report
  0/0. This did not cause the 30 Hz boot, which also happens without RSF.

Release VS2026 build and 84/84 ctest (11 opt-in skips, `rdr2_capture*` excluded as above); the RDR2
settings test covers the refresh policy; XeSS FG cold fixture 39 tags/result 0. In game after the fixes: a
settings apply that previously failed and three preset changes (1280x800 inputs) completed without
a presentation failure or crash. Repeated VSync toggling has not been separately confirmed.

## RDR2 on Intel: FSR2 input route, orchestrator Auto SR and FidelityFX 2.4.0, 30 October 2026

The 0.1.1 package on an Intel Arc B370 (vendor 0x8086, device 0xb083, driver 32.0.101.9150) ran
three sessions without one SR evaluation. The game used its FSR2 carrier (`FSR2=0->2/3`, input
1130x706 into 1920x1200), but `rdr2.log` has no FSR dispatch, fallback, refusal or SR context
line. Every such path logs on first entry, so the patched import slot `+6032a98` never routed a
dispatch. The vendor census that recorded 4,412 dispatches hooks the DLL export instead
(`loader/census/src/census_internal.h`), as does OptiScaler (`inputs/FSR2_Dx12.cpp`,
revision `e7e3872d410f704479d94d318d68d163b6b58947`). The decrypted 25 October dump confirms the
static route (`+26f0505 call 2ea0af5`, `jmp [+6032a98]`, slot bound to the export `+3c70` at ASI
load), so the slot was displaced or bypassed later at runtime; which one is not established.

The adapter now detours `ffxFsr2ContextDispatch` in the game's Rockstar-signed
`ffx_fsr2_api_x64.dll` (timestamp `64907ede`, image `0x10000`, export `+3c70`, entry bytes
`48 85 c9 74 36 48 85 d2 74 31 8b 41 04 39 82 20`). Calls from the RDR2 image are routed; the
first caller RVA and whether the import slot resolves to the export are logged. A guarded
`ffxFsr2ContextCreate` detour (`+3ab0`) logs result, flags and sizes. FSR routing no longer
gates the DLSS route. The `2ea0af5` thunk check is dropped because nothing patches there.

SR Auto is now an orchestrator policy: `rsf_gpu_select_sr_backend` resolves the adapter
preference against the host's choices (FSR3, then XeSS fallback). The shared native host
defaults `SRBackend` to Auto (7) and resolves it from the device that records the first pass,
and the callback host uses the same function. The RDR2 package no longer writes `SRBackend=1`.

GTA V Enhanced is removed: it ships native DLSS FG, and OptiScaler covers the remaining needs.
Its adapter, ASI, tests and research note last exist at `150ff92`. The generic engine-owned
presentation mode of the shared host remains.

FidelityFX SDK 2.4.0 (`7539ae8b`) replaces 2.3.0 in `vendor/` and both packagers: FSR 2.3.4,
3.1.6, upscaler 4.1.2.4555 (SHA-256 `aad4fccc...4ccace`) and FG 4.0.2. The FSR4 INT8 compatibility
hook now keeps a table of pinned upscaler builds. In 4.1.2 the INT8 capability predicate moved
from `+8d70` to `+9960`; both are the same slot of the provider vtable (third entry after the
RTTI pointer, `.rdata+1af36b8` and `+3635270`). It adds a Shader Model 6.6 query, which the lease
already requires, and accepts ASIC family `0x96`. AMD's license text forbids disassembly of the
SDK binaries; the existing hook and this re-location depend on inspecting them.

Checks on the Intel Arc B370 with VS 2026 Build Tools 18.11: Release `eng/verify.ps1` built and
ran 86 tests, 84 passed or skipped (11 opt-in). `rdr2_capture` and `rdr2_capture_rgb10` fail
with `capture_timeout:true`: the diagnostic capture's 5 s budget is exceeded on this iGPU; that
code is unchanged. fmt and Clippy pass. With the RDR2 shared host (`rsf_unity_sr_d3d12_test`,
`RSF_RDR2_HOST_INI/ROOT`), no `SRBackend` logs `Auto SR selected backend 5` and XeSS 2.0.2 output is
accepted. FSR3 and FSR4 INT8 pass on 2.3.0 and 2.4.0 (`4.1.2.4555, RVA 0x9960`), FSR2 2.3.4
passes, and cold FG with FSR FG and XeSS FG tags 39 frames with result 0; FSR FG reports generated
frames. These are synthetic GPU fixtures.

Game run on the same device, deployment receipt `.local/deploy-backups/rdr2-intel-20261030/`
(Game DLL `0af430d5...52c57a7`, FidelityFX 2.4.0 runtimes): the game created its FSR2 context
through the export (`flags=0x29`, 960x600 into 1920x1200), and on entering gameplay the export
detour saw `RDR2.exe+26f050a`. RSF Auto selected XeSS, `SR context backend=5`, evaluation 0 and
output accepted. XeSS FG then reported `mode=1 active=1` with source frames advancing 120 and
presents 240 per 4 s. Pause produced the expected frozen-jitter native fallback. The preceding
run only reached the menus, so it shows no dispatch; that build's in-image caller filter was not
shown to be wrong. Generated-image quality, latency and HUD modes on Intel remain unchecked.

## RDR2 0.1.1 package, 29 October 2026

`tools/package-rdr2.py` builds the first RDR2 download from the post-migration Release outputs
and the AC7 0.1.1 vendor runtimes. Release VS2026 passed 104 discovered/93 executed/11 optional
skips with fmt/Clippy. The RDR2 shared-host GPU fixture passed against the staged package for
four SR providers and three cold-FG providers with the overlay open. Built and synthetic-tested;
this exact ASI/Runtime has not run in RDR2. [Build record](releases/v0.1.1-rdr2.md).

## Shared host domain repair, 28 October 2026

Game/engine policy has moved out of shared hosts: adapter setup supplies identity/configuration/
window policy; Unity owns its startup/exports/configuration and AC7 produces material mip bias.
Generic runtime mechanisms retain SDK/GPU/presentation ownership. Affected binaries build and
contract tests pass; RDR2/Unity redeployment and live preservation checks remain outstanding.

## RDR2 shared overlay and quality controls, 26 October 2026

All five performance presets now change real engine input resolution during play, including
Native/DLAA at1:1 and Native->Quality transitions. The permanent PID140840 run measured
2560x1600,1707x1067,1485x928,1280x800 and853x533 inputs into2560x1600 output; the human confirmed
it works. The fix updates RAGE's pending graphics owner before native synchronized reconfiguration.
Earlier active-only/notification attempts and their corruption are retained as failed evidence.

The shared Rust overlay replaces cancelled native UI insertion. Busy observation/full GPU-ring
UI omissions are corrected. The final FG-Off selector correction retains the last provider for
re-enable while displaying Off consistently; frontend regression/build pass, deployed, separate
post-patch visual acceptance pending. Release VS2026:97 discovered/11 optional skips, fmt/Clippy;
Rust41+19 tests/release for the panel. Frontend ABI2, overlay ABI13, game ABI19. Diagnostics default
off and caches unchanged; RTSS/Social Club corrections preserved. HDR/non-NVIDIA hardware and
complete HUD-mode acceptance remain open; broad rendering_ready stays0.

Evidence: [RDR2 research](research/rdr2-renderer.md),
[continuation](../games/rdr2/RESEARCH-HANDOFF.md), and
`.local/rdr2-tools/quality-overlay-20261026/{permanent-result.json,deployment.json}`.

## Core frame dump and AC7 HUD correction, 26 October 2026

F10 and the C API now use an orchestrator-owned named frame service across D3D11,
D3D12, Vulkan and Minecraft's GL/D3D12 bridge. GPU readers keep snapshots through
real completion and produce opaque PNGs, exact raw rows, camera/SR/FG metadata and
explicit unavailable stages. New integrations can submit named resources through
C interfaces. [Contract, boundary correction and evidence](research/core-frame-dump.md).

The user's AC7 retake confirms a genuine pre-UI scene, separate menu raster and final
image at source frame 7167/native frame 4651. Nimbus composite input one was missing
from the first exporter; it is now leased alongside input zero. The later actual FG
input is correctly named `fg_scene_color` and can contain UI. Existing FG routing
and broad readiness flags are unchanged. The stored HUD alpha is preserved as-is.

The primary 83-case native batch and the independent feature-only 78-case batch
pass, each with 11 opt-in skips; formatting and Clippy pass. D3D11/WARP, public
D3D12/WARP, Intel Vulkan with validation/RTSS, and the real hidden OpenGL pipeline
validate PNG/raw production, identities and completion. Minecraft's 43 checks pass;
Godot's native add-on and both editor editions build, as does its C# facade.
AC7, the owned Unity pair and Minecraft's JAR payload are backed up, deployed and
hash-verified. Godot's package/editors are staged. New Unity/Minecraft/Godot gameplay
capture acceptance remains open; no user's game was launched or stopped.

## End-of-day checkpoint, 25 October 2026

Stopped at the user's request, with all repository changes committed for the
next session. No completion or broad readiness claim is made.

- Latest native Release verification: 75 discovered, 64 executed, 11 opt-in skips;
  Rust formatting and Clippy pass. Log: `.local/godot/verify-godot-wrapup-20261025.log`.
- Standard/.NET editors build with the FSR1 format correction. The first new
  aggregate-matrix case correctly failed because FSR1's shader declared RGBA16F
  while RCAS wrote an RGBA8 UNORM destination. The guarded engine patch selects
  the storage format separately for EASU and RCAS. GPU retakes and updated
  Debug/Release templates remain pending. [Format evidence and build hashes](research/godot-fsr1-storage-format-20261025.md).
- `eng/test-godot-vulkan-matrix.ps1` now aggregates functional/export fixtures and
  actual generated-image reports, preserves failed cases and indexes player PNGs.
  Initial failed-control evidence is
  `.local/godot/matrix-20261025-211312-790/index.html`; the full matrix and passing
  aggregate path remain unverified. SR fixtures now accept Off, FSR1 and Auto.
- The latest XeSS FG guide-extent check passes both editions' functional cases
  and four camera-motion generated-image comparisons after the real SR plan
  settles. [Evidence](research/godot-fsr4-fg-interop-20261025.md).
- DLSS options publication removes API/synchronization errors, but a native
  secondary window still causes a shutdown access violation in driver queue
  destruction. Startup, resize, count and provider-return controls shut down
  cleanly. The primary-only latency-chain candidate failed because the SDK
  re-enabled the omitted secondary marker; that candidate was reverted.
  [Investigation and next proposal](research/godot-vulkan-dlss-fg.md).
- FSR2's six bounded diagnostic captures produced 24 intermediate outputs.
  Boundary bias is already present in reprojected history and can increase during
  rectification. This is diagnostic evidence, not a production correction.
  [Stage evidence and limits](research/godot-fsr2-stage-edges-20261025.md).
- XeSS count2/3 cannot be accepted on this NVIDIA device, whose SDK maximum is
  one. Cross-provider count preflight and general ordinal analysis were audited
  but not implemented. [Audit](research/godot-xess-mfg-capability-audit-20261025.md).

Resume with the FSR1 GPU retake, then the distinct secondary-WSI lifetime fix,
FSR2 reconstruction investigation and pending capability/scene/export coverage.
Retain the existing image gates and failed controls. Captures remain bounded;
duplicate cleanup does not disable new captures.

## Godot developer integration in progress, 25 October 2026

Latest harness milestone: XeSS FG editor image checks pass all twelve standard/.NET
object/camera/combined cases with SR0/SR5, covering 48 actual generated images and
fresh independent references. Maximum centroid error is 0.265658 output pixels;
the existing 0.35-pixel gate is unchanged. Optional foreground maintenance records
owned-window activation, bounded failures and successful retries. Rebuilt standard/.NET
Release FSR4 SR exports pass switching, resize and raw capture checks. Debug exports
pass XeSS FG/PCL switching, Off/resume and resize on both editions. Negative FG4
admission preserves the working FSR3 pipeline and continued generation on both
editions. The strict DLSS lifecycle fixture now passes all eleven phases and API
checks after the options correction, but still fails shutdown. See the [XeSS capture evidence](research/godot-xess-fg-capture-20261025.md)
and [DLSS lifecycle investigation](research/godot-vulkan-dlss-fg.md).

The Vulkan FSR4 FG adapter, shared DX12/Vulkan importer, exact-provider admission
and generic routing now build on MSVC. The Release gate passes 74 discovered tests
with 63 executed and 11 opt-in skips, plus Rust formatting and Clippy. NVIDIA
device queries correctly refuse an FSR3-only provider list. AMD execution and
generated-image acceptance remain untested. [Evidence and limits](research/godot-fsr4-fg-interop-20261025.md).

The next coordinated MSVC Release gate passes 75 discovered tests, 64 executed
and 11 opt-in skips, plus formatting and Clippy. Streamline now filters device
requirements by the selected adapter's exact LUID and gates NVIDIA latency
constructors by the selected vendor. The fresh NVIDIA pair passes standard/.NET
camera-free native-WSI PCL and standard DLSS SR switching/resize with four raw
frames. A fixed-guide extent guard also passes standard/.NET XeSS FG SR switches,
Off/resume and resize. AMD and mixed-adapter execution remain untested.
[SDK evidence](research/godot-streamline-device-requirements-20261025.md).

`integrations/godot` now contains a pinned 4.7.2 GDExtension, a native engine
module, guarded source patching and an automatic harness. Standard and .NET
editions passed the DX12/Vulkan renderer probe. The standard custom-engine DX12
run passed shared DLSS/FSR3/XeSS SR, FSR3/XeSS/DLSS FG switching, SR-independent
generation, DLSS MFG and PCL with SR/FG Off. A typeless-resource fixture passed
after correcting the shared normalizer. The C# facade now runs on the .NET engine.
Actual generated-frame dumps exposed zero motion at the native consumer despite
valid engine vectors. Declaring native callback pipeline stages and correcting
camera matrix layout/Y conventions fixed the captured inputs. The new visual
harness compares actual FSR-generated output with separately rendered midpoints
and rejects the preserved broken capture. It covers standard/.NET object and
camera motion separately. Explicit engine-owned swapchain creation also replaces
DXGI detours in this developer path. All six standard/.NET visual cases passed,
covering 24 actual generated images, and the corrected standard/.NET functional
tests passed all six provider/PCL phases with real foreground ownership. The Release
native gate passed (44 executed tests, eight opt-in skips); explicit FSR3
presentation and FSR/XeSS switching device fixtures passed separately. Vulkan
runtime remains unfinished. Standard/.NET Debug and Release exported players now
pass runtime switching and controller lifecycle fixtures, including C#; the
GDScript-only .NET Release export also passed. PCL diagnostic pings are checked
in each runtime phase, and lifecycle checks cover camera changes/cuts, pause,
Off/resume, controller replacement and PCL after the last camera is removed.
SR-enabled captures exposed a second motion convention: Godot's temporal
sentinel `(-1,-1)` requires camera reprojection. The engine module now resolves
it without replacing valid object vectors. The expanded matrix completed 30
cases and 120 actual generated frames: all vector/camera gates passed, while
26 image cases passed and four FSR2/FSR3 object-motion cases failed. The suite
fails overall and preserves the images. Most FSR2 displacement is already in the
SR source; a same-motion warmup experiment did not remove it. PCL from startup
without any camera/compositor also passed on both editions and Release exports.
Native Vulkan FSR2/FSR3 SR is now implemented as a reusable runtime service and
a Godot adapter, with same-device image leases and engine command-buffer recording.
Both editions exercise provider switching, resize/reset and actual player images.
The opt-in raw readback harness checks depth, motion and raster jitter against
scene geometry and preserves actual SR output. Validation found and corrected
FSR2 storage-format mismatches in the pinned SDK and Godot fallback, a missing
copy-destination usage flag and a null Windows surface instance. Native Vulkan
FSR3 FG now has a complete runtime swapchain owner, four reserved queues,
submission-bound input leases and actual generated-frame readbacks. Standard/.NET
functional tests cover SR-independent generation, Off/resume, resize and a
secondary native window. Validation found a truncated SDK descriptor pool and
a presenter CPU retirement race; guarded staged corrections address both.
The corrected SR Off/FSR3 image suite passes all 12 standard/.NET cases and 48
actual generated frames against new MSAA midpoint references.
Synchronization-validation present hazards also reproduce in official Godot and
remain unresolved. Vulkan PCL/Reflex now has a reusable native service, early
Streamline dispatch, CPU/source token tracking and root-window Present markers.
Standard/.NET camera-free fixtures cover On/Boost/Off, resize, synthetic input
and diagnostic pings, including a control with no FG presentation owner. A
SR Off/FSR3 image suite retains 12 passing cases and 48 actual generated
frames. The final PCL ownership/synchronization path additionally passes four
combined-motion cases and 16 actual generated images. Real WSI callbacks now
use a versioned C ABI broker, and primary/secondary SDK ownership is asserted. Other Vulkan providers remain unfinished. The SR-only readback checks do not
establish Vulkan image-quality acceptance. For DX12, resetting only selected
FSR2 history frames removes most measured source displacement; this diagnostic
does not fix the normal temporal path. The 30-case image suite still fails.
These engine/device runs
do not establish broad visual acceptance or measured latency.
An additional geometry control keeps the ordinary FSR2 cube failure visible,
while a thin camera-facing box passes its four generated-image checks. It narrows
the reconstruction investigation without changing production rendering or gates.
Vulkan SR refusal now records a spatial fallback while preserving the SDK error.
Full-image GPU gradient checks and standard/.NET missing-DLL fixtures pass;
normal SR regression also passes with C# and raw inputs. The FSR2 displacement
and other-provider work remain open.
Native Vulkan DLSS SR now shares the early Streamline/PCL session, leases the
matching CPU token, stages owned input images and uses a separate SR viewport.
Standard/.NET switching, resize, raw-input/C# checks and six DLSS SR + FSR3 FG +
PCL image cases pass. Model-switch teardown testing also exposed a deferred SDK
release that can outlive a camera-free shutdown; drained feature retirement
addresses it. Rebuilt Vulkan Debug/Release templates now pass 12 standard/.NET
exported SR, FG and camera-free PCL fixtures, with C# on .NET SR/FG and actual
PNG/raw input checks in SR. Template/player and both native-library copies are
fingerprinted. XeSS Vulkan SR now passes six standard/.NET image cases and 24
actual generated images with FSR3 FG and PCL. DLSS FG passes strict standard/.NET
checks and midpoint image comparisons, including shared DLSS SR + FG. MFG3's
private buffer-write hazard is corrected; all three actual output ordinals pass
strict GPU validation and independently rendered quarter-step comparisons.
XeSS FG's Vulkan/DX12 presenter passes standard/.NET lifecycle/C#/PCL checks; the
first standard object-motion cases with SR Off and XeSS SR pass actual generated
image and guide checks. FSR4's deferred graph split passes standalone GPU work,
both Godot editors' switching/resize checks, C# on .NET and one combined-motion
image comparison per edition. The corrected private-output capture also passes
four complete raw SR frames per edition.
The present hazard is isolated to RTSS hook injection in bounded process-only
controls. Warm steady-state FFX captures no longer show the reset/disable feedback
loop; cold startup latency remains separate. FSR2 displacement, new-provider
exports, Vulkan FSR4 FG routing and broad scene acceptance remain unfinished.
[Evidence and corrections](research/godot-engine-integration.md),
[developer integration](../integrations/godot/README.md).

## REA MCP update, 25 October 2026

Pinned and installed REA 6.8.0, with an optional private Node path in the repo
launcher and matching existing user-wide registrations. Six production stdio
checks passed the 229-tool/six-prompt catalog and target-free request; Ghidra/IDA
readiness passed. This is tooling validation, without game or target analysis.
Existing sessions need a reconnect. [Setup and evidence](tooling.md#rea-for-codex-and-claude-code).

## Red Dead Redemption 2 scaffold, 25 October 2026

26 October frontend correction: the human confirmed the ABI19 ScriptHook status overlay and
quick menu in gameplay; inline RSF rows in the stock graphics page remain unfinished. XeSS
FG flicker was measured as result3 (too few frames), resetHistory1, source-gap1 despite valid
inputs. Normal RAGE PresentEnd cleanup called provider abort for retired CPU inputs, which
clears XeSS history. Removed provider abort from this completion path while retaining shared
Streamline token retirement. Release VS2026 passes82 discovered/11 optional skips, fmt/Clippy;
backed-up Runtime68b7229e...5bd37a0 is deployed. Follow-up gameplay shows83.344 seconds/4320 source
frames/8640 SDK presented frames with37 active/error-free samples after warm-up; the human
reports improved look/feel and no orange flashes. Residual white text is observer contention;
the separate correction retains cached labels and reports stale after1s. It was backed-up/hash-
deployed as ASIa1a7c98e...e07544c at20:19 after normal exit, preserving the verified FG runtime;
the label change's visual acceptance remains pending. Evidence is
`.local/rdr2-tools/xess-flicker-20261026/retirement-deployment.json`, its preceding log/state
samples, and `rdr2-xess-retirement-verify.log`. The earlier timing-input experiment did not fix it.

Live continuation: AB executed DLSS/FSR3/FSR4 INT8/XeSS and returned DLSS in one RDR2 process,
with twenty original pixel outputs. Pause exposed a stale native NGX handle after RSF context
shutdown. A persistent registration now outlives per-feature contexts; AC passed
DLSS->FSR3->DLSS, twelve further outputs and actual Pause/Settings with native NGX success.
Release VS2026 passed 67 executed tests/11 skips, fmt/Clippy; the corrected build is deployed
with backed-up hashes in AC/deployment.json. New source brackets the engine's final output
helper, identifies its real native backbuffer through the render-target cache, and observes
native marker IDs through Present. Its GPU fixture passes fenced scene-copy pixels and state
validation. ABI18 now connects native sleep/mode/markers to shared FG presentation and
associates input, SR, scene submission and Present. Forty-frame DLSS/FSR3/XeSS GPU paths pass
39 FG tags, Fixed mode1 and debug validation, including live provider transitions. FSR3 reports
generation callbacks. Original final/HUD-less pixel copies match in the UI-free fixture.
The bridge retains the engine queue and
fence-joins Streamline's required proxy queue. A rejected raw-queue substitution crashed
ExecuteCommandLists; its debugger evidence is retained. Release VS2026 passes67 executed
tests/11 skips, fmt/Clippy. AD then exposed skipped loading inputs exhausting the six-token
pool. Ordered native PresentEnd now retires skipped CPU inputs and the SDK's authoritative
pool. A regression covers140 inputs without an image, CPU-ring wrap, then40 valid SR/FG
frames. It passes39 tags and debug validation; the final full gate is
`.local/rdr2-tools/rdr2-skipped-input-final-verify.log`.

A byte-guarded diagnostic applied equivalent skipped-token retirement to the running AD
session. DLSS FG2x/4x, Off/resume and DLSS->FSR3->DLSS SR with FG2x were accepted by real
SDK activity and driver multiplier reports. Eight SR outputs are finite; projection/inverse
residual is at most3.93483e-8. The actual scene/final2560x1600 pair differs in116394 HUD pixels
and zero outside minimap/prompts. After normal exit, the permanent Runtime9a9d0fcc...b7d055ac
was backed up and hash-deployed at18:13 with diagnostics off. Evidence is AD's
`permanent-deployment.json`, logs and `hud-pixel-comparison.json`. The installed permanent
build has not yet been cold game-tested; the game-control connection is unavailable.
All-mode HUD separation, native FSR selection, engine resolution ownership and native options
remain open. No generated-image quality or physical latency claim follows from activity.
One requested menu agent
completed and was released; [its report](../games/rdr2/NATIVE-OPTIONS-RESEARCH.md) now includes
live stock-row identities. Incidental greyed AA controls are not implemented RSF controls.

26 October implementation: native DLSS and FSR selections now normalize into one SR adapter
and execute the shared D3D12 runtime. Game ABI 17 adds synchronous acceptance, actual submission
and Present callbacks. The game plugin owns guarded hooks, current worker camera conversion,
input states and native history reset; the runtime owns settings, vendor contexts and fenced
retirement. Engine-sized inputs and supplied exposure are preserved. Standalone DLSS now forwards
its application ID and receives required manual Present bookkeeping. DLSS, FSR3, FSR4 INT8 and
XeSS passed actual GPU host fixtures with pixels, debug validation, unsubmitted-slot refusal,
submission retirement/reuse and Present. Full Release VS2026 gate passed 67 executed tests and
11 opt-in skips, fmt/Clippy (`.local/rdr2-tools/native-adapter-verify.log`). Backed-up deployment
completed with diagnostics off; `.local/rdr2-tools/native-adapter-deployment.json` records hashes.
Live game acceptance is pending. FG, PCL, native menu replacement and ownership of the render
resolution producer remain unfinished. This supersedes the older source/deployment status below.

Run AA completed 27 bundles, including 88 exact-call camera/colour/depth/motion/exposure/output
sets. All camera conversions pass, maximum projection/inverse residual `4.68364e-8`. A visually
selected static rock ROI in AA6/7 has median reprojection residuals 0.0427-0.1700 pixels, versus
1.646-11.996 with the opposite sign. Full shader identities and original pixels resolve the
previous stream-PSO gap. Pause has several scene-wide effect draws before final `BB74CEA4`
and UI; a universal first-draw boundary remains invalid. Photo/cinematic/letterboxed cutscenes,
visible Social Club overlay, full UI-only extraction and HDR remain unverified; the display has
no HDR support. [Current evidence and producer sites](research/rdr2-renderer.md#run-aa-and-native-sr-adapter-26-october).

Camera/HUD follow-up, 26 October: the guarded probe recorded 128 run-W, 320 run-X and 640 run-Z
DLSS camera samples. Matrix/jitter relationships and 192 same-frame ScriptHook comparisons are
measured; pause freezes projection while DLSS jitter advances, and one Z history reset makes
previous view equal current view. RenderDoc captures establish shader topology but replay the 3D
scene black. Native Z supplies 586 original pixel readbacks across 12 captures: normal gameplay
has a HUD-less scene copy, while wheel/pause/log/Dead Eye add scene-wide changes afterward.
Those modes cannot share an assumed first-draw HUD boundary. HDR, photo/cinematic/cutscene,
nonzero motion reprojection and complete GPU frame association remain open.

The Mor3a 0.82 source was belatedly inspected at revision `bc853d0b`; its DLSS input, frame
association, HUD-history and diagnostic capture contracts are retained in
[the integration handoff](../games/rdr2/RESEARCH-HANDOFF.md). Its nominal camera and heuristic
Vulkan HUD capture do not replace measured DX12 proof. The latest stream-PSO/F10/final-draw
probe passed the Release VS2026 gate (57 passed, eight opt-in skips, fmt/Clippy) and was deployed
with matching hash `d480f183...`, diagnostics off. Those latest changes have no live acceptance.
The complete single-request input/camera/UI capture bundle is still source work; finish it
before requesting another run. All RSF providers must use DLSS-shaped inputs regardless of the
native DLSS/FSR selection. Routing, RSF SR/FG execution and native menu replacement remain
unimplemented. [Detailed research](research/rdr2-renderer.md).

Crash investigation follow-up: removed the rejected ASI cache quarantine and its INI switch.
The ASI leaves the pipeline library, warm-up cache and marker untouched. An exact-build HRESULT
trace and unfiltered 32-frame D3D12 diagnostic stacks identified Social Club recording into a
different backbuffer than RAGE's command list. Close first returns `E_INVALIDARG`; Reset repeats
it later. A guarded in-process selector correction preserves RAGE's supplied buffer index for
external command lists and retains Social Club's original selection for its own lists. The first
corrected game run caught supplied index 1 versus current index 0, passed the prior crash point,
wrote new caches and recorded no D3D12 errors with RTSS active. The final revision was then deployed
and passed a cached Story-mode session with DLSS and FSR2, followed by another cache-reuse launch
with census and tracing disabled. The Release VS2026 gate passed 57 tests with eight opt-in skips,
formatting and Clippy. [Cause, exact bytes and run evidence](research/rdr2-renderer.md#social-club-frame-ownership-correction).
The completed census records 4,900 DLSS evaluations, 4,412 FSR2 dispatches, 13,267 Presents and
83,549 Reflex markers in its final periodic summary, with zero record drops. The user confirmed
both Story-mode paths and a normal exit. Input descriptions and call sites now feed the six-step
SR/FG sequence in the research note. Full RSF SR/FG execution, matrices, HUD separation and motion
coverage remain future integration work. Census and tracing are disabled in the final game INI;
forced DX12 and the guarded correction remain active. No changes were committed.

Added `games/rdr2`: exact-build detection of Steam `RDR2.exe` 1.0.1491.50 (build 13773296) and
a lifecycle that refuses preparation. `ReScaleFrame.RDR2.asi` is a new deployment mode: the game's
ASI loader loads it, and it runs the plugin session off the loader lock without impersonating a
system DLL. The target is forced DX12 with DLSS, FSR and XeSS SR/FG, inputs from the game's own
NGX/FSR2 calls and translation of its native Reflex markers. The fingerprint shows encrypted code,
so static analysis needed a runtime dump. At this initial scaffold checkpoint the entry had not
been loaded by the game; later game evidence and the current result are recorded above.
`eng/verify.ps1 -Configuration Release -VS2026` passed 46 native checks with eight opt-in skips,
including the new `game_rdr2_plugin_contract`, followed by formatting and Clippy.

Follow-up the same day: two user-run sessions produced decrypted-image dumps; the ASI loader runs
modules during CRT startup, after decryption. A guarded pointer write of `"d3d12"` to the engine's
API parameter (`ForceD3D12`, default on) made the game save `kSettingAPI_DX12` and build a D3D12
pipeline cache after one self-restart. New tooling: `loader/census` (`rsf_vendor_census`), a
pass-through probe recording D3D12/DXGI, Vulkan, NGX, FSR2/FFX API, XeSS, Streamline, NVAPI Reflex
and related calls with caller RVAs, stacks and decoded arguments; and module dump sidecar schema 2
(threads, MSVC RTTI, `.pdata` functions, section hashes, name lists) with IDA/Ghidra apply scripts in
`tools/dump`. At that checkpoint both were synthetically tested only. The
Release gate passed 57 native checks with eight opt-in skips, formatting and Clippy. The census is
deployed behind `[RDR2] Census=1`; the completed gameplay census is recorded above.
[Fingerprint, reference review, FG requirements and open research](research/rdr2-renderer.md).

## Drag'n Wash character motion, 20 October 2026

Added a fingerprint-guarded character shader patch and its game-specific URP motion
producer. It retains actual deformed vertex positions per renderer/submesh and writes
the existing temporal motion target for SR and FG. The original colour/depth/shadow
passes remain intact. All 64 generated variants compile; Release managed builds,
CoreCLR/shipped Mono contracts and a D3D12 moving-geometry fixture pass. The patch is
deployed locally with backups. Unity player binding and Claw visual acceptance remain
pending; the Editor fixture is blocked by a missing license. The shader audit also
identifies separate procedural mesh, skinned particle-material, VFX and fluid coverage
gaps. [Evidence, implementation and limits](research/drag-n-wash-character-motion-20261020.md).

## Drag'n Wash 0.1.1 SDK build and packaging, 18 October 2026

Removed the installed-game requirement from the Unity build/package path. Public Unity
SDK references compile the unchanged helper; Release artifacts and allowlisted vendor
runtimes produce the package without deployment. Compile references are excluded from
the download, and dependency notices and every archived file hash are checked.
Windows Release verification passed 44 native checks with eight skips and clean Rust
formatting/Clippy. The available newer URP reference cache fails the target-specific
metadata preflight; this is recorded separately from compilation, not reported as a
target-player pass. No fresh game session is claimed.
[Inputs, failed approaches and checks](releases/v0.1.1-drag-n-wash.md).

## Release target correction, 18 October 2026

Withdrew the Project Wingman observer preview to draft and removed its public
download links. Wingman remains renderer research, without SR/FG/latency/VR
processing. The requested next package is Drag'n Wash with the latest shared
runtime, Unity helper and overlay. The initial assumption that its managed build
needed game assemblies was corrected by the SDK build above.

## Windows 0.1.1 preview packaging, 18 October 2026

Prepared separate AC7 and Project Wingman observer prereleases, retaining the
stable AC7 SR-only assets. Component versions and Cargo lock entries are 0.1.1.
Fixed VS2022 C4127 template conditions with `if constexpr`; added Wingman's
allowlisted observer packager and restored the AMD loader/MFG notice to AC7's
payload. Local SDK-enabled Windows Release verification passes 44 native
checks with eight skips, formatting/Clippy and all 59 Rust tests. No fresh game
or VR acceptance is claimed. [Rationale and checks](releases/v0.1.1-windows.md).
## Reusable Unity URP package, 18 October 2026

RTSS compatibility is a hard requirement for every supported API, including active FG
and provider changes. Disabling or excluding RTSS does not satisfy this integration.

The new `integrations/unity/com.kpc.rescaleframe` package supplies URP scene colour,
depth, object/camera motion, jitter and completed-frame inputs to shared SR, FG and
PCL. It includes DX11/DX12/Vulkan bridges, Editor Game-view presentation, runtime
quality/model/provider controls, SDK-limited MFG and actual Base/output FPS counters.
The full target remains Editor Play and Windows players across all three APIs.
[Source contracts, failures and current evidence](research/unity-urp-package.md).

The current-main Release gate passed 44 native checks with eight optional skips and
clean formatting/Clippy. Unity 6000.6.6f1 / URP 17.6 built the validation player.
RTSS-enabled Unity runs executed DLSS SR with active FSR3 FG and PCL markers on all
three APIs. Follow-up runs also executed XeSS SR with active XeSS FG and FSR2 SR with
active FSR3 FG. Repeated Editor Play and runtime provider changes resumed rendering.

Corrections cover the shared Streamline owner, Play-session cleanup, RTSS's nested
creation queue association, a stable engine latency handle, DX11's cached buffer-zero
view and presentation queue, Vulkan window-thread coordination, and XeSS's 32-bit SDK
frame counters. Explicit GPU checks passed 40 DX11 physical-copy pixels through
provider changes and retained one latency handle across a wide-ID/session-epoch test.

The UPM archive is built. The latest native DLL still awaits deployment to the open
editor; it must close before its loaded file is replaced. Foreground DLSS-G/MFG,
visible RTSS OSD and broad image-quality acceptance remain open. Stock URP reproduces
the DX12 reduced-scale depth-backbuffer validation errors tracked as UUM-136054.
This integration is in progress until the remaining live acceptance is completed.

## RTX 40 MFG unlock, 18 October 2026

Integrated MFG-Unlocker's guarded RTX 40 temporal transform, provider gate and early
Streamline wrapper publication into the existing DLSS-G host. The Insert overlay uses
its existing multiplier control up to the admitted runtime capacity and reports unlock
admission separately from active generation. FG ABI 2 and overlay ABI 12 require matched
components. Other GPU architectures keep their native SDK path. [Source, lifecycle,
failed approaches and evidence](research/rtx40-mfg-unlock.md).

The explicit focused RTX 4070 Laptop fixture passed 2x, 3x, 4x and 6x with SDK-confirmed
presentation deltas, count changes, generation Off/On and repeated provider transitions.
FSR and XeSS remained active during their stages; engine buffer identities survived.
Production MSVC Release verification passed 44 native tests with eight opt-in skips,
formatting and Clippy; 59 Rust tests passed. The real overlay ABI/input/render/resize
preflight passed. The focused device run used logging instrumentation; the production
rebuild removes only that instrumentation and passed the full native gate.

The isolated build at main baseline `3cc6af2` is deployed to AC7 and Drag'n Wash, and the
rebuilt `rescaleframecraft-1.1.11-dev.jar` is deployed to N1GHT. Installed hashes match;
the upstream MIT notice is included. Backup:
`.local/deploy-backups/rtx40-mfg-20261018-094106-547`. Game settings were preserved, and
all 277 original Drag'n Wash files verified unchanged. No game was launched. Actual game
image quality, pacing, latency and other-device acceptance remain separate from the
synthetic device result.

## Shared DLSS model selection, 18 October 2026

AC7 and Unity's Insert overlay and Minecraft's Sodium/Reese's options and F8 screen now select
Default, E, F, J, K, L or M independently of quality/render scale. AC7 saves `Rendering/DlssPreset`
in its existing per-user AC7.ini. Unity saves it in ReScaleFrame/preferences.ini, with
`UnitySR/DlssPreset` as the installation default. Minecraft saves `dlssPreset` in its JSON config.
Invalid saved values fall back to Default. Driver overrides can replace this request, so the
controls do not claim to report the effective driver model.

Source rationale: the bundled Streamline 2.14.1 `sl_dlss.h` defines the supported model IDs;
its [DLSS plugin](https://github.com/NVIDIA-RTX/Streamline/blob/v2.14.1/source/plugins/sl.dlss/dlssEntry.cpp)
compares the per-quality preset fields before evaluation and recreates the NGX feature on change,
resetting history and delaying the old feature's release. The shared SR implementations set all
quality fields together. AC7 includes the preset in its cached option comparison and both scene
and auxiliary frames. Unity and Minecraft use their existing GPU-drained reconfiguration paths.
DLSS ABI 5 and overlay ABI 11 require matched rebuilt components.

Validation: `eng/verify.ps1 -Configuration Release -VS2026` passed from isolated main baseline
`6e70ab7` plus this change: 44 native passes, eight opt-in skips, formatting and Clippy clean.
`cargo test --workspace --locked` passed 58 checks, including preset clicks, Default restoration,
quality independence, backend gating and ABI offsets. Minecraft `test jar` passed 22 checks,
including the preset-bearing JNI signature and actual Sodium option registration.

The headless native D3D12 fixture evaluated all seven preset requests on an RTX 4070 Laptop,
using a shared SR/FG host, Quality mode, 427x240 input and 640x360 output. All evaluations returned
success and finite nonzero pixel readback. Each request used a fresh fixture process; this does
not prove live switching or effective model identity under driver overrides. Use
`RSF_DLSS_TEST_PRESET=<numeric ID>` with `rsf_unity_sr_d3d12_test` to select the fixture request.

The isolated AC7 proxy, Unity runtime and matched overlay are deployed with backups and matching
installed hashes. Minecraft's rebuilt `rescaleframecraft-1.1.6-dev.jar` is deployed to N1GHT;
its embedded JNI DLL matches the isolated build. No games were launched. Live game preset
switching and visual quality remain user-run acceptance.

## VulkanStory host integration

The Vulkan provider bridges and guarded FidelityFX/Streamline SDK corrections now live in
`runtime/vulkan`, consumed through VulkanStory-Rewrite's pinned ReScaleFrame dependency.
FSR4 compatibility is shared with the existing DX12 backend instead of maintaining a port.
The native Vulkan bridges, corrected SDK binaries and shared FSR4 target built successfully
from the pinned host checkout. The clean no-SDK gate exposed an unused `shared` parameter
in the DLSS fallback; the corrected gate now passes 44 native checks with eight optional
GPU checks skipped, followed by Rust formatting and Clippy.

`runtime/managed` contains the SR/FG controllers, Vulkan SDK wrappers, latency scheduling
and generated-present pacing. VulkanStory keeps game input production and renderer resource
operations behind host interfaces. Public contract names are forwarded; bootstrap resolution
and staging include both ReScaleFrame assemblies. Production build, native bundle, staging
and deployment pass, with installed module hashes matching the tested package.

On the RTX 4070 Laptop, the game switched DLSS, FSR3, FSR4 and XeSS SR, then returned to TAA
in one process, with 480 successful SR frames. The initial run stopped at its focus assertion.
An explicitly authorized follow-up used the opt-in one-request focus gate and passed all 57
remaining actions: DLSS/FSR/XeSS FG switching with advancing presentation counters, multiplier
request/clamping, latency Off/On/Boost, world reload and clean exit. DLSS reported one generated
frame maximum on this device. The trace has 2,967 successful Reflex sleeps, once per frame,
including all three latency modes, with no duplicate calls. Checked logs show no Vulkan,
frame-token or teardown failures. Captures show the scene and HUD through the transitions;
these checks do not measure input-to-display latency or establish every GPU's MFG limits.

## Minecraft OpenGL SR and FG, 17 October 2026

The next motion increment adds final GPU vertex history for Flywheel 1.0.6 and Colorwheel 1.3.0,
including indirect, instanced and crumbling draws. Iris integration preserves shader-pack hands,
excludes shadow history and coordinates effective TAA ownership. Java compilation and 26 checks
pass, including a real offscreen OpenGL deformation/cutout test. Live shader-pack acceptance and
other missing motion producers remain open. [Source contracts and proof limits](../games/minecraft/MOTION.md).

The 1.1.7-dev live shader run exposed missing world geometry, incomplete Iris framebuffers and
depth-format mismatches. The 1.1.8-dev correction reattaches depth by image identity and format,
retires stale stencil attachments and converts world/display depth through a shader pass.
All 29 Java/GPU checks pass, including D32F-to-D24S8 depth copying with color/stencil preservation.
The corrected JAR is deployed with a matching hash; live world acceptance remains pending.

The human confirms world geometry is restored in 1.1.8-dev; incomplete-framebuffer errors stop.
The 1.1.9-dev follow-up gives the owned scene an explicit D32F allocation for Iris depth copies
and preserves Complementary's frame-varying atmospheric sampling while its TAA/jitter remain off.
All 30 checks pass and the JAR is deployed. Static-cloud convergence and the remaining depth
error are awaiting the new live run. The general motion goal remains paused.

The human accepts the 1.1.9-dev world/cloud correction and requests its release as Minecraft
1.1.9. The accepted log shows explicit DEPTH32F world storage and no incomplete-framebuffer or
depth-format mismatch messages. Release packaging changes version metadata/documentation only;
runtime classes and bundled native files are compared against the accepted development JAR.

The next Minecraft increment installs DH 3.3.3 and adds Iris initialization/resize fixes, shared
jitter ownership and a separate projection for unified distant depth/motion guides. All 32 Java/
GPU checks pass. Real modpack startup binds DH/Iris callbacks; world/transition acceptance remains
open. [Source contracts and evidence](../games/minecraft/DISTANT-HORIZONS.md).

The human accepts the DH appearance, then identifies TAA-only particle/item trails. The
1.1.11-dev correction captures Iris-batched entities, respects sprite coverage, and adds previous
surface depth/slope visibility for raw TAA. All 34 checks pass, including occluder rejection and
valid sloped history. The JAR is installed; Prism's memory dialog blocks the live test. RTSS OSD
routing is a separate pending per-app profile proposal. The full motion goal remains active.

Minecraft 1.1.12-dev adds final GPU vertex history for Iris's solid/translucent hand draws,
shares the existing GPU replay core independently of Flywheel, and decodes compressed hand
depth through the previous scene projection. All 37 Java/GPU checks and packaging pass, including
production draw-state restoration and missed-frame invalidation. The JAR awaits deployment:
Prism's pending 1.1.11-dev launch is still blocked by its low-memory dialog. Live hand/particle
acceptance, terrain/water deformation and shader-deformed actors remain open.

Minecraft 1.1.13-dev adds final GPU motion for Sodium/Iris terrain and water. Section mesh
generations distinguish rebuilds from arena moves; section-local indexing survives translucent
sorting. The real-GL regression exercises the production adapter, retained deformation, changed
base offsets, reversed draw order, cutout holes and selective rebuild invalidation. Iris pipeline
retirement releases the shared motion resources. Live binding/appearance and deployment still
await Prism's pending launch dialog; shader-deformed actors remain unimplemented. All 38
Java/GPU checks and packaging pass; the installed native payload is preserved byte for byte.

Minecraft 1.1.14-dev replaces the fixed 1-15 generated-frame slider with runtime FG capability
limits and total-output multiplier labels. Sodium ranges, pending selections and saved values
clamp per backend; Auto follows its resolved provider. JNI carries the native maximum/support
flag, and shared status excludes retired-provider capabilities during a switch. All 41 Java/GPU
checks, the native gate (44 passed, eight opt-in skips), Rust checks and an explicitly enabled
hidden GPU provider-switch fixture pass. The JAR is built and atomically installed while the
game is closed, preserving Prism's cached 1.1.11-dev filename and backing up the old bytes;
the internal version is 1.1.14-dev. Live/UI acceptance remains held by the launcher dialog.
[Details](../games/minecraft/NATIVE.md).

Minecraft 1.1.15-dev adds final GPU vertex history for buffered entities, block entities and
particles. Per-vertex object identities survive batch/index ordering; GPU growth preserves early
submissions, and final topology checks invalidate an object's earlier draws when necessary.
Complete GPU coverage suppresses the corresponding CPU replay. All 43 Java/GPU checks and JAR
packaging pass; the native payload is unchanged. Live mixin/visual acceptance remains open.
The candidate is atomically installed in the closed instance with its hash verified, preserving
Prism's cached filename and backing up 1.1.14-dev. Its low-memory dialog still awaits acknowledgement.
[Producer contracts and limitations](../games/minecraft/MOTION.md).

Imported the accepted local mod into `games/minecraft`; native texture sharing, vendor execution
and DXGI presentation use the shared runtime. The game adapter produces dense camera/geometry
motion, scales the world render target, resolves before hand/HUD and carries input frame identity
through native FG. Reese's Sodium Options uses Sodium's public Config API for runtime settings.
The packaged retail dependencies are extracted by verified content hash, without overwriting
loaded modules. Work is isolated from unrelated Wingman edits in the primary checkout.

Live DLSS reconstruction/world image acceptance and genuine PCL/mouse reports are recorded for
1.1.5. The native regression covers four SR backends, independent three-provider FG switching,
menu/abort token lifetime, threaded window ownership, resizing and pixel checks. Historical
startup/resize corrections and input research remain in [NATIVE.md](../games/minecraft/NATIVE.md).

The 1.1.6-dev cleanup reuses camera matrices/JNI status storage, compares typed settings,
formats diagnostics on demand, removes unused CPU notifications and repeated attachment arrays,
and releases native-renderer GL objects on shutdown. Twenty-two Java checks pass, including
moving-camera metadata and a real staged-JNI contract check. Windows Release verification passes
44 native checks with eight opt-in skips plus Rust format/Clippy; the separate RTX4070 GL/SDK
regression also passes. Physical controller, displayed FG cadence, frame pacing, shader packs,
FSR4 hardware and dedicated motion for GPU-only animation remain separate acceptance gaps.

## Wingman native observation, 11 October 2026

Built and deployed a dedicated XInput carrier and guarded native temporal-upscaler observer,
using the orchestrator's plugin lifecycle. Native TAA remains unchanged; borrowed graph facts
are copied without treating RDG pointers as GPU leases. Release verification passes 43 native
checks with seven hardware skips, plus Rust formatting/Clippy. The user-run desktop mission
confirms startup and 197 readable samples with a stable history owner, settled 1690x1057
render rectangle and 1707x1067 temporal output. VR is unavailable. GPU leases, reconstruction,
FG, latency, controls, HUD separation and VR routing remain required; readiness stays zero.
[Evidence, lifecycle and deployment](research/project-wingman-renderer.md#native-observer-and-desktop-run-11-october-2026).

## Wingman IDA validation and timeout correction, 11 October 2026

The shared REA/IDA configuration now permits 30-minute requests (`38ebf17`). A
fresh REA connection decompiled Wingman's native default temporal-upscaler entry
after 20 minutes 9 seconds of cold analysis. Completed analysis, Hex-Rays readiness,
normal worker/workspace cleanup and the unchanged original executable hash are
verified. The helper's raw-byte request was unsupported by the IDA adapter and
returned an explicit refusal. The old timed-out worker was stopped; policy blocked
deletion of its retained workspace. No game code, build or deployment changed.
Desktop and VR processing remain required and unimplemented; the subsequent observer is above.
[Static corroboration](research/project-wingman-renderer.md#ida-cross-check-11-october-2026)
and [tooling evidence](tooling.md#ida-pro-through-rea-and-direct-mcp).

## IDA Pro tooling and REA update, 11 October 2026

Updated repo-local REA to 6.4.0 and refreshed its shared workflow. Configured the
installed IDA Pro 9.3 through a local headless supervisor for REA, Codex and Claude
Code. Corrected Windows interpreter ownership so worker closure releases the actual
IDA process. MCP catalog/registration checks, real decompilation, unchanged input
hash and database cleanup pass. Client reconnection and Claude's project-server
approval remain required. No game build or deployment was performed.
[Setup, initial failures and evidence](tooling.md#ida-pro-through-rea-and-direct-mcp).

## AC7 FSR4 FG availability delivery, 10 October 2026

AC7's proxy calls the same shared overlay-stat helper as Unity, including the
actual-device SDK availability mask. FSR4 FG is therefore disabled when no
major-4 provider is exposed. The AC7 carrier/plugin/overlay were rebuilt and
deployed with backups; the real overlay and D3D11 provider/mask fixtures pass.
The AC7 packager now uses the dedicated carrier and includes FG SDK dependencies
and a current development README, rather than the old shared shim/SR-only payload.
Native Release verification passes 42 executed checks with seven skips. No new
AC7 gameplay run was performed for this delivery. [Packaging](dependencies.md).

## Unity UI regression, touch and FG startup, 10 October 2026

FSR4 FG is now greyed out when the loaded SDK exposes no major-4 provider for
the game's actual D3D12 device. The presentation owner caches that availability
at creation; the shared overlay intersects it with its existing choice mask.
Unsupported native requests are refused before queuing a transition. Supported
devices retain the choice. Release verification, Rust tests including the disabled
FSR4 click regression, and the RTX provider/mask fixture pass.

Live reproduction found that the managed snapshot still copied a null imported
texture and used HDR metadata format 26 for the real SDR format-28 backbuffer.
Replaced it with a native copy recorded on Unity's current pre-UI command list,
removed the separate HUD-copy ring and bumped helper ABI to 4. The local run now
survives menu/gameplay and FSR3/XeSS/DLSS-G replacements; the user confirms it
works. This supersedes the earlier snapshot proposal below. Release/managed
checks pass. Claw/touch hardware proof is separate. FSR4 ML-FG still refuses on
non-AMD devices: the pinned SDK exposes only FG3.1.6 there; the non-AMD INT8
path applies to SR. Exact live evidence and researched sources are linked below.

The subsequent first-frame freeze exposed two defects in `12bce3d`: querying an
invalid imported-backbuffer descriptor aborted graph recording, and Auto's
resolved provider was discarded during physical-chain creation. Corrected the
lookup to GetRenderTargetInfo and passed the resolved provider to creation.
The shipped-registry test reproduces the rejected descriptor call; the GPU test
now asserts the actual cold-start provider before testing switches. New game
progression remains unverified. The earlier automated pass did not cover these
two paths; see the linked correction record below.

Applied an engine-owned pre-UI snapshot instead of a later native swapchain
lookup, serialized the shared overlay draw/interop paths, and added touch/pen
plus short-tap handling. Portable packages now select FG by the owning GPU
instead of inheriting local FSR3: NVIDIA DLSS-G, Intel XeSS FG, AMD FSR3 FG.
Saved manual selections retain precedence. Release verification, shipped Mono
and CoreCLR contracts, promoted-touch input checks, and the RTX overlay/provider
fixture pass. The reported game flicker/crash and physical Claw touch result
still require a fresh game run. [Evidence and limits](research/shared-fg-20261004.md#unity-ui-regression-correction-10-october-2026).

See [current status](current-status.md) for the source/release/acceptance summary checked on
7 October 2026. This tracker preserves dated increments, test counts and superseded failures;
earlier unchecked or pending entries are not automatically current blockers. No new game/device
experiment was performed for the documentation audit; [scope and checks](documentation-audit.md).

## REA development tooling, 8 October 2026

Installed REA 6.0.0 with a locked repository-local npm dependency, a shared MIT
workflow, and project registrations for Codex and Claude Code. Both registered
stdio launch commands pass initialization, the 139-tool/six-prompt catalog check,
and a target-free session request. Existing Ghidra 12.1.4/JDK provider readiness
passes. Claude Code still needs its normal project-server approval, and both
clients need a restart/reconnection. No runtime build, game analysis or deployment
was performed. [Setup and evidence](tooling.md#rea-for-codex-and-claude-code).

## AC7 shared frame generation, 5 October 2026

Built and deployed the AC7 adapter correction for DLSS-G, FSR3/FSR4 and XeSS switching.
Completed SDR scene colour is now required and supplied as HUD-less colour, so FidelityFX
uses its UI extraction/composition path. Captured resources stay in their declared read
states until provider interpolation completes. A separate completion fence on the physical
presentation queue authorizes D3D11 reuse. Native command-list aliases are used for non-
Streamline queues. Off diagnostics no longer dereference an absent provider, and provider
transitions reset pacing-counter baselines. Non-NVIDIA adapters can create FSR/XeSS/Off
presentation without initializing a NVIDIA host.

The extended AC7 CPU/RHI/window fixture passes 120 sampled/direct frames with DLSS SR,
repeated DLSS/FSR3/XeSS/Off provider changes and 2x/4x requests clamped to queried limits.
It records 24 active FSR samples and 20 XeSS samples. DLSS activity was not established:
Windows denied the fixture foreground ownership. The RTX 4070 Laptop SDKs report one
generated frame; higher MFG counts and FSR4 execution require suitable hardware.
Release verification passes 42 executed native tests, seven opt-in skips and Rust
formatting/Clippy. Deployment includes matched binaries, signed dependencies and backups.
New AC7 gameplay switching, HUD/image quality and DLSS reactivation are pending a user run.
[Research and exact validation](research/shared-fg-20261004.md#ac7-adapter-completion-5-october-2026).

## Shared frame generation, 4 October 2026

Current acceptance: the user confirms the final Drag'n Wash corrections resolved the FSR
image defect, after previously accepting XeSS and DLSS-G. Runtime provider changes and
SDK-reported multiplier controls are deployed. Full Release verification passes 42 executed
native checks with seven opt-in skips; 58 Rust tests and shipped-Mono checks pass. AC7's
existing DLSS behavior is user-accepted; its new FSR/XeSS live acceptance and higher MFG
counts on suitable hardware remain separate verification gaps. The entries below preserve
the failures and corrections that led to the accepted Unity result.

The user accepts AC7's earlier DLSS-FG behavior. Both game adapters now route independent
SR and FG selection through a stable presentation facade, with provider replacement after
a drained Present. Unity includes shared DLSS SR/FG registration, source CPU tokens and
completed HUD-less colour plus normalized depth/motion. Multiplier controls use queried
DLSS/XeSS limits; the pinned AMD API provides 2x. Unsupported creation retains the current
provider. Controls no longer request a restart.
[Implementation and evidence](research/shared-fg-20261004.md).

The first Unity run crashed; the imported RTHandle null dereference was repaired. The next
run still had inactive FG and an incorrectly disabled DLSS choice. Corrected the render-to-
Present thread handoff by binding identity to engine buffers, shared Streamline registration,
missing FG dependencies, token retirement on refused inputs, and a log sharing conflict.
Release verification passes 42 executed native tests with seven opt-in skips and 58 Rust
tests. CoreCLR/shipped Mono checks pass. Cross-thread hardware fixtures pass runtime provider
handoffs with FSR3/XeSS activity and retained engine buffers. Shared DLSS SR evaluates and
passes GPU readback. The hidden native DLSS FG fixture verifies tags and handoffs, without
activity proof; the D3D11 fixture reports DLSS activity. Both installs contain the corrected
Release files with backups; all 277 original Unity files are unchanged. Live game activation,
runtime switching, moving-scene/HUD quality and MFG on capable hardware remain unaccepted.

Latest user run confirms successful live switches and XeSS generation by RTSS, but reports
inactive alternatives. The log exposes conflicting shared DLSS SR/FG viewport-zero constants;
separated their viewports and verified both on one SDK token with GPU readback. Fixed captured
input read states through asynchronous Present, and added bounded FSR preparation/dispatch
activity logs. The mixed-resolution device fixture and full Release gate pass; corrections
are deployed to both games. FSR activation in the user's run remains unexplained until fresh
activity evidence is available. DLSS recovery and image quality still need the next game run.

20:05 live correction: startup succeeds after native queue ownership repair; a subsequent
switch crashes in the bridge's unguarded GetFullscreenState. Protected concurrent queries
and physical publication, preserved application counts, and verified roughly three million
queries across switches. Live FSR logs confirm generation; added its measured DXGI counter
and Unity FPS clock samples. DLSS's ReflexNotDetected failure requires effective Reflex On
while active. Built, gated and deployed these corrections. Live switch stability, DLSS
activity and FSR pacing acceptance are still open.

## Unity Mono research and shared plugin direction, 3 October 2026

4 October increment: shared drop-in shim, Mono/Harmony URP hooks, native DX12 SR and the
shared overlay are built and deployed. All 277 original Drag'n Wash files remain unchanged.
Live logs confirm overlay opening and vendor/quality requests reaching all temporal providers.
Five-provider GPU readback and 55 Rust tests pass; moving-scene SR and image acceptance remain
pending. AA-copy ordering and build-directory version-alias collisions were corrected.
The next live run confirms DLSS result 0 with accepted frames increasing and no refusals,
after fixing truncated render extents and Unity's D32_FLOAT_S8X24 depth view. Full VS2026
Release verification passes: 37 native tests, six opt-in skips, Rust format and Clippy.
[Implementation and evidence](research/unity-dx12-runtime-20261004.md).

User-reported distant-object shimmer in FSR2/FSR3/XeSS: corrected the Unity DX12 adapter's
Y-jitter mapping against AMD's Unity integration and Intel's API convention. Added bounded
history-reset/jitter logs. Visual improvement awaits the next matched game comparison.

The supplied static-skyline recording rejects that first sign correction. Replaced the
pre-GPU sign assumption with measured post-conversion raster jitter and removed the extra
FSR/XeSS flip. Bounded GPU motion readback reports zero motion in four sampled regions;
settled XeSS logs show continuous history and successful evaluations. Built, verified and
deployed; the shimmer is still an open visual defect pending the new comparison.

Corrected the SR boundary after inspecting the shipped post stack: DoF preceded the original
STP slot. Temporal SR now replaces the incoming HDR scene at post-processing entry and feeds
output-size DoF/blur/bloom/tonemap; the later STP hook avoids duplicate evaluation. Managed
build and shipped-Mono contract checks pass; deployed. Visual shimmer/edge acceptance pending.

The user confirms the post-processing order is much better. Packaged the hash-verified deployed
snapshot for MSI Claw testing, defaulting to XeSS Quality and including all vendor runtimes,
licenses, relative configuration, per-file checksums and install/uninstall instructions.
Claw/device acceptance and quantitative image comparisons remain pending.

Added shared startup GPU policy: Auto resolves the adapter owning the native D3D12 device,
then selects NVIDIA DLSS, Intel XeSS or AMD FSR3. Package/deployment defaults use Auto;
manual overlay choices override it for the session. Preserved DirectInput ordinal 1 in the
common shim and added a real Windows forwarding fixture. Clean staged-source verification
is separate from the concurrent full workspace gate. AMD RDNA4 auto-classification is deferred;
FSR4 remains a manual experimental choice. Claw hardware acceptance remains pending.

- [x] Fingerprint Drag'n Wash Steam build `25286774`, Unity `6000.3.14f1`, Mono and packaged
      URP Forward+/RenderGraph path. Verify player/executable PDB GUID/age pairing. Completely
      decompile 12 selected game/renderer assemblies into ignored references. Parse selected
      pipeline/camera/canvas settings with strict reads and retain parser failures.
      [Research](research/drag-n-wash-renderer.md) and [game evidence](../games/drag-n-wash/engine.json).
- [x] Record the shared Unity Mono plugin direction using reflection and Harmony, with pipeline
      adapters and the existing native orchestrator. Map camera policy, temporal production,
      pre-tonemap reconstruction and SDR overlay boundaries from the shipped managed code.
      [Design and validation sequence](research/unity-mono-plugin.md).
- [x] Add `rsf_game_unity_mono`, producing `ReScaleFrame.Game.UnityMono.dll` through the existing
      game-plugin ABI. The researched Drag'n Wash name/hash/architecture is the initial allowlist.
      The initial scaffold refused preparation/start without retaining services; it has since been
      replaced by the Mono/Harmony lifecycle described above. Rendering readiness stays false.
- [x] Build and synthetic-test the scaffold on Windows/MSVC with
      `eng/verify.ps1 -Configuration Release -VS2026`: the repository gate passed with six
      expected environment/vendor skips; Rust formatting and Clippy passed. The Unity fixture verifies
      detection, ABI/size guards, inactive lifecycle and repeated cleanup. This is not a Mono,
      Harmony, graphics-device or game test.
- [x] Implement the managed bootstrap in the player's existing Mono domain and guarded Harmony
      URP contracts for the fingerprinted player. Broad discovery across other players remains open.
- [x] Implement native copy-through, fences, frame identity, temporal inputs and output-resolution
      SR reinsertion with managed/device fixtures and recorded live evaluation. General transitions
      and resize/teardown acceptance remain separate coverage.
- [ ] Game-test custom fluid/deformation coverage, scene/settings/resize transitions, HUD-less/UI
      boundaries, teardown, FG and latency. No game launch, injection or installation change
      occurred during the initial static investigation. Later live SR and user-accepted FG corrections
      are recorded above; complete scene/resize/Claw coverage and broad readiness remain open.

2 October documentation: repository skills now separate general renderer analysis, shared
Unreal 4 concepts, UE4.18/AC7 evidence and TrueSky production. The parent transcript, accepted
implementation and failed experiments were reviewed; metadata/local-reference/whitespace
checks pass. This is documentation validation, not a new game/device test.
[Scope and checks](research/skills-session-update-20261002.md).

ReScaleFrame is a monorepo. All first-party components share this history and release version. Separate runtime/plugin DLLs do not imply separate repositories.

## Project Wingman scaffold and static research, 3 October 2026

- [x] Fingerprint the examined x64 executable and match its UE4.27.2 renderer against pinned
      source and symbol-backed references. Twenty native functions named/annotated in Ghidra.
      Temporal input/history/jitter fields, widget target owners and stereo texture setter are
      statically identified. [Research and hook map](research/project-wingman-renderer.md).
- [x] Add `rsf_game_project_wingman`, producing `ReScaleFrame.Game.ProjectWingman.dll` through
      the existing game-plugin ABI. Exact name/hash/architecture detection is separate from
      readiness. Preparation and activation refuse with `RSF_ERROR_NOT_READY`; status stays
      inactive and unprepared. No hooks, retained host services or game-installation changes.
      [Plugin guide](../games/project-wingman/README.md) and
      [engine evidence](../games/project-wingman/engine.json).
- [x] Build and synthetic-test on Windows/MSVC with
      `eng/verify.ps1 -Configuration Release -VS2026`: 34 tests executed, five skipped; Rust
      formatting and Clippy passed. Both game-plugin DLL contract tests pass. The new fixture
      checks identity, wrong-build/architecture refusal, ABI/size guards, inactive lifecycle and
      repeated cleanup. This is test-process DLL validation, not a game or graphics-device test.
- [ ] Identify effective render-scale policy and verify live temporal input formats, camera/history
      conventions, per-view/eye identity and RDG/RHI resource lifetimes.
- [ ] Validate expected hook bytes and implement native SR/fallback output reinsertion.
- [ ] Trace all live desktop/VR HUD routes and establish a completed HUD-less image and native
      reinsertion. Separate widget textures alone do not complete this requirement.
- [ ] Implement and game-test loading, renderer lifecycle, scale/scene/resize transitions and VR.
      Project Wingman rendering, FG and latency support remain unimplemented.

## Active AC7 scope correction, 1 October 2026

2 October release acceptance: the user confirms the latest 22:17 deployment works and requests
replacement of the broken v0.1.0 GitHub package. The previously accepted wing/cloud plugin is
unchanged; the optimized GPU-fence bridge and FSR4 INT8 compatibility are now accepted in game.
This is user-reported visual acceptance, separate from the explicit RTX4070 device fixtures.
The package now includes the proxy, AC7 plugin, overlay, DLSS, FSR 2/3/4 and XeSS SR runtimes,
all accompanying notices, exact source and checksums. Frame generation and Reflex remain deferred.
[Research and validation](research/sr-interop-performance-20261002.md).

Release follow-up: startup hotkey hint added through overlay ABI5. It draws above the completed
frame for eight seconds, fades over the final second and is dismissed on opening the panel.
No mouse input, settings changes or reconstruction participation. Native Release gate passes
33 executed tests/five skips; Rust workspace tests pass53. Actual Rust overlay host verifies hint
pixels with panel closed, Insert toggling, settings panel rendering and resize. The game plugin
hash remains unchanged; proxy/overlay hashes change for the hint. No agent AC7 launch.

2 October FSR4 compatibility: implemented a fingerprint-guarded, device-leased INT8 capability
hook in the installed SDK4.1.1.2740. This targets the native decision behind OptiScaler's scoped
adapter/driver substitutions, leaving actual GPU identity and FP8 checks intact. RTX4070 output
watermark confirms FSR4-I8 at3x;80 frozen-input Ultra frames and84 lifecycle fixture frames pass.
The D3D12 bridge uses GPU ordering and three command slots. User acceptance is recorded above;
no new measured in-game FPS comparison was supplied.
[Mechanism and proof](research/sr-interop-performance-20261002.md#fsr4-int8-compatibility-implementation).

2 October21:30 user acceptance: wing artifacts and cloud blockiness are clean. The latest log
confirms native Texture2D/RG32F validation, divisor1 activation and one-to-one depth dispatch.
Follow-up XeSS/FSR performance work replaces current-frame CPU transfer waits with GPU fence
ordering and three guarded command slots.240 standalone Ultra evaluations produce byte-identical
outputs before/after; lifecycle and full-pipeline hardware checks pass. The initial official FSR4
refusal on RTX4070 was subsequently resolved by the fingerprint-guarded INT8 hook recorded above.
[Evidence and limits](research/sr-interop-performance-20261002.md).

2 October wing/cloud follow-up: reviewed the earlier transcript and20:13 captures before changing
the implementation. The20:05 kernel never activated because its guard required an array UAV;
TrueSky's texture owner creates a Texture2D UAV. Corrected the one-to-one kernel and guard to
that native layout. A new expected-byte patch at TrueSky RVA0xbfc09 promotes only normalized
scene-depth bounds from R16G16_UNORM to R32G32_FLOAT through the native allocation owner.
Captured aircraft depth has up to8.57% relative error in UNORM GPU output; RG32F reduces it below
0.000034% in the standalone replay. Full scene-grid clouds at Ultra exceed the user's observed
stock Balanced cloud-resolution floor. Installed21:24, backup `ac7-pair-20261002-212445-314`.
Release gate33 passed/five skips, Rust format/lint and real overlay Insert/render/resize passed.
No AC7 launch; live wing/cloud acceptance remains pending. [Evidence](research/ac7-plane-artifacts-20261002.md).

2 October flight regression: user confirms clean aircraft and corrected hangar exposure after19:10,
but sky/lighting/volumetric clouds are broken. New captures locate the damage before DLSS.
TrueSky divisor1 skipped its x2/x3/x4 depth-pass selection while still dispatching and cleaning up.
Added a guarded native pass/call correction and first-party one-to-one normalized depth kernel;
full resolution waits for actual shader/binding readiness. Original shader and native cleanup are
preserved. Explicit NVIDIA GPU comparison passes for x1 and shipped x2. Flight visual acceptance
is pending; accepted aircraft/colour/FP16 fixes retained. Installed20:05, backup
`ac7-pair-20261002-200538-588`; Release gate33 passed with five skips, Rust format/lint and real
overlay Insert/render/resize preflight passed. No AC7 launch. [Evidence](research/ac7-plane-artifacts-20261002.md).

2 October rendering correction: all three current defects are priority-one blockers. Implemented
native TrueSky scene-grid production with a guarded DLL instruction patch and native FP16 scene
colour allocation. Added a depth-guarded GPU colour correction after DLSS and before native bloom/
tonemap, without changing presets. Frozen Performance GPU replay removes broad bands; Ultra
flight replay preserves silhouettes after guarding. Native cloud/precision changes are built,
not yet game-accepted. Installed19:10 with backup `ac7-pair-20261002-191007-200`. MSVC Release
gate passes33 executed tests, five vendor/environment skips; Rust format/clippy and actual overlay
Insert/pixel/resize preflight pass. Installed hashes match the build. No autonomous game launch or new test target. Full evidence and failed
variants are in [the artifact research](research/ac7-plane-artifacts-20261002.md).

2 October retest: aircraft, distant cloud blocks and DLSS Performance/Ultra exposure defects
persist. The00:09 ordinary-translucency correction is not the artifact fix. Complete mid-frame
D3D11 state preservation is built and device-tested in a standalone binding/counter experiment.
F9 now pairs the TrueSky composite's colour/loss outputs and records its cloud/depth inputs;
CB readbacks select shader-declared slots. Release gate passes33 tests with five environment
skips, format/lint pass. No new test target or game launch. Artifact and exposure fixes remain
game-unverified. [Evidence](research/ac7-plane-artifacts-20261002.md).

Installed10:22, backup `ac7-pair-20261002-102210-392`; real-overlay Insert/render/resize preflight
passes. Game was not launched. Await paired F9 cloud/aircraft comparisons with SR off and Ultra.

2 October 00:09: updated goal is gameplay aircraft artifacts, then motion-vector improvements.
Latest flight sample has affected surfaces in depth/written motion and a pre-reconstruction colour
defect. Removed obsolete global separate-translucency enlargement; native ordinary sizing restored,
scoped UI preparation retained. F9 skips inactive stages and budgets readbacks per sample. Release
and overlay preflight pass; plane/UI retest pending. [Evidence](research/ac7-plane-artifacts-20261002.md).
User confirms background/cloud stability and blur improved. DLSS banding remains separate.

23:19 build installed after user confirms stable background/clouds. Bloom common-source
rewiring now generates native bloom/exposure from reconstructed scene; source identity
must match or late path remains. Native view-state jitter cycles scale with output/render
area (32/72 at Performance/Ultra), replacing ineffective console tuning under native ownership.
F9 saves exposure guide. Release/overlay preflight pass. Bloom/edge/exposure retest pending;
aircraft colour banding and cloud-overlap smear still open. See stability research note.

22:52 stability corrections installed. User confirms hangar crash no longer reproduced.
FSR/XeSS packed scene-colour rejection corrected with explicit linear RGBA16 conversion.
TrueSky native projection adapter now preserves vertical jitter handedness for matched primary
views. DLSS receives explicit-sentinel dense motion to preserve valid zero aircraft vectors.
F9 includes submitted motion. MSVC and shader compilation plus overlay preflight pass;
visual/backend retest pending. [Evidence and limits](research/ac7-stability-20261001.md).

22:34 correction deployed: CPU trace maps null PS View slot to worker BasePass material
recording. Saving shared View uniforms by move cleared their slots during replacement.
Save now takes an owned reference, publishes from a local replacement, and joins native
outstanding recording tasks before shared view changes/restoration. MSVC Release build and
overlay Insert/render/resize preflight passed. Hangar crash and shimmer retest pending;
no autonomous game launch. See the native renderer research note for evidence and limits.

Hangar retest: fixed camera-pan position produces explicit PS slot1=null before indexed draw,
not merely a missing binding at commit. Shader SHA1 and expected layout recorded in research.
Mapped four exact CPU command creators; bounded null-slot CPU stack probes installed. Release
build/overlay preflight pass. Crash still unresolved; user-driven reproduction required. No game launch.

21:17 crash persists at the same PS uniform read. No null View-helper argument recorded;
missing buffer type remains unproven. Direct RHI pixel setter/commit observers now record
expected layout hash and preceding bindings. Diagnostic build only; crash remains a blocker.
User explicitly prohibits autonomous AC7 launches. Preserve SR/UI corrections; cloud work deferred.

Crash blocker only: successful533x300 SR confirmed, same null PS uniform crash recurred.
Corrected scaled-uniform availability before all participating translucent views and explicit
ownership of generated view uniforms through queued RHI completion. Raw queued uniform
pointers were previously released at CPU view restoration. Bounded null-binder diagnostics
added. Release builds and overlay deployment preflight pass; gameplay crash fix unvalidated.
Cloud investigation deferred until the crash is resolved.

User confirms all UI fixed by the native UnmodifiedTranslucency producer build. Ultra Performance
was refused: active view536x300 exceeded exact DLSS533x300. Corrected active rect sizing versus
allocation padding and completed UI normal/scaled uniform selector ownership. Release built;
Ultra Performance and crash retest pending. Dump confirms null PS uniform slot1; its exact bind
producer is unproven, so the source omission correction is not yet a game-confirmed crash fix.

UnmodifiedTranslucency correction now implemented: native begin/resolve/render wrapper owns
output-size allocation/viewport, engine-resampled matching depth and unjittered native uniform,
with saved scene state restored after resolve. MSVC Release built; UI game retest pending.
Eight-frame opt-in native route snapshots added for cloud jitter attribution. Cloud fix remains open.

Retest correction: briefing is user-confirmed sharp, other UI remains low resolution/jittery.
The ordinary separate-translucency allocation hook does not own AC7 UnmodifiedTranslucency.
Mapped its actual allocation/begin/resolve and render-pass callers in Ghidra; full-resolution
depth/view/resolve ownership remains to implement. Cloud video confirms vertical instability;
cause unresolved. Do not report all UI fixed or broaden back into FG/latency.

Eleven new captures isolate a scene-sized UI/translucency branch despite full-output SR.
Native SetSeparateTranslucencyBufferSize ownership implemented at guarded RVA 0x10be2b0;
actual scene/output dimensions drive allocation before rasterization. MSVC Release built;
corrected UI capture validation pending. Renamed misleading pre-HUD capture label to UI branch input.

After the launch-crash correction, AC7 logged successful native SR evaluations but the user
reports blurry/pixelated output. Corrected stale compatibility-derived overlay status and added
bounded F9 SR/backend/graph/downstream images. MSVC Release built; visual defect remains open.

First ABI10 game run crashed in native SR resource retention. VelocityRT was incorrectly treated
as an RHI texture instead of a pooled target. Corrected the pool-to-texture lookup, built Release
and redeployed matching DLLs with overlay preflight passing. AC7 relaunch remains pending.
The crash mechanism and evidence are recorded in the native renderer refactor notes.

The immediate goal is correct super resolution and improved motion-vector coverage. The native
refactor expanded into latency and frame-generation infrastructure before its SR path had been
validated in AC7. That expansion is deferred: Reflex/PCL wiring, FG/MFG handoff and broader
presentation tracing are not prerequisites for this goal. Preserve existing work without extending
it unless a measured SR defect requires it.

The earlier compatibility path has user-accepted upscaling and a user-confirmed lighting/reflection
correction. The newer engine-owned SR/UI implementation builds with MSVC Release but has not been
deployed or game-validated. Neither result proves that current UI blur, temporal instability or
hangar banding is resolved. Motion research establishes native encoding, camera-transform leads
and velocity eligibility sites; missing-object coverage is still unvalidated.

Next, audit only the native SR enablement and input/output path against the existing captures and
matching UE4 source. Establish one bounded candidate for game validation: intact scene colour and
postprocessing, matching depth/motion/view identity, correct jitter and history, and output-size UI.
Do not add another generic architecture layer. Prepare a matching DLL set and exact capture
instructions once that path is reviewable. Compare native and reduced-resolution briefing,
hangar, flight and pause/resume before declaring an improvement.

Then resolve measured motion conventions and coverage, starting with carrier-launch/refuelling
weapons and attached missiles, followed by moving vehicles. Match colour draws to velocity writes
or a verified rejection reason before changing the producer. Audit TrueSky separately using its
depth/history and stationary-camera evidence. No universal cloud or object-motion fix is claimed.

This scope correction is documentation-only; it adds no runtime experiment or build result.

Deployment on 1 October: explicitly installed the current Release proxy, ABI10 AC7 plugin and
matching Rust overlay in the user-named AC7 installation. Installed SHA256 values match build
outputs. The deployment preflight loaded the real overlay and passed Insert open/close, visible
pixel rendering and swap-chain resize. Matching FSR/XeSS runtimes and notices were also installed
by the deployment procedure. Previous files are backed up under
`.local/deploy-backups/ac7-pair-20261001-195716-425` and
`.local/deploy-backups/ac7-sr-20261001-195716-201`. Native SR and UI behavior in AC7 remain unvalidated;
this overlay preflight is not a gameplay test.

## AC7 release

- [x] Prepare the 0.1.0 Windows x64 package with the accepted proxy/overlay, retail DLSS runtime,
      dependency notices, source-commit manifest and archive checksums. Installation, usage,
      supported scope and removal are documented in the release notes and packaged README.
      Packaging does not change rendering code. [Packaging procedure](dependencies.md#packaging-the-ac7-release).
- [x] Published [v0.1.0 for AC7](https://github.com/KillerPixelCrew/ReScaleFrame/releases/tag/v0.1.0)
      from `4386a9079d68df3f9f26e55f69d1a01f42bcb306`. Windows package, corresponding-source ZIP
      and SHA256SUMS uploaded; GitHub asset digests match the local files and the downloaded
      checksum file matches. Packaged retail runtime initialized and reported DLSS support in the
      hardware load/query test. Framework and AC7 READMEs are separated.

## Repository foundation

- [x] Native CMake targets for bootstrap, orchestrator, AC7 plugin, and launcher.
- [x] Minimal C ABI for plugin metadata and executable recognition.
- [x] Rust workspace with an egui status surface.
- [x] Architecture, UE4.18 source leads, and presentation research retained in the repository.
- [x] Local Release build, C/C++ SDK compatibility, plugin contract checks, Rust formatting, and Clippy verification.
- [x] GitHub Actions workflow for native and Rust verification; individual run results are tracked in Actions.
      Every run from 7 to 8 September failed at the first MSVC warning, the secure-CRT deprecation
      of `fopen`, because nothing in the tree had ever been built with MSVC. Fixed 26 September
      along with the three other things MSVC refused: a test variable named `small`, which a
      Windows header defines as a macro; a fixture that added `dllexport` to functions its header
      declared plainly; and a missing `stdlib.h`. The suite then passes on Windows, 22 of 22, with
      Visual Studio 2026 through the `-VS2026` presets. CI has not run since.
- [x] The frame tap survives the Windows D3D11 runtime. First MSVC run of `tests/frame_tap.cpp`
      failed 23 checks that had always passed under Wine, and the cause is a property of the stock
      runtime rather than of the tree: it rewrites the work-submission entries of its heap vtable on
      every flush, which discards patched hooks, so no Windows run of this project could ever have
      observed a draw after the first read-back. Measured with a probe on 26 September, fixed by a
      sentinel check on every hook entry and pass-through hooks on the flush-class calls, tap ABI 8.
      [Evidence](research/d3d11-runtime-vtable-rewrite.md).

## Native AC7 renderer refactor

- [x] SDK ABI 2 and runtime-owned plugin prepare/start/quiesce/stop/status; native preparation
      precedes graphics activation. Recognition still does not report rendering readiness.
- [x] Renderer-owned view sizing before allocation, native temporal preparation, pre-tonemap
      graph SR node with native spatial fallback and auxiliary dependencies, downstream
      descriptor/rectangle/uniform ownership and FXAA bypass. Native mode bypasses the binding
      matcher, GPU constant-buffer rewriting and UI/chain/reinsert ageing timers.
- [x] Copied queued RHI identity, render-thread pool-reference retirement and F9 native execution
      records. Windows MSVC Release gate: 33 passed, five explicit vendor/device experiments skipped.
      Independent Windows WARP graph/queue/pixel tests passed; no new AC7 run yet.
- [ ] Validate this native path in AC7: briefing, hangar, flight, pause/resume, preset transitions,
      lighting/reflections, UI sharpness and temporal stability. `rendering_ready` remains zero.
- [x] Continued source implementation: runtime-owned native configuration, transactional output
      resizing and a leased engine eye-adaptation guide converted to scalar exposure. Focused
      MSVC Release build succeeds; new image behavior and backend resize paths are not game-validated.
- [x] Lifecycle source integration: failed-prepare module retention, calls outside lifecycle locks,
      transition/status reader ownership, actual controller activity and deferred graphics-owner
      shutdown before backend release. MSVC Release build passes; live teardown remains unverified.
- [x] Cropped-view source path: region-local colour/depth/motion guides, rect-local backend
      evaluation, native rect reinsertion, rectangle history resets and separate whole-surface
      configuration. Primary constrained views are scaled from renderer-owned surface dimensions.
      MSVC Release build passes; actual cropped image behavior remains unverified.
- [ ] Complete native UI producer/isolation ownership, carrier cleanup and
      lifecycle integration. The active refactor goal remains unfinished.
- [ ] Validate exposure input ownership, output resizing, native UI isolation for
      FG and simulation-frame handoff. Velocity coverage/cloud reprojection remains separate work.
      [Implementation and evidence](research/ac7-native-renderer-refactor-20261001.md).

## First working target

- [x] AC7 upscaling accepted by the user on 29 September 2026, including stable full-output
      briefing terrain/icons, automatic startup/reinsertion and the simplified Insert overlay.
      The final capture verifies zero-jitter view twins in the terrain's hull/domain stages as
      well as VS/PS. Temporary automatic capture disabled after acceptance.
      [Final result and scope](research/ac7-consumer-session.md#accepted-result).


- [x] Decrypted module capture and import map. Cross-built, Wine-tested, and run in AC7 on 6 September; that run was not MSVC-verified. [Evidence](research/ghidra-tooling.md).
- [x] AC7 frame/resource analysis and live view-buffer mapping. Captures identify sparse velocity and a separate half-size mask, plus a temporal-filter candidate. [Evidence](research/ac7-frame-capture.md).
- [x] Temporal jitter enabled through the engine AA gate and measured in perspective views. Game-tested; the recorded patch build was not MSVC-verified.
- [x] Console-variable access and `r.ScreenPercentage` changes verified in game; the recorded build was not MSVC-verified.
- [x] Rust input model: quality levels, UE motion encoding, and all-blocker viability reporting. Unit-tested; no GPU calls. [Backend guide](../runtime/backends/README.md).
- [x] Rust jitter/motion unit conversions and scale-dependent jitter sequence length. Unit-tested against recorded values; live C/C++ sign/scale agreement remains open below.
- [x] Jitter sample count follows render scale; a timer restores scale after mission loading. Game-tested through a full mission.
- [x] Streamline DLSS adapter: loading, device handover, support/render-size queries, tags, constants, and evaluation. Runs in AC7.
- [x] View layout mapped through matrix identities, including camera basis, projection, and `ClipToPrevClip`. Offline check: 11 captured perspective views, no failures.
- [x] D3D11 motion decode with bias/scale parameters, unwritten sentinel, and compute-state handling. Cross-built and tested under Wine/DXVK with source-encoded values and an axis flip. Game-buffer dumps matched the reference range and unwritten fraction.
- [x] AC7 view reader with matrix/size checks, main-view classification, pixel jitter, and unjittered projection. Recorded dataset: 50 recognized buffers, ten perspective views, secondary views marked. [Review](review.md) identifies remaining validation defects.
- [x] `TemporalAAJitter` located at `0x720` by comparing pre/post-patch captures, then checked against projection entries and live pixel offsets.
- [x] Orchestrator frame assembly converts plugin camera/resource data into a DLSS frame and rejects unusable combinations. Unit-tested without GPU work.
- [x] Live DLSS evaluation. Initial run: 2,176 recognized passes, 2,175 evaluations, no refusals. Mission run on 7 September: 7,917 evaluations, no refusals, 1024Ã—576 input and 2048Ã—1152 output. Recorded images show recovered detail and a complete scene; flight showed no obvious smearing. F7 is a debug display; reinsertion, grading, HUD, and controlled motion validation remain pending. [Evidence](research/ac7-frame-capture.md).
- [x] Conditional composed-colour selection for the DLSS bridge. Persistent input watch and AC7
      recombine rule are cross-built and synthetic-tested under Wine. Each frame falls back to the
      identified colour unless a matching composition is observed; no new per-frame allocations.
      Briefing relief presence was reported game-tested on 7 September. Reconstruction quality,
      hangar/flight regression, resource overwrite timing, and F6 remain open. [Decision and checks](research/ac7-composed-scene-color.md).
- [x] Conditional translucent depth for the research DLSS bridge. IA/VS shadowing and immediate
      depth-only replay use startup allocations and restore disturbed state. Same-frame composed
      layer/source-depth identity gates backend selection; absent layers keep original depth.
      Synthetic pixel readback covers accumulation, opaque occlusion, frame reset and state
      restoration under Wine. Game-tested in the briefing on 7 September: 733,861 of 744,726
      candidate draws replayed. Flight regression remains unverified.
      [Decision and checks](research/ac7-translucent-depth.md).
- [x] Separate translucency at its own resolution. 4.18 halves the layer and doubles it back on
      composite, so at a 50% render scale the briefing relief reached any reconstruction as a
      quarter-resolution image. Patching the halving out fixed the relief and the cannon tracers at
      once. The scale is now a four-byte immediate inside the patched instruction, aligned so it can
      be rewritten while the game runs, and derived from the render scale in effect so it follows a
      quality level instead of being fixed. Going above the scene's resolution needs the engine to
      size the layer's depth to match, which four one-byte patches enable by narrowing `Scale < 1.f`
      to `Scale == 1.f`; they are no-ops for every scale the engine produces on its own.
      Game-tested on 7 September: the briefing relief draws at 2048Ã—1152 inside a 1024Ã—576 scene
      with a matching depth, and reaches the reconstruction. Flight, the post-mission replay and
      other heavy screens are unverified. [Evidence](research/ac7-frame-capture.md).
- [x] Consumer session controls: automatic DLSS startup and deferred reinsertion; Insert opens an
      overlay containing only Enable DLSS and presets. Choices persist per user. Repeated preset
      changes use the last applied screen percentage, reset history and rebuild reinsertion;
      disable restores native rendering. Built and synthetic-tested on Windows; game validation
      pending. [Evidence and checks](research/ac7-consumer-session.md).
- [x] Default separate-translucency path renders at nominal full output resolution across
      scene presets and uses unjittered VS/PS view twins before direct recombine. The previous 1:1
      DLSS layer resolve is opt-in. Built and synthetic-tested; engine padding, pass classification,
      the user reported the half-output briefing looks good; the requested full-output sharpness
      change and flight regression still require a new game run.
      [Decision and limits](research/ac7-consumer-session.md).
- [x] Correct pixel-to-world unjitter math after the full-output terrain jitter report. The old
      helper and fixture both applied the linear-depth screen-matrix correction to device-depth
      SVPosition. Corrected fixture fails before the fix; scaled-view/depth regressions and the
      Windows Release gate pass. Full-output visual retest pending.
      [Evidence](research/ac7-consumer-session.md#full-output-jitter-pixel-to-world-unjitter-correction).
- [x] Captured and corrected enlarged-layer view/depth selection: 1600x904 terrain draws had
      800x452 views despite unjittered twins. Seven further engine gates now select the sized view
      and sampled depth above scale one; a follow-up game capture verifies 1600x904 bound views.
      This alone did not resolve the user's shimmer report.
- [x] Extend unjitter to tessellation and geometry stages (tap ABI 13). The captured terrain uses
      12-control-point patches and a VS that hands positions to DS. Synthetic tessellation/GS
      pixel readback passes; the final all-stage capture and user visual acceptance confirm the briefing fix. Prevent
      stale view twins from replacing smaller material allocations after address reuse.
      [Evidence](research/ac7-consumer-session.md#captured-enlarged-view-mismatch-and-tessellation-gap).
- [ ] Game-validate automatic startup, all five presets, disable/re-enable, saved choices, and the
      briefing layer's fixed visible dimensions and temporal stability.
- [ ] Reinsert the result. A debug view exists behind F7 and is game-tested: a full screen draw over
      the back buffer from inside the Present hook, with a rough tonemap so linear scene colour is
      viewable. It is what showed the reconstruction moving, which is the only way ghosting and a
      motion vector's sign can be judged. It is not the real path, which reinserts the reconstructed
      scene before the game's own composite so the grade and the interface survive. That is the
      remaining structural piece and the reason the picture is ungraded and has no HUD.

      The whole path is built behind F6 and has been run against the game. It took three runs to get
      there and each failed differently, which is worth keeping because the failures were all the
      same mistake about bindings.

      First run: nothing happened at all. The tail walk had taken a 2048Ã—32 strip as the composite,
      a UI bar the final draw also reads, and promoted that. Zero gates opened, so F6 did precisely
      nothing. Fixed by rejecting any input less than half the height of the target it is drawn into.

      Second run: the panel said no tail. The rule required the final draw to have exactly one input,
      and it has seven, because D3D11 leaves shader resource slots bound until something replaces
      them. `frame_tap.h` warns about this in as many words and the tail code ignored it, so the tail
      had never been identified in any run. This is the same failure as the composite selection, the
      interface format and the layer identity: a running game binds more than it reads.

      Third run: the tail is found and reinsertion runs, and it blew up the interface. Later runs
      the same day fixed the blow-up (the plan went stale on screen changes and is now restaked)
      and left the picture cleaner but the interface soft, with the briefing's 3D environment
      missing under F6.

      Superseded, 7 September, and then reversed. The interface was never a target to promote by
      format: AC7 rasterizes it at a fixed 1920x1080 and, on the briefing and hangar, draws it as
      world-space widget quads into its own render-resolution `R8G8B8A8` layer, depth-tested
      against the scene, before compositing it itself. Extraction into a layer of our own was built
      and run under M2 below, and it showed the opposite of what was assumed here: the quads read
      the scene and its glow chain and the game keeps processing them, so taking them out
      discolours the frame. Promoting the layer they draw into, with the viewport scaled, is the
      route again, now as `scene_promote` with the layers named by the classifier rather than by a
      rule about formats. The account of the three runs stays because the failures were all the
      same mistake about bindings.

      Finding the tail: the frame tap shadows the output merger and describes the draws into a
      render target it is asked to watch, with extent, viewport, ordinal within the pass, and every
      pixel shader input with its slot number. The loader points it at the swap chain's back buffer,
      takes the composite among what that draw reads (eight-bit colour, at least half the target's
      height), then watches that. An "interface target" used to be picked out of the composite's
      inputs by its format; that rule is retired, since the surface it picked is a render-resolution
      target of unknown role and converter targets are 1920x1080. None of this is in a capture here,
      because the exported action list records render target bindings and not shader resource
      bindings, which [ac7-frame-capture.md](research/ac7-frame-capture.md) states as a limitation
      twice.

      Doing the substitution: `runtime/graphics/scene_reinsert` promotes the composite and the
      interface target to output resolution, points scene colour at the reconstruction, and hands
      the tap a plan. The tap then swaps those bindings before forwarding them and scales viewports
      and scissor rectangles while a promoted target is bound, so the game tonemaps and grades the
      reconstruction with its own shaders, draws its own interface over it at output resolution, and
      its final upscale into the back buffer becomes a copy.

      Two decisions in that, both load bearing. Scene colour is substituted only after the composite
      has been bound in the frame, because the scene passes read scene colour while they are still
      writing it and an ungated substitution is a feedback loop rather than an upscale. And the
      reconstruction now runs at that same moment rather than at Present, because it is the one
      point where the scene is finished and nothing downstream has read it; evaluating at Present
      would leave the scene a frame behind the interface drawn over it. The price is a mid-frame
      evaluate, so the whole pipeline is saved and restored around it through
      `runtime/graphics/d3d11_state`.

      Cross-built with mingw-w64 and tested under Wine on DXVK, against a real device: the watch
      across seven states, the substitution checked by asking the context what actually got bound,
      the state save checked stage by stage after being deliberately disturbed, and the plan checked
      for what it gates and what it leaves alone. What no test here can reach is whether the
      substitution produces a correct picture, which needs the game's own shaders reading the game's
      own constants.

      Known and not fixed: bloom is still computed from the render resolution scene, so the glow
      composited over the reconstruction is low resolution. Post process shaders that address texels
      rather than sampling normalised will address the wrong ones, because their constants still
      describe the buffer the engine believes it has. Both are visible only in a rendered result.
- [ ] In-game overlay. The three pieces are now joined by a caller: `loader/proxy/overlay_host`
      loads the egui DLL, builds the renderer against the game's device, subclasses the window the
      swap chain presents to, and lays out and draws one frame per Present. F5 opens it. It starts
      as soon as the game has a device rather than at F8, so the panel can be opened to see that
      the backend is not running and why, and it fills its refusal line in the order the pipeline
      actually fails.

      The bridge applies start, scale, debug view, reinsertion, dump and capture intents after
      drawing on the Present thread. Quality and the master enable toggle remain unwired.

      F5 game runs reached layout but failed in back-buffer view creation. The observer could
      select a helper device before the presenting device existed. The host now derives its
      device/context from the presenting chain, checks back-buffer ownership, and releases its
      target references each frame. The worker-thread target experiment was discarded.

      Cross-built with mingw-w64. A regression test creates the wrong observer device deliberately
      and checks actual rendered pixels, target restoration and ResizeBuffers. The real egui DLL
      also passes that test under Wine with DXVK and RenderDoc, without DLSS. The old host
      reproduces the view-creation access violation in that two-device setup. AC7 validation of
      the fix remains pending. See [the investigation](research/ac7-overlay-device.md).
## Representation

The [representation plan](representation-plan.md) covers everything after the SR input chain: UI
extraction into a mod-owned premultiplied layer, the DX11 to DX12 presentation bridge, and a
vendor-neutral contract that DLSS, FSR and XeSS implement for super resolution and frame generation.
Each milestone ends deployed to the game and is judged by the log lines named in the plan. Status
classes are the tracker's: built, synthetic-tested, game-tested, device-tested.

- [x] M0. Compile again and decide the four uncommitted files. Built and synthetic-tested
      (`ec7f156`).
- [ ] M1. Classify every UI draw per screen, change nothing. The mechanism is built and
      synthetic-tested; the run that answers the question has not happened.
  - [x] The classifier as a pure function over shadowed facts, in `games/ac7/src/ui_rules.cpp`
        with a no-device test (`445fc54`).
  - [x] Vertex declaration and widget-target fingerprints, verified against UE 4.18.3 at
        `0a14a8d537a3` rather than the checkout's default 5.8.2 branch, where the same structures
        have a different shape (`0b16d2c`).
  - [x] Observer creation hooks for input layouts and both shader stages, with the texture
        descriptor reported for every texture rather than only filtered ones. Observer ABI 5
        (`1599517`).
  - [x] Re-entry counted rather than flagged, so a hook may issue context calls (`ad16848`).
  - [x] The tap shadows the pixel shader, blend and depth-stencil state, and reports them with the
        layout, vertex shader, stride and topology it already held. Frame tap ABI 7 (`cc64734`).
  - [x] Membership sets held by address with eviction before reuse, and Castagnoli shader hashes
        with the published check value pinned (`9210a29`).
  - [x] A candidate prefilter inside the tap, so the classifier sees the few draws in a frame worth
        describing rather than a callback per draw (`3039edc`).
  - [x] The loader wires creation to classification and reports the counts, `RSF_UI_CLASSIFY` on by
        default because it changes nothing.
  - [x] The run, 7 Sep 2026. It killed two rules rather than confirming them: converter targets
        cannot be found by descriptor shape (over 180 textures matched, and Slate also draws into a
        1920x3304 target no size list would hold), and the frame's own target had been wired to the
        composite rather than the back buffer. Both corrected; a converter target is now confirmed
        by watching Slate write into it. Twelve Slate declarations were named correctly by their
        element signature, and seventeen addresses were forgotten on reuse, so the registry's
        eviction fires in a real frame.
  - [x] Shader hashes printed in the trace and named from settings, so a wrong rule can be
        corrected without a rebuild (`8ac6959`).
  - [ ] A second run under the corrected rules. The counts from the first are superseded and are
        not carried forward as measurements. Screens are still reported by extent rather than by
        name: which screen the game is on is M3's question.

M2, extraction. Built, game-tested, and the test changed the design: the mechanism works and the
insertion point is wrong. Five runs on 7 September 2026.

- [x] `fullscreen_pass`: one triangle, four modes, the premultiplied composite all three frame
      generation SDKs specify. Verified by breaking it (`e1572f4`).
- [x] `ui_layer`: `R8G8B8A8_UNORM` at back-buffer extent, cleared to zero, double buffered,
      shareable. Its test measures under DXVK that Unreal's translucent blend leaves coverage at
      zero and that the patched blend accumulates, which is what the divert rests on (`061f55b`).
- [x] The divert primitive: retarget through the originals, viewport scaled by the fraction covered,
      blend alpha patched from a cache, everything restored, refusals counted with a reason
      (`a45ce13`).
- [x] Wired end to end on F3 (`fd67216`).
- [x] The runs. The interface reaches the screen at native resolution, 59584 draws diverted with no
      refusals and every written frame composited, which is the thing promotion could not do. Three
      faults were found and two of them were mine: the composite read the tail walk's back buffer
      field, which that walk releases after thirty-two frames, so the interface was diverted and
      never put back; and the colour transform belonged at the composite rather than at the layer.
- [x] Measured against the game rather than assumed: AC7's interface blend is already
      `ONE / INV_SRC_ALPHA` on colour and alpha with write mask `0xf`, so coverage was never the
      problem. The quads read the scene and its blur and glow chain as inputs, at render resolution,
      which makes them composites rather than overlays.
- [x] **The route changes.** Compositing at present skips AC7's own UI composite, its glow and its
      grade, and on the title screen the diverted widget texture is the whole picture, so the frame
      comes out flat and discoloured. Promotion of AC7's interface target is the correction: the
      game composites it, so the colour and the glow are the game's, and the promoted target is
      itself the premultiplied layer frame generation wants. The argument recorded here against
      promotion was wrong on its premise and is marked superseded rather than deleted.
- [x] `scene_promote` replacing `scene_reinsert`, 26 September. The plan promotes the composite,
      the interface layers and the chain. A layer is a target the classifier has seen a widget quad
      drawn into, refreshed every frame and dropped after 120 presents without one, so a layer the
      pool retires ages out instead of holding a plan slot; a chain target is an eight-bit
      render-resolution input of a composite draw, confirmed by watching the draw that writes it
      read the composite, with the composite watched again every 300 presents so a chain that
      appears after the tail walk still rebuilds the plan. Every set change rebuilds the plan on the
      present thread. Deleted with it: the shape hunt and its collection, the format rule for the
      interface target, and the tail relooks; the tap has four watch slots and no hunt. Built and
      synthetic-tested with MSVC on Windows. Not game-tested. The run has to answer two things the
      code cannot: whether the intermediate between the tonemap and the UI composite is filled by a
      draw (then it is promoted) or by a copy (then the log says it was never drawn into while
      watched, and a `CopyResource` hook is the next piece), and whether dropping the scene depth at
      the quads costs anything visible on the briefing.
- [x] The first Windows game run, 26 September, which took the day: the game crashed at launch
      (Steam's overlay and our vtable hook on Present forwarding to each other), then presented
      nothing (the genuine entry is patched too, by RivaTuner on this machine), then found no
      scene pass (the swap chain size was read once, at its DPI-scaled startup size), then
      refused every F6 (typeless surfaces need typed views). Each is measured and fixed in
      [the coexistence note](research/windows-present-hook-coexistence.md). At the end of it:
      Streamline 2.14.1 loads, DLSS evaluates every frame at 800x450 to 1600x900 with zero
      refusals, the panel's mouse works the way SpecialK's does, and F6 promotion ran for over
      13,000 frames with the composite, the chain target and two interface layers promoted, two
      depth mismatches per frame where the quads drop the scene's depth, and the layers aging out
      of the plan when the game moved to a screen without widget quads. Whether the picture is
      right is the next line.
- [ ] The judgement: the front end sharp, and the menu shimmer gone without closing the jitter
      gate. Not yet written down; the run above ended with the picture in front of the user.
- [ ] egui drawn into the layer, so it rides generated frames later.

M4, the presentation bridge, has its riskiest piece answered as far as this machine can answer it.

- [x] `shared_surface`: D3D11 textures and a fence created shareable and opened on D3D12, with the
      export, open and signal round trip covered (`510281f`).
- [x] Measured, and the answer is yes. Under a prefix built by `eng/wine-test-prefix.sh` from an
      installed Proton's DXVK and vkd3d-proton, every stage passes: a D3D11 texture opens on a
      D3D12 device on the same adapter, and a fence signalled on one side is seen on the other.
      Risk 7 is retired. Under the default prefix WineD3D returns `E_NOTIMPL` and the fixture skips,
      which is why the script exists: a fixture that had only run there would have reported the
      bridge impossible and been wrong about the only environment that matters.
- [ ] `texture_dump` fails under DXVK with the subprocess killed, reproducibly, while passing under
      WineD3D. Unexamined.
- [ ] The swap chain facade, the ring, and the intercept.

M5, the vendor-neutral contract, is built and tested without hardware.

- [x] `backend.h`: reconstruction and generation providers, capabilities, resources with their
      D3D12 states and lifetimes, and the swap chain ownership every vendor takes differently.
- [x] `negotiate`: a pure function over what each vendor said, so the combinations nobody has the
      hardware to try are decided and tested here. The Streamline single-session conflict, the
      no-bridge refusal, the multiplier cap, and the two different causes that produce the same
      D3D12 route (`fc54132`).
- [ ] Wrapping today's Streamline path behind it, the orchestrator session, and SDK ABI 2.

M6 and M8 have their vendor structure, with no implementation behind it.

- [x] `rsf_backend_fsr` and `rsf_backend_xess`: providers that exist whether or not the SDK headers
      do, answering `NOT_COMPILED` rather than failing to link, with capabilities filled from what
      each SDK is rather than from what this machine has. Both branches compile (`fc54132`,
      `7fa92f0`).
- [ ] Everything that does the work.

M3 has its foundation only.

- [x] `game_frame.h`: the frame record, the screen classes, the eligibility rules, and
      `rsf_camera_frame` moved out of the orchestrator into the SDK where a plugin can see it
      (`b23d18d`).
- [ ] `screen_policy`: video, graphics context and loading facts from the game. Needs Ghidra work on
      the `UManaComponent` anchors, and is better done with the extraction result in hand, because
      it decides which screens extraction is armed on.
- [ ] `graphics_settings`: the per-context render scale through `FGraphicsSettingsManager`.
- [ ] M2. Divert into the UI layer and composite at present with FG off: the layer, `fullscreen_pass`,
      `composite`, the divert primitive with the alpha-op blend patch, `scene_promote` replacing
      `scene_reinsert` (interface targets deleted, chain targets added), egui in the layer.
- [ ] M3. Frame identity, eligibility, and the game's own per-context screen percentage table.
- [ ] M4. Presentation bridge with a pass-through present and no FG.
- [ ] M5. Migration: `backend.h`, game SDK ABI 2, orchestrator session, `dlss_bridge.c` deleted,
      overlay ABI 3; `rendering_ready` flips only with this milestone's evidence.
- [ ] M6. Frame generation, FidelityFX first (any D3D12 GPU, no XeLL, no Streamline device conflict).
- [ ] M7. Frame generation, DLSS-G; DLSS-SR moves to the D3D12 proxy while DLSS-G owns FG.
- [ ] M8. Frame generation, XeFG with XeLL, non-Intel mode on the development machine.
- [ ] M9. Super resolution per vendor behind `rsf_sr_provider` (FSR via the bridge, XeSS-SR D3D11
      on Arc).
- [ ] M10. Claw device run: XeSS-SR D3D11, XeFG 3x/4x, XeLL. The only device-tested milestone.

Retained from the earlier list and folded into those milestones: the loader and orchestrator
handshake (M5), plugin lifecycle (M5), the synthetic bridge (M4), XeLL and MFG on the Claw (M8,
M10), frame identity and scene/HUD boundaries (M1 to M3), reinsertion into post-processing (M2),
combined SR and MFG (M6 to M10). Still outside the plan:

- [ ] Standalone profiles, native egui rendering/input, and handheld controls.
- [ ] WSGM launch/profile integration, limiter coordination, and rendered-frame AutoTDP inputs.
- [ ] Matched-condition visual, frame-time, latency, and power measurements.

## AC7 motion and velocity improvements

14 October capture correction: early translucency samples consumed the readback budget before
flight particles, including unused IA slots. F9 now reserves per-producer budgets, reads declared
streams and actual previous-transform/bone buffer views, and preserves predication. The prior
flight capture proves VS 127 mesh-particle bindings but lacks their geometry samples. The newest
capture is briefing; affected weapon/vehicle/kill-cam/particle data still needs collection.
[Evidence](research/ac7-velocity-20261013.md#coverage-capture-correction-14-october).

14 October ocean-cutscene follow-up: identical modified PS/HS/DS objects are now shared across
material variants, alongside VS/GS/SO. A tessellated material-alias check causes zero additional
device shader creations. Indirect history dispatch uses the validated GPU record count, and fixed
producers avoid zero-tail copies. Slow-frame CPU summaries identify shader/resource preparation.
The user confirms the port/ocean cutscene regression is fully fixed in deployed `0af8c9e`;
cinematic FG is also game-confirmed. Affected-object visual motion coverage remains open.
[Evidence](research/ac7-velocity-20261013.md#ocean-cutscene-dips-and-remaining-work-cost-14-october).

14 October performance correction: cinematic FG is user-confirmed. The remaining FPS regression
exposed whole-allocation clears/copies for every small material draw after a large effect. Shared
SO storage now grows geometrically, uses one bounded zero copy, and exposes its output directly
to history compute through a read-only SRV. The RTX 4070 Laptop reproduction falls from roughly
14 ms to 3.4 ms for 100 tiny draws after large storage growth. The later ocean-cutscene follow-up
above records game-confirmed performance recovery.
[Evidence](research/ac7-velocity-20261013.md#per-draw-buffer-bandwidth-regression-14-october).

14 October follow-up: the new captures identify a rendered cinematic rejected as unknown despite
a player plane and known camera actor. That combination now selects replay FG, preserving pause,
transition and cut resets. Repeated per-material GS HLSL compilation is replaced by one fixed
algorithm with DXBC interface linking and shared shader objects. Release/WARP checks pass;
the later follow-up above records game-confirmed FPS recovery and cinematic FG activation.
[Evidence](research/ac7-velocity-20261013.md#rendered-cutscenes-and-compilation-stalls-14-october-follow-up).

The same-run corpus now validates the complete native PN VS/HS/DS chain and 29 ordinary
material shaders on WARP. Capture 6 has changing native component histories and written motion;
stationary hangar weapons and the incomplete flight capture leave affected-scene visual coverage
open. These captures precede the deployed performance/cinematic correction.

14 October: native beam, ribbon and animated-trail fills now supply stable counter/segment
identity. The material pass interpolates prior rendered curves across subdivisions and retains
surviving nodes when new heads appear. Native detour and WARP checks cover these paths, strips,
ordering, factory reuse, clipping and cuts. Affected weapon/vehicle/kill-cam scenes still lack
gameplay acceptance. [Evidence](research/ac7-velocity-20261013.md#beam-ribbon-and-animated-trail-producers-14-october).

Current increment: triangle tessellation carries stable control-point keys, captures bounded
native patch-corner history and preserves adaptive tessellation/material depth in the normal draw.
WARP checks changing density, independent domain motion, cuts, 12-point indirect submission and
state restoration. Retained native PN HS/DS variants create with matching synthetic VS inputs.
AlphaComposite also retains the reference pass's opacity behavior. The affected-scene coverage
audit and game checks remain open. [Evidence](research/ac7-velocity-20261013.md#tessellated-material-velocity).

Current increment: single-stream triangle native GS draws retain their own final generated
positions and material varyings. WARP checks combined VS/GS deformation, opacity clipping,
camera cuts and native-state restoration. Tessellation, multiple GS streams and GS instancing
still need their producer routes. Affected-game checks remain deferred; the goal is active.
[Current evidence](research/ac7-velocity-20261013.md#native-geometry-shader-producer).
The following dated paragraphs retain the status at each preceding increment.

Instanced mesh-particle correspondence now uses the native sorted previous-transform buffer and
full pose/vertex keys. Native queued-binding/lifetime and reordered-instance GPU fixtures pass.
GS/tessellation remain unfinished; gameplay checks remain deferred. See the current research
increment for exact native guards and ownership.

Indirect geometry now preserves GPU argument identity/offset, validates its copied native mesh
capacity on the GPU and replays without CPU readback. The full producer and actual tap fixtures
cover indexed/nonindexed submissions and bounds. Sorted sprites also pass independent-motion
validation with reversed ordering and recycled vertex-factory identity. Native GS/tessellation
and mesh-particle instance correspondence remain open; the goal is still active.

Later 13 October increment: added and connected a dedicated material velocity/depth producer
with opacity clipping, additive-opacity recovery, GPU generated-geometry history and a queued
commit into native inputs before post-processing. ABI 15 carries per-mesh owner/material settings.
The full GPU producer fixture, retained shader validation and Release gate pass (45 native tests,
seven opt-in skips). Native GS/tessellation/indirect routes and particle correspondence coverage
remain unfinished; affected-game testing is deferred because the human cannot test now. The
overall motion goal remains active. [Current source and proof limits](research/ac7-velocity-20261013.md).

13 October increment: native opaque velocity no longer depends on TAA/motion-blur selection,
small primitives are retained, and unchanged-root movable meshes reach native velocity only after
successful history lookup. F9 summaries now distinguish unwritten values from valid zero and
capture bounded native translucency vertex streams/layouts. Release verification passed (44
native tests passed, seven opt-in tests skipped). The full 4.27-style translucent velocity/depth
pass is still unimplemented; the experimental blend-gate patch does not replace it. Weapons,
vehicles, animated-mesh/kill-cam and particle coverage still require the affected native producer
data and game checks. [Current increment and exact remaining work](research/ac7-velocity-20261013.md).

The engine's camera transform is already available through `ClipToPrevClip`; camera motion is
not being estimated from images. The current DLSS path decodes the sparse object-velocity buffer
and asks Streamline to resolve unwritten pixels using depth and that transform. The tasks below
include completed offline investigation and proposed changes. The
[30 September research](research/ac7-motion-vectors.md) found a conversion discrepancy against
native shaders and a valid-zero limitation in the deployed Streamline kernel. Corrections and
producer extensions have not been game-tested.

- [x] Disassemble the 207 recorded native VS/PS blobs and the deployed Streamline motion kernel.
      The ordinary velocity producer stores total current-minus-previous NDC; the inspected
      temporal consumer replaces depth-derived camera motion at written pixels and exports colour
      only. The custom projection branch has different jitter selection and remains unidentified.
- [x] Compare RenoDX and Luma shader replacement at pinned revisions. Author an independent
      packed-output plus float-motion sidecar probe. It compiles as `ps_5_0` and creates on Windows
      D3D11 WARP; draw readback, binding integration and game validation remain undone.
- [x] Check captured `ClipToPrevClip` against both frames' unjittered translated projections.
      Maximum absolute matrix error is `1.44e-6` across 472 repeated bindings over two briefing
      frames. This is capture analysis, not a new stationary-camera or flight test.
- [x] Locate AC7's small-object velocity selection gate in the decrypted image. The known dynamic
      loop calls `ShouldRenderVelocity` at RVA `0x10fdc00`; size rejection is `76 38` at
      `0x10fdd0c`, followed by history/transform checks in `HasVelocity` at `0x1183820`.
      Native/source matched with Capstone and named/decompiled in the local Ghidra project.
      Eligibility bypass and affected-scene validation remain pending.
      [Evidence](research/evidence/ac7-velocity-coverage-20260930.json).
- [x] Build opt-in F9 motion capture with guarded engine observations, shader/CB records and
      lossless backend input samples. Windows WARP capture and readback tests pass; the Release
      verification gate passes (30 tests passed, four vendor-environment tests skipped).
      AC7 capture validation remains pending. See [capture procedure](../loader/README.md).
- [x] Analyze six 30 September F9 captures and isolate hangar colour posterization with a
      frozen-input RTX 4070 Laptop experiment. Performance preset K is smooth at unchanged
      800x452 input and 1600x900 output. Forced K was withdrawn because driver overrides can
      defeat it; actual NGX flags preserve HDR and the preset-independent fix remains open.
      R11/FP16/FP32 GPU input comparisons and FP32 output also preserve the same bands;
      widening formats does not solve this captured failure.
      A diagnostic perceptual-SDR input is smooth, but the actual game route is unimplemented.
      Bounded tonemap/scene-effect/HUD/composite snapshots now support that investigation.
      Recognize the captured fullscreen flight HUD producer
      and preserve its promoted output; add final-screen raw/preview captures. Windows Release
      verification passes. Corrected AC7 run and pause/resume validation remain pending.
      [Research](research/ac7-ui-hdr-20260930.md).
- [x] Measure current coverage and conventions from the 3 and 4 October F9 captures. In flight
      only the player aircraft and its attached weapons write velocity; the submitted dense
      field reproduces written vectors and depth reprojection to 0.000 px median error. The
      size gate drops only sub-3-pixel primitives. Translucent effects, the kill-cam and clouds
      (device depth 0) get camera motion only. Map all 4.18 velocity gates in the binary; this
      calls the 7 September translucent-velocity result into question pending a retest.
      [Research](research/ac7-motion-depth-20261004.md).
- [x] Store decoded motion in R32G32_FLOAT. Storing the resolved motion that way too broke SR in
      the 14:27 game run: R32G32_FLOAT is not D3D11 shareable, so the DLSS-G and FSR/XeSS
      transfers refused every frame. The resolved motion is R16G16_FLOAT again; the
      shared-host hardware test now covers this path. Redeployed; not yet game-tested.
- [x] Fill translucency hints from a scene-colour snapshot at the first translucency pass and
      the separate layer: DLSS transparency/bias/colour-before-transparency/layer tags, FSR
      reactive and transparency-and-composition, XeSS responsive mask. DLSS SR with current
      presets is not expected to read most of them. Built, WARP-tested, deployed; not
      game-tested.
- [x] Reproject unwritten pixels at TrueSky cloud depth, from a guarded relay at the
      composite_tile draw (DLL RVA `0xabc6b`). Built, resolve WARP-tested, deployed; the
      relay and conversion are not game-tested.
- [x] Port 4.27's material texture mip bias, `max(log2(render/output) - 0.3, -2)`, as
      biased sampler clones inside the primary view's base pass (`0xebf050`) and translucency
      scopes. `4.27-plus` (tip `41b2c549`, 15 September 2026) was checked as the newest UE4
      source; it changes no velocity or depth code. Built, hardware D3D11 tested, deployed; not
      game-tested.
- [x] Remove DLSS-only hangar shimmer and Performance/Ultra Performance banding without forcing a
      preset: the post-DLSS colour correction is off by default (`RSF_DLSS_COLOUR_CORRECTION`),
      and DLSS receives an invertible display-range encoding with HDR input off, decoded back to
      linear with highlight recovery (`RSF_DLSS_TONEMAP`). Probe-measured on the real runtime,
      WARP- and shared-host-tested, and game-confirmed by the user on 4 October.
      [Research](research/ac7-motion-depth-20261004.md).
- [ ] Run the hint and cloud build in flight near clouds and with smoke, on DLSS, FSR and XeSS.
- [ ] Retest the translucent-velocity gate with the two earlier rejections (`0x1184e4c`,
      `0x10fdc7c`) accounted for, or withdraw the 7 September result.
- [ ] Trial `ShouldRenderVelocities` and the `HasVelocity` always-velocity jump (`0x118384c`)
      separately for animated movable meshes and the kill-cam.
- [x] Fix the F9 recorder's `fraction_unwritten` summaries: select packed/decoded views for the
      corresponding textures, preserving valid decoded zero. Built and synthetic-tested on 13 October.
- [ ] Prioritize the reported carrier-launch and refuelling weapon/attached-missile omissions,
      then possibly missing moving ground vehicles. Match each visible colour draw to its
      velocity draw or exact rejection reason, separating per-primitive bounds from material/LOD
      sections. Confirm previous world transforms, attachments, bone motion and valid zero values.
      Only then apply a guarded size-gate bypass or class-specific producer change and measure cost.
- [ ] Audit TrueSky cloud motion with a stationary camera and with rotation/translation. The
      installed renderer has wind and cloud-reprojection string leads, not proven output vectors.
      Trace its own time/depth/history inputs; static density still needs camera motion and finite
      cloud depth. Independent drift/evolution and multilayer blending need separate validation.
- [ ] Capture flight particle producers. Stock CPU sprite streams expose `OldPosition` from
      `Particle.OldLocation`, although their velocity helper returns current position. Confirm
      AC7's input layout, history interval and previous billboard/deformation state before adding
      a VS/PS motion output. Beam/trail fill paths sometimes duplicate current into old position.

- [ ] Complete the engine path inventory in flight. The captured temporal and reflection consumers
      are inspected; compute/motion-blur and flight-specific shaders remain. Locate their per-pixel
      camera-motion calculation and establish whether a complete, reusable motion texture exists
      or the result is only an intermediate shader value. Record the pass, inputs, encoding,
      extent, timing, and lifetime. Follow the later correction in
      [ac7-frame-capture.md](research/ac7-frame-capture.md): resource 63083 was identified as a mask,
      not velocity flattening; capture resource IDs are not runtime identifiers.
- [ ] Choose the motion source from that evidence. Prefer reusing a suitable engine-produced
      full-motion texture. If the engine only computes motion inside a consuming shader, evaluate
      exporting that intermediate through a shader change versus retaining Streamline's existing
      resolve. Engine camera transforms still need depth to become per-pixel displacement.
      Do not assume an engine-produced result is more accurate without matching conventions.
- [ ] Establish the exact meaning of written object vectors. Determine whether they already
      use the verified ordinary total-motion path or AC7's unidentified custom projection path.
      Preserve valid zero motion separately from unwritten pixels, and never add camera movement
      twice. Verify the sentinel handling against the Streamline version actually loaded.
- [ ] Reconcile the Rust and live C/C++ motion conversions. `MotionToPixels::unreal` in
      `runtime/backends/rsf-upscaler/src/frame.rs` uses half the viewport extent and a vertical
      sign flip, while `loader/proxy/src/dlss_bridge.c` and
      `runtime/orchestrator/src/dlss_pipeline.cpp` currently default output/backend scales to
      one. Trace clip displacement, normalized UV displacement, pixel units, and temporal
      direction end to end. Measure aircraft or missile displacement on both axes before applying
      the proposed ordinary-path Streamline scale `(0.5, -0.5)`. Rust's helper expresses the
      opposite temporal direction from the FSR/XeSS resolve; distinguish direction from units.
      Matching a numeric range alone does not establish matching units.
- [ ] Verify jitter throughout reprojection. In `games/ac7/src/view_uniforms.cpp`, establish
      whether the captured unjittered `ClipToPrevClip` result holds in stationary and flight views,
      and identify the custom producer's projection/jitter selection.
      Removing jitter from `ViewToClip` alone does not prove this. With a stationary camera and
      changing jitter, unjittered camera motion must remain zero. If necessary, remove both frames'
      jitter transforms and recompute the inverse. Explicitly match the backend's vector-jitter
      flag to the measured object-vector convention.
- [ ] Add motion diagnostics: raw/written-pixel coverage, decoded object vectors, camera-only
      vectors, resolved vectors, and previous-frame reprojection error. Expose the actual submitted
      scales, jitter, frame identity, and reset reason. Separate disocclusions and changing shading
      from vector errors; do not judge correctness from a vector colour plot alone.
- [ ] Propagate camera cuts and interrupted history through the AC7 camera frame. Prefer the
      engine's cut/history state, with evaluated-frame continuity tracking for missed frames.
      Cover mission loads, camera-mode switches, and resource/resolution changes. The pipeline
      currently adds a reset on rebuild; ordinary fast flight must not be treated as a cut.
- [ ] Audit geometry coverage using the diagnostics: aircraft, missiles, animated control surfaces,
      and other independently moving or deforming geometry. Where writes are missing, first look
      for an existing engine velocity path that can be enabled. Otherwise assess a shader change
      or additional velocity pass using previous object transforms or deformation state. Opaque
      depth alone cannot recover independent object movement.
- [ ] Investigate clouds, smoke, and contrails separately. Locate their own depth, history, and
      motion inputs, and assess whether they can supply useful motion for reconstruction.
      Validate sky reprojection at clear/reversed-Z depth separately from ordinary geometry.
      Do not assign opaque background motion to transparent layers as though it were exact.
- [ ] Confirm the screen-droplet/refraction pass and place it after reconstruction where feasible.
      Coordinate this with output reinsertion so the scene is reconstructed before the game's
      grade and HUD composite. Recheck all relevant extents at reduced render scale; full-resolution
      captures alone did not establish an output-resolution HUD path.
- [ ] Compare the existing explicit-sentinel dense resolve with Streamline's sparse resolve for
      DLSS, since the deployed kernel treats valid zero as unwritten. Keep this independent of
      producer/scale changes and measure GPU cost. If submitting a
      complete field, set backend metadata accordingly to prevent another camera-motion resolve.
      Validate depth-aware edge dilation and its metadata without applying it twice.
- [ ] Validate in controlled captures and live motion: stationary camera with jitter, horizontal
      and vertical pans, forward flight near terrain, roll/FOV changes, tracked aircraft, crossing
      missiles, clouds/contrails, and camera cuts at native and reduced render resolution.
      Check colour/depth/motion/camera frame alignment and GPU cost. Record synthetic-tested,
      capture-validated, and game-tested results separately before marking tasks complete.

## Reusable SR orchestrator

- [x] Game-independent SR session and D3D11/D3D12 transfer API. Transactional backend selection,
      requested/effective status, SDK version reporting and history reset/refusal handling.
- [x] FSR2, FSR3 and FSR4 family selection with version overrides and separate SDK runtime paths;
      XeSS D3D12 adapter. FSR2/FSR3/XeSS device-tested on Intel UHD and RTX 4070 Laptop. FSR4
      refusal tested on those adapters; FSR4 evaluation still requires compatible hardware.
- [x] Generic sparse-motion/depth preparation, WARP numeric fixture and controller rollback tests.
- [x] Compatibility path and Insert overlay selector for DLSS/FSR2/FSR3/FSR4/XeSS. Built;
      synthetic hardware sequence DLSS â†’ FSR2 â†’ FSR3 â†’ XeSS â†’ DLSS passed. Engine identity and
      world units are supplied by the integration. Existing AC7 startup still begins with DLSS.
- [ ] Game-test AC7 switching, motion, exposure, transitions and overhead. New FSR/XeSS game
      support is not claimed. Public plugin lifecycle migration, native XeSS D3D11, FG and latency
      remain separate work. [Research, revisions and limits](research/orchestrator-sr-switching.md).

## Engine integration improvements

The observed AC7 FSR/XeSS switch refusals were missing vendor DLLs (Windows error 126).
Signed SR runtimes and notices are now deployed with hash verification. The hardware fixture
passes DLSS -> FSR2 -> FSR3 -> XeSS -> DLSS using the installed paths. This is a deployment fix
and synthetic device evidence; fresh AC7 switching/visual validation remains pending.
[Evidence](research/sr-runtime-deployment.md).

The reusable FG contract, CPU frame ledger, ownership/fallback controller, FSR3/4 and XeSS FG
providers, and early D3D12 Streamline/DLSS-G host are implemented. Synthetic controller tests pass.
DLSS-G, FSR3 and XeFG passed disabled-presentation lifecycle probes on the RTX 4070 Laptop GPU;
FSR4 was refused. Enabled FG, game export/facade integration, independent PCL/pacing transitions,
shared D3D12 DLSS SR and latency/visual validation remain open. [Evidence and limits](research/orchestrator-fg-implementation.md).

Independent PCL and Reflex services now expose cold profiles, shared token ownership and bounded
status without requiring FG. PCL-only API/render/Present checks pass on NVIDIA and Intel with zero
sleep calls; Reflex Off/On/Boost checks pass on NVIDIA with one sleep per source frame. This is SDK
API evidence, not ETW validation or physical latency measurement. [Evidence](research/orchestrator-latency-services.md).

The Ghidra MCP extension and stdio bridge are installed and connected to the retained AC7 dump.
Six CPU boundary names pass byte-span checks and are recorded in a tracked replay manifest.
Auto-analysis finished and the database save succeeded. This is static research, not a
game-tested CPU/render handoff. [Recovery evidence](research/ac7-fg-cpu-boundaries.md#ghidra-recovery-and-repeatable-names).

The reusable render-link registry carries source IDs through engine-owned queued objects without
using the latest CPU frame. Delayed rendering, multiple submissions, capacity refusal and stale
pointer/ticket checks pass synthetic tests. Seven AC7 renderer/family/task functions are named,
byte-checked and saved. Native Release verification passed (31 tests, five opt-in fixtures skipped).
Game hooks, main-view selection, RHI handoff and final FG export remain unwired.
[Ownership trace and limits](research/ac7-render-frame-handoff.md).

The current [frame-generation orchestrator plan](frame-generation-plan.md) defines FG0-FG9 for
DLSS-FG/MFG, XeSS-FG/MFG, FSR3/FSR4 FG, Reflex/XeLL and independent PCL telemetry. Planning/source
inspection is recorded on 30 September 2026; subsequent implementation evidence is linked above.
Reuse the completed root UI work and finish its paired FG export. Shader replacement and
motion-quality improvements belong to the separate motion workstream; FG owns the input contract,
presentation integration and compatibility/acceptance checks.

Use the existing plugin/runtime ownership split. These tasks extend the first working target and
the motion investigations above; none is a claim of completed implementation.

- [ ] Capture view data on the CPU when UE4 constructs or uploads it, with the matching render-frame
      and view identity. Measure the current constant-buffer staging allocation, copy, and immediate
      Map cost before replacing it. Reusing staging allocations is an interim improvement only;
      do not introduce stale camera data to avoid a synchronous readback.
- [ ] Promote discovered AC7 passes into explicit, verified engine-function or shader identities,
      with resource checks as confirmation. Keep broad format/binding discovery as a diagnostic
      mode. Select resources for a known pass, view, and frame rather than the first plausible set.
- [ ] Establish a frame record with render-frame ID, view ID, valid rectangles, camera/exposure
      state, and resource-use boundaries. Distinguish rendered frames, Present calls, and generated
      frames. Define immediate-consumption versus copy-before-reuse obligations; retaining a
      texture's allocation does not preserve its contents.
- [ ] Complete reinsertion at the chosen scene reconstruction boundary before expanding into FG.
      Return an output-resolution result to downstream passes and preserve grading, effects, and
      an output-resolution HUD. Bypass only the original filtering/upscaling that SR replaces;
      investigate cloud-specific temporal accumulation before disabling any temporal pass.
      The substitution and the output-resolution HUD path are built behind F6 and described in the
      first working target above; what remains here is running it against the game, the bloom and
      texel-addressing consequences recorded there, and the cloud temporal question.
- [ ] Verify the engine's exposure and pre-exposure convention, frame alignment, and backend
      conversion. Identifying a 1x1 texture alone is insufficient. Compare against auto-exposure
      during bright/dark transitions and record the source and effective values in diagnostics.
- [ ] Verify texture mip selection at reduced render resolution. Apply any required bias through
      appropriate engine/material paths, restore it when SR is disabled, and compare fine-detail
      recovery against shimmer. Avoid indiscriminate global bias or sharpening as a substitute.
- [ ] Sequence the integration work around CPU-side view capture, explicit pass/frame identification,
      correct reinsertion, and exposure/mip tuning before additional backends and FG. Build the
      egui inspection and capture tools alongside these steps so they can validate each change.

## egui development and validation workflow

Use the prepared egui overlay as the main in-game development and validation interface as well as
the eventual player settings UI. Prioritize effective settings, buffer inspection, frame-time plots,
and capture of the next N frames; prove the capture format before building offline sequence replay.

- [ ] Connect the existing egui DLL, D3D11 renderer, and input hook through the runtime lifecycle.
      Support opening/closing the interface, correct input ownership, graphics-state restoration,
      and clean shutdown. Reuse the existing overlay implementation rather than creating another UI.
- [ ] Implement a bounded command/status path: egui requests changes, the render integration applies
      them at a defined frame boundary, and the UI reports effective settings or a refusal reason.
      Keep vendor calls and per-frame GPU work in the runtime, and game-specific preparation in AC7.
- [ ] Add a Compare view for native game output, native-resolution DLAA, reduced-resolution DLSS,
      and ordinary scaling at the same reduced resolution. Support split-screen or a movable divider
      where inputs are matched. Label a genuine native reference as a separate render or matched
      run; it cannot be recovered from the same low-resolution input.
- [ ] Make comparisons temporally valid. Handle history resets when changing backend or resolution,
      label warm-up periods, and compare settled results. Use independent histories if running
      multiple temporal configurations together. Record the extra cost of comparison mode.
- [ ] Add an Inspect view for scene colour, depth, raw/decoded/resolved motion, camera-only motion,
      written-pixel coverage, exposure, and reprojection error. Include pixel inspection, units,
      display scale, and resource/frame identity. Distinguish disocclusions and shading changes
      from motion errors. Use this view to deliver the motion diagnostics listed above.
- [ ] Separate freezing the displayed diagnostic image from pausing processing. Continue maintaining
      valid live history when only the display is frozen, and define reset/resume behaviour when
      processing stops. Never feed repeated or mismatched frames to a backend accidentally.
- [ ] Add a Capture view with capture-next-N-frames, progress, cancellation, and completion/error
      status. Save matching colour, depth, motion, available exposure, camera matrices, jitter,
      resets, frame IDs/timing, extents, formats, and relevant settings in a versioned sequence
      format. Include game fingerprint, RSF commit/build, backend/SDK version, adapter, driver,
      graphics API, and capture stage so results can be reproduced and compared.
- [ ] Use bounded GPU readback queues with completion tracking and background file writing.
      Configure memory/disk limits and report dropped or incomplete frames and their identities.
      Mark gaps that invalidate temporal replay. Avoid silently stalling gameplay and measure the
      capture overhead; GPU context work stays on its owning thread.
- [ ] Keep diagnostic UI out of captured scene inputs and backend history. Offer a separate
      annotated screenshot/export when overlay information is wanted. Preserve unmodified numeric
      buffer data alongside any tonemapped or colourized previews.
- [ ] Add a Performance view with frame-time history, CPU hook/readback time, GPU decode/SR time,
      resource rebuilds, and diagnostic overhead. Retrieve GPU timings without forcing immediate
      synchronization and identify unavailable/invalid measurements. Distinguish rendered and
      presented/generated rates; do not label either as input latency.
- [ ] Add a Status view showing selected pass/view, actual render/output sizes and valid rectangles,
      requested/effective configuration, frame continuity, history reset reasons, missing inputs,
      backend support, and capture queue/dropped-frame status. Bound logging and graph history.
- [ ] Once live capture is validated, implement offline replay of complete sequences through a
      supported backend with their original order, constants, exposure, and reset state. Reject
      incompatible/incomplete inputs or mark comparison limits explicitly. Use replay to compare
      motion conversions and backend settings without requiring another full game session.
- [ ] Define repeatable visual/performance runs using the motion scenarios above plus exposure
      transitions and fine-texture scenes. Compare native, DLAA, reduced-resolution DLSS, and
      ordinary scaling under matched conditions. Export settings, warm-up rules, capture identity,
      image comparisons, and timing summaries; separate backend cost from tooling overhead.
- [ ] Validate the initial egui workflow in game: change settings and confirm effective values,
      inspect buffers, capture a bounded sequence, exercise cancellation/queue overflow, and check
      that opening, freezing, or closing diagnostics does not corrupt history or game input.
      Record built, synthetic-tested, capture-validated, and game-tested status separately.

- [x] Repair the 30 September mixed proxy/overlay deployment: ABI 4 proxy paired with ABI 3
      panel prevented Insert startup. The rebuilt real Rust panel passes Insert, draw and resize
      checks; matched pair deployed with backups. Add explicit pair deployment/preflight tooling.
      Corrected AC7 run pending. [Research](research/ac7-consumer-session.md).

## Repository review follow-up

The [30 September renderer ownership audit](research/ac7-renderer-roots-20260930.md) changes
the SR integration work: late resource identities can alias an earlier GBuffer. Successful
backend evaluation is not evidence that the supplied scene includes intact lighting/reflections.

- [x] Capture the pooled GBuffer/tonemap alias and premature lighting/reflection SRV replacement.
      Gate composite/chain promotion by phase, refuse MRT promotion/gates and call shared gates
      once per binding. Windows MSVC Release built and synthetic regression passed: 31 tests
      passed, five vendor skips, Rust formatting/lint passed. User confirms lighting/reflections
      restored in the 19:09 game build. Jitter/shimmer returned and remain unresolved.
- [x] Map and name native post-processing, per-view renderer and widget converter roots in the
      decrypted Ghidra project, plus main temporal construction/Process/ComputeOutputDesc,
      composition descriptor gathering/execution, RHI commands and pre-visibility/jitter setup.
      Post-processing/widget F9 scopes game-captured in 14 sessions (42 paired samples). All 42
      backend jitter pairs match native view samples; sampled cuts are zero. CPU scopes end before
      the post-process draws, so TLS alone is insufficient. Expanded graph/RHI capture built and
      deployed with expected-byte guards; runtime validation pending.
- [x] Validate sampled graph-to-RHI command associations: nine completed process-215904 sessions,
      27 paired samples and 1119 matched draw/dispatch records with no native/command drops. Name
      Tonemap/FXAA/material/Nimbus HUD/composite descriptor/processing methods from runtime vtables.
      This sampled result does not establish all queue/Present identities.
- [x] Diagnose the pause-menu F9 crash using an identical-code linker map and minidump: route copy
      used an unretained borrowed translucency texture. Add owned retention/current-frame capture
      checks and pre-execution native checkpoints; emit a Release linker map. Windows MSVC Release
      verified, matching proxy/overlay deployed; corrected crash scene pending game validation.
      [Evidence](research/ac7-graph-capture-crash-20260930.md).
- [ ] Make the identified main temporal node and engine graph own SR inputs/output
      descriptors and rectangles. Flight tonemap/scene-effect still write 928x524 before 1600x900
      final composition; this is a measured extent problem, not the proven whole shimmer cause.
- [ ] Move UI resolution to widget producers and dependent downsample/glow allocations; preserve
      logical layout, refresh timing and real depth occlusion. Retire settling/discovery heuristics
      only after equivalent transition captures pass.
- [ ] Reassess remaining hangar banding using an intact input scene. Keep driver preset selection
      unrestricted. Post-tonemap SR is a possible experiment, not the established correction.

Open defects from the [7 September review](review.md). These are not fixed by the documentation rewrite.

- [ ] Move F8 startup and F7 state changes to a render-thread command boundary; eliminate concurrent immediate-context use.
- [ ] Reject non-finite camera, projection, reprojection, and jitter data in the reader/assembly path; add poisoned-input regressions.
- [ ] Roll back every observer hook after partial installation failure, and quiesce callbacks before teardown.
- [ ] Fix RGBA16F colour selection in the frame tap and test colour/history selection together.
- [x] Parse settings so explicit zero values work. `read_number` uses `strtoul` with base 0, so hexadecimal
      overrides and a present-and-zero value are honoured (7 September). Type/range validation and a
      `config_parse` test land with the representation plan's M5.
- [ ] Gate the debug blit on a successful evaluation of the current frame and reset history after gaps.
- [ ] Preserve input press/release events between frames so egui does not lose short clicks.
- [ ] Add Rust test execution and a pinned SDK-header compile job to CI; keep hardware evaluation a separate gate.
      Planned as the two-leg vendor matrix in the [representation plan](representation-plan.md).
- [ ] Make capture-timeline reads reflect inherited bindings at draws, or explicitly report unsupported tracking.
- [ ] Derive discovery extents from verified render data/backend planning so inputs below 50% can be selected.

The [validation plan](research/validation-plan.md) defines acceptance. Keep build, synthetic, game, and target-device results separate.

Native widget producer continuation: QueueRender/PrepareTargets/DrawScaledWindow now own physical
raster density while preserving logical layout; owned glow siblings resize and replacement painting
bypasses refresh skipping. Native settings stop writing screen-percentage cvars. FSR/XeSS receive
engine exposure with transactional context policy changes. MSVC Release built only; not deployed,
no new tests. Borrowed UI targets, native UI isolation and FG identity remain open.
[Evidence](research/ac7-native-renderer-refactor-20261001.md).

Native UI composition: ABI3 appends leased ui_input from the verified view-owned HUD target.
Runtime now owns demand-driven paired scene/UI/composed GPU snapshots with native scope identity;
F9 writes these layers. MSVC Release built, not deployed/game-tested. Raw UI alpha/effects and
simulation/Present identity remain unproven; these inputs do not authorize FG yet.

Native renderer identity continuation: ABI4 copies input-loop source frame and per-renderer
submission ID through native SR/HUD packets. Bounded constructor bindings retire before native
delete; stop separately drains the outer Tick return path after disabling hooks. Existing
post-SR native jitter removal/uniform rebuild confirmed. MSVC built only, no new tests/deployment.
Final Present association, vendor markers and borrowed UI allocation ownership remain open.

Shared UI producer migration now controls the verified game-instance mono/stereo canvas allocator,
keeps its1920x1080 logical layout and resizes before borrowed widget rendering. Replacement-frame
contributors repaint via native dirty state, with subsequent RefreshFPS preserved. MSVC Release
built only, no tests/deployment/game validation. Dormant/stereo/DPI/glow transitions and final
presentation/latency integration remain open. [Research](research/ac7-native-renderer-refactor-20261001.md).

Native ownership audit correction: renderer retirement drains final RHI pool releases before/after
its native wait/delete helper. Composition accepts only explicitly declared primary-view packets
and requires complete paired identity/rectangle equality; bounded capture counts failures too.
MSVC built only; no new tests/deployment. CPU semantic markers and final presentation remain open.

Native update association: derived/base GameEngine Tick and RedrawViewports hooks bind the
update-to-render phase into renderer identities and queued packets. Long-call guards cover
synchronous Present and retirement waits. MSVC built only, no tests/deployment/game execution.
Actual Reflex/PCL marker/sleep routing and final presentation association remain incomplete.

Native CPU event handoff now uses ABI5 copied callbacks for outer/input/simulation boundaries.
Runtime owns bounded CPU sequence state and a real-time optional consumer; F9 pairs source/QPC
metadata with native composition identity. MSVC built only, no tests/deployment/game execution.
Vendor sleep/marker consumer and final render/submission/Present association remain incomplete.

Main viewport submission provenance now binds through native FViewport Draw and renderer
construction (ABI6 viewport_key), queued packets/composition/F9 metadata. MSVC built only; no
tests/deployment/game execution. Slate's actual window/RHI Present handoff remains separate and
unfinished, along with vendor sleep/marker wiring and the final completion audit.

Pass completion correction: ABI7 separates semantic before/after callbacks from scope-state
restoration, resolves output resources again at end and removes an unused global pass copy.
MSVC built only, no tests/deployment. Slate window/task/RHI end-drawing functions are now mapped
and named; their queued source/swapchain handoff and vendor markers remain unfinished.

Queued Slate window ownership is implemented: bounded task-to-source binding, main-window checks,
leased native swapchain and execution-thread COM identity match at the existing Present observer.
ABI8. MSVC built only; no tests/deployment/game run. Shipped getter/layout, scene-resource agreement,
Present flags/result/generated handling and vendor Reflex/PCL consumer remain unfinished.

Actual Present completion contract implemented (observer ABI7, game ABI8 unchanged): original
interval/flags/HRESULT preserved, test calls bypass per-frame graphics work, runtime stores copied
numeric source/window completion/QPC evidence, and counter records accepted non-test calls.
MSVC built only; no tests/deployment/game run. Present1/generated handling, scene-resource agreement
and real vendor sleep/marker consumption remain incomplete.

Present1 coverage implemented through the shared overlay-aware installer/observer body (observer
ABI8). Original parameters survive forwarding; recursive internal calls do not duplicate events.
Route publication precedes activation and in-flight calls block uninstall. MSVC built only; no
tests/deployment/game execution. Scene agreement, vendor consumer and live coverage remain open.

Scene/window declaration agreement is implemented without capture copies: unique completed
primary composition matches the exact Present source/session/viewport and is rechecked after
return. MSVC built only, no tests/deployment. Actual sampled texture/final HUD-less resources,
vendor sleep/marker setup and full completion validation remain unfinished.

Final family surface producer implemented (ABI9): full native postprocess scope leases the real
family render target through RHI completion, replacing HUD-intermediate identity. No ordinary
GPU copies added. MSVC built only, no tests/deployment/game execution. Exact Slate sampling/late
canvas contributions and vendor sleep/markers remain incomplete.

Ordinary Slate texture binding path implemented (ABI10): candidate-only source-surface filtering,
leased native texture across queued bind commands, exact source/window comparison and separate
texture-bound Present evidence. MSVC built only, no tests/deployment/game run. Other consumers,
late canvas/HUD-less resources and vendor sleep/markers remain unfinished.

3 October DLSS-G/Reflex MVP (game ABI11, overlay ABI7): guarded primary RHI submission scope,
copied flight classification, one D3D12 presentation/SR host, coherent constants/CPU tokens,
leased FG inputs, physical Present markers and requested/effective/active controls. MSVC/Rust
checks and RTX synthetic generation passed: 16 tagged source frames, 32 SDK presents, 17 sleeps
and 96 markers. COM/resize and missing-runtime/Intel native fallback checked. No new AC7 game,
HUD, latency or pacing acceptance. UE4.18.3 Launcher FID reference built/installed with 123,308
signatures; user-completed analysis supplied confirmed camera/controller getter matches.
The first new game run retained native presentation because its 10-bit buffer was excluded.
R10G10B10A2 support now passes the same active 2x fixture. The redeployed ABI7 HUD separately
reports rendered/application and SDK aggregate presented FPS with FG activity, using QPC and
real counters. New game activation still requires a run. [Evidence](research/ac7-dlss-fg-20261003.md).

Facade startup corrected after the user's Waiting for renderer report: shared observer
presenter/device/output bookkeeping now runs before facade callbacks. An observer-installed
RTX fixture verifies startup readiness, source counters and active 10-bit 2x. Paired build
redeployed; the next game run is pending. Current full gate is blocked by unrelated concurrent
Unity-header compilation; focused ownership checks are recorded separately.

Latest FG correction: native Slate WINDOW/TEXTURE_BINDING scopes were always refused by a
scene-only family/view requirement, so Present could not retire SDK tokens and shared SR
stopped after filling all six slots. Role-specific owner validation now admits the native
viewless packets while preserving source/viewport/window checks. Regression and RTX fixtures
exercise the real AC7 queued scope path: 16 tags, 32 SDK source/generated presents, active 2x,
all tokens retired, 96 latency markers. Full VS2026 Release verification passed: 36 tests,
six opt-in skips, Rust format/lint. FG stays enabled with Reflex On; AC7 flight activation is
still pending. [Cause and evidence](research/ac7-dlss-fg-20261003.md#slate-scope-rejection-and-exhausted-frame-tokens).

The 22:37 AC7 run confirms matching CPU/RHI/window/scene identity and flight classification;
the remaining refusal is the sampled-scene-texture prerequisite. Added the engine's direct
backbuffer path using exact canonical COM identity and full output extent. The queued RTX
fixture activates FG for both sampled and direct paths, rejects missing/mismatched ownership,
and retires all tokens. Full Release verification passed (36 tests, six opt-in skips). Game
activation and visual/pacing acceptance remain pending. [Evidence](research/ac7-dlss-fg-20261003.md#direct-backbuffer-presentation-path).

AC7 flight FG is now game-confirmed: the 22:51 run matches the direct backbuffer and reports
SDK active 2x; the user confirms generation. Real FPS fluctuates roughly 60-170 (also in RTSS).
Pacing correction rotates the three command slots independently of fence serial, submits the
D3D11 signal before flushing, and isolates SR/upload waits from the FG queue, following local
OptiScaler revision 92337ebf. Added per-second timing evidence and guarded hangar/briefing
classification. The 240-frame RTX fixture passes all three scene classes and both presentation
paths. Native build passes; the shared full gate hits eight unrelated version-proxy collisions,
and all eight identical binaries pass in isolation. Rust tests/format/lint pass. Corrected
AC7 pacing and the two additional scenes await live validation.
[Evidence](research/ac7-dlss-fg-20261003.md#flight-activation-pacing-correction-and-additional-scenes).

The 23:27 pacing deployment hit a startup regression from the concurrently combined loader,
before FG initialization. A dedicated `rsf_proxy_ac7` carrier now preserves DirectInput ordinal
1 and the original AC7 entry path, while including the new pacing/scene code. Its isolated
DLL-load/observer/D3D11 startup test passes and the 23:38 pair is deployed with FG On. The generic
loader is preserved. AC7 launch/pacing/menu validation remains pending; the full shared gate
still encounters version-shim collisions outside the isolated tests.

4 October: user confirms AC7 startup plus hangar/flight FG; pacing improved but input-correlated
dips remain. Briefing is deferred at the user's request. Deployed source-timing instrumentation
at 00:07 to distinguish Reflex, engine/input, RHI and SR waits; no scheduling behavior changed
in this increment. Exact diagnosis and a corrected pacing run remain pending.

The hangar capture now measures up to roughly 29 ms in the Reflex begin path during 34 ms
source-frame intervals. Corrected its ordering: next-frame sleep/input waits for preceding
application Present plus SDK bookkeeping, rather than only scene RHI completion. Aborts
release the wait; sent window messages can progress without consuming posted input. A queued
two-thread regression proves next-token acquisition remains held until Present completion
for sampled/direct paths, and abort recovery passes. Full Release gate is green (37 tests,
six opt-in skips); the final wait adjustment passes focused build/ordering checks. Actual
hangar dip resolution remains pending. Briefing and additional PCL input wiring remain deferred.

The user rejected the full-Present join: it worsened dips, so that barrier was removed. User
reports Reflex Off eliminates them. Corrected the separate bug that promoted requested Off
to On during FG; SDK sleep and PCL calls still run in Off. Added applied-mode and split slow-call
timing logs. Official NVIDIA guidance plus local UE4.27.2 source identify frame-wide marker
boundaries that still need mapping to AC7 4.18; no verified 4.18 Reflex example was found.
Saved RTSS marker injection is enabled, but effective interference is unproven; no-RTSS test
requested. Full Release gate and Rust tests pass. Deployed 00:55, FG On; pacing remains open.

The user reproduced dips with RTSS off, ruling out RTSS as a necessary trigger. Split timing
shows 28-29 ms inside Reflex sleep. Reinvestigation in the user's AC7 Ghidra project verified
full native frame boundaries: copied BeginFrame task identity, queued RHI BeginFrame/EndFrame,
and pre-frame-sync simulation completion. Implemented full-frame markers, simulation start
before input, six-marker token retirement and removal of the extra CPU join. ABI12 adds
mouse/keyboard dequeue records, guarded controller-poll markers and SDK-registered PCL ping
routing. Build/native gate and device contracts pass; corrected AC7 pacing and input behavior
await a user-run hangar test. RTSS coexistence remains required, briefing remains deferred.
Final full Release gate: 39 executed tests, six opt-in skips. Device fixture also verifies
SimulationEnd after Present and token retention through EndFrame. Deployed 4 October 01:34,
matching hashes in `.local/deploy-backups/ac7-pair-20261004-013424-912/deployment.json`;
fixed 2x FG and Reflex On remain configured. No game pacing acceptance yet.

The 07:57 run confirms fewer hangar dips, almost none in flight, and all input/PCL paths;
On + Boost still dips. Remaining SDK sleeps reach 26-31 ms with short GPU-active work.
ABI13 now reserves the token before BeginFrame dispatch and sleeps at verified native engine
pacing RVA 0x1adbcb0, matching the engine-owned Reflex ordering. Added read-only actual driver
sleep-state diagnostics. Full gate passes 39 executed tests/six skips; retail device contracts
pass, including RHI begin before sleep and duplicate-sleep refusal. Deployed 08:16 with backup
`.local/deploy-backups/ac7-pair-20261004-081610-248`; next hangar run is pending.

The delegated [lighting investigation](research/ac7-lighting-shadow-20261004.md) finds stippling
already in pre-SR colour. Directional contact-shadow noise is a lead; the existing F9 captures
lack its live uniforms and all use Ultra Performance. Reserved existing capture quotas for
its pre/post-light colour, live CBs, depth and shadow inputs, included in the 08:16 deployment.
The lighting defect is unresolved; matched Ultra/Quality captures are requested.

The user counted eight hangar dips on the native-pacing build and none in flight. Driver
readback confirms 33,333-us sleep during a dip with low latency enabled, foreground true,
2x FG, no VRR/forced VSync/DFG, and only about 3.3 ms GPU-active work. The 08:45 build moves
the vendor RenderStart from pre-input BeginFrame to actual scene RHI submission, retaining
full EndFrame/Present coverage and one start per token. This targets empty-queue timing in
the cross-API proxy; actual resolution remains unverified.

Fresh lighting captures prove the directional contact draw introduces the dots with contact
length 0.04, flags 3 and advancing View phases 7/5/2. Applied a guarded DXBC transform that
adds the missing per-view phase to contact noise, preserving all contact rays and shader
signatures. Hooked the matched native pixel-shader factory and retained its resource metadata
and optional trailer. Disassembly, WARP/hardware shader creation and refusal/preservation
checks pass. Combined Release gate passes 39 tests/six skips; deployed 08:45 with backup
`.local/deploy-backups/ac7-pair-20261004-084546-571`. Hangar pacing and visible lighting
acceptance are pending; the latest F9 captures are all Ultra Performance.

The user reports both defects remain after 08:45. Shader creation did apply; an offscreen
execution probe proves its phase affects the contact branch, but moving-game quality remains
unresolved. The next F9 must confirm the bound patched shader and lighting delta.

09:20: restored the hidden native D3D11 swapchain/Present used by OptiScaler's DX11/DX12
reference wrapper. Our previous facade never ended a frame on its native D3D11 device.
Internal presents retain external hook forwarding but are explicitly excluded from application
callbacks and counters. Added bounded, non-filtering NVAPI caller/queue-state diagnostics.
Full gate passes 39 tests/six skips; hardware contracts verify no double-counting, resize and
marker lineage. Deployed with backup `.local/deploy-backups/ac7-pair-20261004-092012-652`.
Actual Reflex resolution and lighting correction remain open, pending the requested game run.

The 09:20 hidden-Present experiment worsened dips and was removed. Audited mode/marker callers
are Streamline with zero requested FPS limit; Reflex Sync feedback is disabled. Corrected the
legacy-to-flip VSync-off translation to query/enable supported tearing, use its Present flag
only with sync zero in windowed mode, and preserve the creation flag through resize. Full gate
and retail device presentation/resize/input contracts pass. Deployed 09:40 with backup
`.local/deploy-backups/ac7-pair-20261004-094032-444`; live acceptance remains open. Expanded
the bounded lighting capture to include missing material/GBuffer inputs and sampler states.

10:55: the Reflex sleep overlapped the previous frame. The game thread slept for frame N+1
while the render thread was still submitting and presenting N, and RenderSubmitEnd followed
PresentEnd. The game thread now waits for the previous source frame's Present to return (or
its native EndFrame or abandonment) before its one sleep, still ahead of input, and
RenderSubmitEnd precedes PresentStart. Simulation and input markers keep their real
positions. The wait is bounded and services sent window messages. `RSF_REFLEX_ASYNC=1`
restores the old order. Built; retail-runtime device fixture passes 17 sleeps/112 markers,
ordered sequence, single-sleep rejection and both join paths; full Release gate passes.
Deployed 11:01 with backup `.local/deploy-backups/ac7-pair-20261004-110157-411`; not yet
game-tested. Expected cost is lost game/render overlap in flight. Research:
`docs/research/ac7-dlss-fg-20261003.md`, "Ordered sleep after the previous Present".

11:15: the user's hangar run rejects the ordered sleep. It executed (join 2.9 ms per frame,
driver sleep about 0.7 ms) and five of 38 hangar seconds still stall at the driver's 33333 us
interval. The frame ownership audit is clean in the hangar: one scene, SR call, Present and
RHI frame per engine frame, no mismatches. Built, not deployed: a bounded dump of the driver's
64-frame latency ring at stall onset plus unstalled baselines, diagnostics only. Fixture and
Release gate pass. The cause of the driver-selected interval is still unknown.

11:31: deployed the latency-ring diagnostic build, backup
`.local/deploy-backups/ac7-pair-20261004-113112-520`. The 11:23 user run predates it and
repeats the 11:01 result (four of 36 hangar seconds stalled).

11:39 run on the diagnostic build: each of five hangar stalls follows, by exactly four
frames, one frame whose driver-reported cross-adapter copy time is invalid (`0x88000000` or
`0x90000000`). The machine is a hybrid laptop whose panel is driven by the Intel GPU. With
generation active the driver interval oscillates between about 4.8 and 7.1 ms and the delay
from PresentEnd to GPU end alternates between about 3.3 and 8.7 ms. The marker stream is
clean. Why the copy sample turns invalid is not yet established. A refresh cap written as a
test was removed undeployed at the user's direction. An accidental `git checkout` of
`dinput8_proxy.c` was reverted from a dangling blob and verified against the deployed binary.

12:17: added a user frame limit that applies to rendered frames before generation: ini
`RSF_FRAME_LIMIT_FPS`, an Insert panel control (overlay ABI 8) and a runtime conversion to
the driver's presented-frame limiter while generation is active. Measured on driver 616.92
in the visible fixture: a 20000 us rendered-frame limit at 2x paced 238 generating frames at
19.90 ms. Reflex mode and limit changes no longer reset generation history. Release gate,
overlay Rust tests, format and Clippy pass. Deployed with backup
`.local/deploy-backups/ac7-pair-20261004-121734-929`. Whether a limit removes the hangar
stalls is not yet tested. An online search for the driver's invalid cross-adapter copy record
found no public report of it.

12:20: the panel's frame limit used a typed value box, which cannot work because the overlay
has no keyboard input. Replaced with Off, minus, plus and a slider. Deployed with backup
`.local/deploy-backups/ac7-pair-20261004-122029-147`; not yet tried in game.

12:23: limit row simplified to a draggable value, a VRR button (OptiScaler's cap: presented
interval 0.3 ms over the refresh interval, converted to rendered frames) and Off. ABI 8 limit
fields now carry microseconds and the stats report the display refresh rate. Typing in the
panel remains impossible: no text input in the overlay contract and AC7 reads the keyboard
through DirectInput. Deployed with backup `.local/deploy-backups/ac7-pair-20261004-122354-572`.

12:26 run with the VRR limit: the driver's 33333 us stall did not appear in the 12 seconds
with generation active (too short to conclude), while a separate dip did: two adjacent
seconds in which the render thread's scene submission ran at 8.7 and 13.3 ms per frame with
52 and 55 ms peaks. The same pair of seconds exists in four of six earlier runs, once per
session, and was being counted with the driver stalls. 12:29: deployed a limit-aware stall
detector and a debug-only sampler that logs where the render thread is executing during such
a hitch; backup `.local/deploy-backups/ac7-pair-20261004-122951-223`. Cause of the submission
slowdown unknown.

12:41 run, limit on, 101 generating hangar seconds: pacing is steady under the limit and one
driver stall still occurred, again three frames after an invalid cross-adapter copy record,
this time following a late frame that lost one of its two presents. Over-presenting is ruled
out as the cause. Fixed my own activation hitch: the limit conversion lagged the SDK's activity
report by a frame and the driver doubled the interval once per activation. The aircraft viewer
was refused as an unknown screen because classification accepts only two view-target classes;
added the plane selector (RVA 0x925e10, cache 0x3a82158) and the hangar pawn as possessed
pawn, plus a log line stating each screen decision. The hitch sampler now waits for a settled
generating screen. Deployed 12:50 with backup
`.local/deploy-backups/ac7-pair-20261004-125005-200`; these changes are untested in game.

13:10: with Optimus off the user's 12:58 run has no driver stall at all (driver reports VRR and
independent flip for the first time), confirming the hybrid presentation path as that cause.
The aircraft viewer was refused because its camera owner is none of the hangar classes; the
hangar is now recognised by the world's game mode (`ANimbusHangarGameMode`), and history resets
on any change of camera owner. The alt-tab hitch is inside Streamline's re-activation. Deployed
with backup `.local/deploy-backups/ac7-pair-20261004-131031-130`; viewer untested in game.

13:21: contact-shadow stippling corrected in the shader. The ray now starts half a texel's depth
change above the surface, measured from the pixel's four neighbours, which removes the false
self-shadowing that point-sampled depth causes at reduced internal resolution. GPU replay on all
six captured frames: ground stipple gone, aircraft and vegetation contact shadows kept,
bit-identical output outside the contact branch. New transformed CRC 4154163049. Deployed with
backup `.local/deploy-backups/ac7-pair-20261004-132153-981`; not yet seen in game. Research:
`docs/research/ac7-lighting-shadow-20261004.md`.

## Codebase review pass (2026-10-09)

Implemented the [codebase review](reviews/codebase-review-20261009.md): shared helpers for D3D12, logging, shader compile, SRV formats and plugin boilerplate; the FG bridge reads engine identity from setup and has one retire export; the frame tap uses a game-supplied role table; per-frame cost reductions in the tap, SR bridge and Unity managed code; dead code removed. The full item table is section 8 of the review.

Status: built (Release, MSVC) and synthetic-tested (ctest 49/49, 7 hardware-gated tests skipped; cargo fmt, clippy and test clean; RTX shared-host test passed). NOT game-tested.

ABI: frame tap ABI 14 (role table, removed override target and UAV refusal). Unity bridge ABI 2 and native ABI 3; rsf_unity_set_policy is now rsf_unity_set_engine_spatial. Deploy the Unity plugin and ReScaleFrame.Unity.Managed.dll together.

Behaviour changes to watch in the next game run:
- AC7 FG needs dinput8_proxy to set units and identity; "units_to_meters must be positive" in the log means FG is off. SR engine version is now 4.18.3.
- Unity FG depends on pass.screen being FLIGHT. AC7 replay and other non-menu screens are now FG-eligible.
- DLSS-G and XeSS get depth and motion in NON_PIXEL_SHADER_RESOURCE; check for D3D12 validation errors.
- DLSS SR no longer resets history every frame while FG is requested but inactive. With FG off there is no HUD-less copy.
- Retire now joins on the provider queue (Unity DLSS-G): look for hitches or 10 s stalls. Trailing Flushes were removed; CPU waits flush first.
- On AMD and Intel there must be no Streamline load attempts. On RTX without a usable Streamline folder, FSR/XeSS/Off stay selectable.
- The interposer loads with DLL_LOAD_DIR and SYSTEM32; watch for "could not load ... (Windows error ...)".
- DLSS SR on native D3D12 now rejects invalid frames (pre_exposure, frame time, resource states).
- FSR/XeSS: exposure appearing or disappearing should not pump brightness; at most one provider reopen at start.
- Nested fullscreen passes no longer restore between each other (3.10): look for flicker, missing UI or SR corruption.
- Frame tap in compat mode: sampler bias, constant-buffer size patches and view constants (3.1 to 3.3); the "reconstruction input set" line must still appear.
- Pixel-binding diagnostic log lines are off by default (RSF_AC7_PIXEL_DIAGNOSTICS=1 enables them).
- The FG choice is saved only after a successful switch. A failed overlay start is final until the overlay is stopped.

### Shared sharpening prototype (2026-11-12, incomplete)

[Shader contract and open integration work](research/sharpening.md): OptiScaler
2fff542 RCAS/DA RCAS/DA DAS ports, perceptual HDR transport, internal constants,
D3D11 helper, embedded SPIR-V and regeneration script. The focused D3D11 WARP
fixture passes. Production pipelines, shared settings, live overlay controls,
engine surfaces, vendor descriptor plumbing and D3D12/Vulkan helpers are not
implemented. This is not game-tested or deployable sharpening support.

### Shared sharpening milestone 1 (2026-11-12)

Supersedes the isolated prototype status above: shared saved settings and overlay
ABI 15 controls now drive the D3D11 production pipeline, and AC7 forwards/persists
the same values. FFX native sharpening is mutually exclusive with custom passes.
[Modes, defaults, execution, ABI changes and evidence](research/sharpening.md).
The production synthetic fixture establishes an edge change and byte-identical
Off/zero-strength output. D3D12/Vulkan/GL and the remaining host/settings surfaces
are milestones 2 and 3. No game deployment or game-tested acceptance is claimed.

### Shared sharpening milestone 2 (2026-11-12)

D3D12 `rsf_sr12` now runs the shared post pass after vendor evaluation and before
copy back, with inline root constants and per-slot descriptors/scratch. The
RDR2 and Unity Mono native-SR hosts apply and save settings live without
rebuilding the SR context. Native FFX sharpening is exclusive with custom
passes. [Execution, ownership and device evidence](research/sharpening.md).
The WARP fixture covers HDR/SDR, flat/edge/alpha behavior, disabled bypass,
missing-depth fallback and three unsubmitted slots with different settings.
Vulkan, GL option forwarding and engine surfaces remain for milestone 3. The
engine-owned spatial/FSR1 branch remains outside the native temporal pass.
No deployment or new game-tested acceptance is claimed by this milestone.


### Shared sharpening milestone 3 (2026-11-12)

Vulkan post-upscale sharpening is built for FFX2/3, DLSS, XeSS and the FSR4
continuation path, with per-recording descriptors/constants and explicit GPU
retirement. GL forwards live settings to its SDR D3D12 path. Godot Resources,
Unity URP Options assets and Minecraft Sodium settings expose all 12 values;
engine-owned spatial FSR1 retains its engine sharpening control. The device
fixture covers HDR bounds, alpha, monotonic edges, MAS, missing-depth fallback,
hard depth boundaries and simultaneous recordings. Engine settings have ABI and
round-trip tests. Validation receipts and limits are in the milestone 3 report;
this describes synthetic and device evidence, not new game acceptance or a
deployment. See [Sharpening](sharpening.md) for every user option.

Milestone 3 receipts: MSVC Release gate 202 executed passes / 11 default opt-in
skips (213 discovered), 66 Rust tests, formatting and Clippy clean; Vulkan
opt-ins 3/3 after supplying runtimes; FSR4 continuation fixture six completed
frames; Khronos synchronization validation clean for the sharpening fixture.
Godot engine and Unity managed package builds passed; Godot Resource, Unity
managed/native ABI and Minecraft settings tests passed (Minecraft 44/44).
RDR2 host backends 1/3/4/5 and both D3D11 bridge variants passed with the release
overlay. The bridge's foreground-game DLSS activity check was not run after
foreground ownership was denied. These remain device/synthetic results.

### Managed Vulkan notice cleanup (2026-11-15)

Removed the managed Vulkan migration notice and its README license exception
after the owner confirmed VulkanStory is their own source code. This is a
documentation-only correction. Checked the diff and remaining notice references;
no build, deployment or runtime validation was needed.

## Native Vulkan DLL exports for external engines, 15 November 2026

The runtime DLL now retains the entire native Vulkan archive. Previously its link
retained only objects referenced internally, leaving SR, FG and latency entry
points unavailable to external engines. The standalone split facade is no longer
compiled a second time into the DLL. Release rsf_orchestrator builds, the new
native_vulkan_dll_exports test passes, and a separate process resolves all 22
entry points required by the TotK adapter under development. This is build and
DLL-contract evidence only; no TotK provider or game validation is claimed.
The full eng/verify.ps1 Release gate is still running and has reported Anomaly
FG device failures (active/generated-frame assertions). It is not a passing gate;
see build/totk-native-export-verify.log for the result.

## Mandatory integration shader-input audit, 15 November 2026

The shared game-render-analysis skill now requires an upfront shader-buffer,
depth, motion and material-coverage audit for every SR/FG integration or migration,
without waiting for a user-reported symptom. Codex uses the canonical .agents
skill; Claude has a discoverable forwarding skill pointing to the same file.
The UE4 integration skill explicitly invokes the shared audit. This is a
workflow correction, not evidence that existing integrations' shader gaps are
fixed. All three skill folders passed skill-creator quick_validate.py, the
Claude reference resolves, and the focused diff check passes. No runtime changes
or game deployment are included in this documentation commit.

### Fallout 4 worker join snapshot correction, 16 November 2026

Corrected the stale joining snapshot path described in
[the dropout investigation](research/fallout4-sr-dropout-20261116.md).
A selected worker revalidates its exact predecessor join before retrying the
complete native proof. Moving-game dropout acceptance remains pending.

Fallout4 join retry trial PID18964 failed user Quality1 acceptance; stale-mode
refusal and temporal/spatial alternation persist. Partial capture and close AV
are retained in the dropout investigation. The proposed unlimited predecessor
wait was built but never deployed, then removed after finding deferred abort
while the callback is busy. Runtime rebuilt with the original bounded behavior;
source cancellation and token lifetime remain required repairs. NVIDIA's four
skill junctions and both checklists for each were verified readable.

Fallout4 cancellation delivery now reaches a busy synchronous RESERVE callback
without retiring its ticket. Shared pacing defers cancellation-driven token
retirement until callback return. Release build and focused reservation/worker
contracts pass, including cross-thread cancellation during RESERVE. No new
game trial;20ms dependency deadline and visual acceptance remain unresolved.

Cancellable predecessor wait deployed in PID62468 after protecting in-flight
pacing from capacity retirement/slot reuse and preserving concurrent cancellation
across token acquisition. User reports improved rendering but backend-switch hang.
Dump retained; process exited before agent kill attempt, controller exit0/restored.
SR quality, switching stability and FG remain incomplete.

Fallout4 closes an unsealed predecessor by cancelling its exact reserved
successor before native worker join. This addresses the source-level wait cycle
identified after PID62468. Matching Runtime REA/IDA evidence is retained in the
trial report. Adapter built; existing focused contracts pass; switch retest pending.

PID65288: user confirms backend switching works after exact-successor
cancellation, but rejects SR below DLAA. No counted evaluation refusals in the
retained transitions; reduced-path image quality remains failed. This narrows
next work to skipped producer paths, temporal inputs/history and downstream
consumers, with complete same-frame capture still missing. FG remains unvalidated.

Reduced-only follow-up: bounded native TAA-gate receipts built into the Fallout4
adapter to correlate actual TAA admission with reconstruction/source identity.
No rendering behavior changed; capture and game validation still pending.

PID50468 now supplies a complete reduced capture at source7833/native8145,
1707x1067 to2560x1600, SR0 and matched Present0 with final image. Static output
is populated; user still rejects temporal dropout. Reset1 in this focus-transition
sample requires settled adjacent-frame history investigation, not acceptance.

Fallout4 reset reasons and producer-snapshot refusal are now separately observable
through bounded receipts. TAA receipts no longer share the16-message generic
budget. Build and camera contract passed; rendering predicates unchanged.

Confirmed reduced-SR refusal: PID8348 native2739 changed only timing18ms->34ms
inside the same native frame. Input identity now excludes that mutable timing
sample while retaining all render/input guards and first-snapshot frame delta.
Shared D3D11 automatic failure capture added with saved overlay control, exact
failure-source resources,3/session cap and explicit temporal failure on spatial
recovery. Release builds and3 focused contracts pass. Neither change deployed;
sequence capture and full capture-content/game validation remain required.

Shared diagnostics now provide Capture4readySRframes through the overlay. It
waits for ready unchanged gameplay configuration, preserves manual captures,
and excludes duplicate source passes. Runtime build and capture-policy contract
pass. tools/analyze-sr-captures.py distinguishes actual evaluated/Presented output
from spatial recovery despite the old misleading SR0 metadata, checked on two
retained real bundles. No new game/device trial; timing repair remains undeployed.

Capture harness automation: independently versioned16-byte capture-control C ABI
and maintained tools/runtime-action.py expose shared failure/sequence controls.
tools/capture-sr-sequence.py waits for stable foreground SR, arms one sequence,
collects exact-PID tagged bundles and writes success/failure/timeout reports.
Runtime build, rebuilt host ABI and capture-policy contracts pass; Python fake-
runtime completion/timeout/report/cleanup tests pass. First ABI test build failed
on a local variable name collision and was corrected before the passing rebuild.
No new game or device fixture was run. Timing-fix game acceptance remains pending.

PID74532 exercised automatic SR-failure capture:3exact-source recovery bundles
retained and cap enforced, now with truthful SR-3 metadata. The timing-only
reread failure was not logged; remaining native1933 rejection is depth-producer
lease validation. Sequence report retained process-exit failure; controller
exit0/restored, no root close. Depth-lease branch/change receipts built for
remaining diagnosis, guards unchanged. SR/FG acceptance remains incomplete.

Capture follow-up: fixed input_dump_result silently rejecting the new sequence
and failure marker names. A real filesystem-only capture_metadata_contract now
verifies emitted marker files and SR-3 preservation, without creating a device.
Runtime build and metadata/policy contracts pass. Fallout4 now offers exact-owner
recovery_native_output before Present for active failed-reduced captures; no
FG admission changed. Adapter builds; native content verification pending.

PID55388 proves depth snapshot equality can pass while CPU input provenance
fails. Added a separate existing-render lease with identity/thread/native-frame/
lifecycle guards and retained independent camera/resource checks. CPU timing
and FG admission remain strict. SR now propagates the already verified gameplay
screen to unblock capture-sequence selection. Adapter builds; existing focused
contracts pass. Undeployed corrections; live validation remains required.

PID57728 automated four consecutive Quality frames complete with reset0. DLSS-G
became active with valid2x output telemetry and repeated tagged sources, but user
reports motion suspension. Exact swap-claim receipt rejects consumed2845 because
new queue tail2846 arrived. Post-consumption handoff now checks the consumed
cursor and existing generation/owner checks instead of requiring an empty queue.
Adapter built and existing worker contract passed; moving FG retest pending.

PID27512 with consumed-cursor correction supplies another four complete reset0
SR frames; static FG remains active. Computer Use plugin was selected and its
absolute drag/key actions exercised, but did not establish camera motion.
Relative-input exception is pending user response; no synthetic motion acceptance.
Added --require-fg to capture harness and fixed FG capture identity: populate FG
view_id, join exact pass identity instead of comparing SR/FG pool generations.
Runtime build and real file-output regression pass, including mismatched-view
refusal. FG capture identity correction is not yet deployed.

Explicitly authorized relative camera pan now automated in tools/pan-game-camera.py.
PID27512 camera movement visually confirmed;6/32FG-active status samples, motion
still fails. Removed old consumed-predecessor raw/tail comparisons from next-poll
reservation/protection, retaining consumed-cursor/generation and fresh-poll guards.
Build and updated regression pass; undeployed. Interrupted trial restored21entries
with reusable hash-checked restore-capture-trial.ps1 after game exit. No new trial.

User accepted the tested SR/FG path and directed work to Reflex/PCL/XeLL/Anti-Lag,
without more exhaustive SR/FG tests. User then disabled automatic capture due to
loading stalls. No Fallout4 or capture runner was active at the check; restored
installation had no RSF INI enabling capture. Trial startup now explicitly writes
CaptureSRFailures=0 and scripted capture is opt-in via CaptureScene. Future
latency runs use lightweight logs/status. Synchronous capture can perturb loading
and pacing, so prior capture-run FPS is not an unperturbed latency benchmark.

Actual-only latency policy is now explicit in AGENTS. Shared D3D/Vulkan marker
admission rejects synthetic/substitute timing; missing actual boundaries are not
manufactured at render submit or input sample. Game ABI24 adds optional version3
receipt provenance and core dedup/counts, not yet emitted/routed to vendors.
Runtime build and three focused CPU contracts pass. See
[actual latency boundary research](research/fallout4-actual-latency-boundaries-20261116.md).
Address Library and Community Shaders identity reverified; matching EXE IDA query
in flight, existing matched dossiers reused. No autocapture or live test.

Fallout4 actual receipt producers now use optional eventv3: monotonic receipt IDs,
observed MSG.hwnd or unknown0, source0 and real callback QPC. Core receipt counts
are queryable through tools/runtime-action.py receipts-status, independent from
sampled frame input. Adapter build and three focused contracts pass; no game
run, no vendor notification association yet. Actual simulation-boundary analysis
continues in the existing live REA/IDA request; no synthetic marker fallback.

Actual-only latency follow-up: XeLL Vulkan markers now follow actual input/sample
and simulation callbacks; sleep no longer implies simulation-start. Submission
end requires its own callback instead of synthesis at Present. Required pacing
ownership remains unchanged. Runtime build and four focused CPU/mock contracts
pass, including Anti-Lag unsupported/failure and no-second-pacer cases. Native
FO4 simulation boundaries, PCL routing and hardware verification remain open.

Fresh matched-EXE REA/IDA confirms native input wrapper, worker update and main
Swap rendering boundary. Fallout4 now reports actual PlayerControls-entry and
pre-Swap simulation events for the exact consumed source. Core forwards observed
replacements of derived bookkeeping; only observed timing authorizes markers.
Runtime/adapter build and3focused CPU contracts pass. Live verification pending;
autocapture remains disabled. Evidence: actual-simulation-boundaries-rea-20261116.json.

Native actual-marker trial PID68740: Reflex On/Boost apply; Off with FG disabled
reports mode0 and continued sleep calls. Gameplay/pan telemetry shows real marker
calls, zero derived boundaries and zero order violations. Receipt counters now
observe actual keyboard/mouse/dequeued messages separately; PCL SDK notification
routing remains incomplete. Autocapture off. Prior stale-carrier ABI refusal was
restored; rsf_fallout4_trial now builds coherent carrier/adapter/runtime artifacts.
No end-to-end latency or XeLL/Anti-Lag hardware acceptance claimed.

PCL receipt routing implemented centrally: verified bound window/thread, unique
existing actual CPU-phase source, session/provider-generation/time checks, no
new token or synthesized marker. Notification calls use a generation-pinned
provider operation. Pure selection contract passes; core/host build corrected
and passing. Live notification and UI telemetry verification remain pending.

XeSS-FG switch (2026-11-16): user-reported hang remains failed acceptance. Shared XeLL sleep/marker lock split and bounded concurrency regression implemented; native retest pending. Recurrence prevention recorded in the rendering skill's latency-sleep-locks reference and linked Reflex/PCL skill. See docs/research/fallout4-actual-latency-boundaries-20261116.md.

Fallout4 XeSS-FG 128-frame stall: CPU dump and REA confirmed a Steam Present lock cycle. Shared Steam facade dispatch now occurs before its lock; fixed live run passed 784 tagged frames, switched Off and back to XeSS, reported 28/56 FPS and 950 XeLL sleeps. Full visual/latency acceptance remains separate. Evidence: docs/research/fallout4-actual-latency-boundaries-20261116.md.

Fallout4 XeSS-FG performance: physical Present interval3 violated XeLL's interval1 requirement. Runtime normalization built and game-observed in native-raster-20261116-233650-937:82 base/164 presented FPS, XeSS FG active2x, SDK results0. User: Much better now. This is sampled performance/user acceptance, not a controlled benchmark. Autocapture off.

XeLL settings completion guard: actual sleep-parameter changes now require bounded completion on the supplied D3D12 queue before xellSetSleepMode. Busy sleep defers without blocking earlier-frame progress. Release build and focused CPU regression pass; new guard awaits game settings-switch verification. Current live game retains the accepted interval fix.

User acceptance, deployed Fallout4 trial233650: Everything seems to work perfectly fine now. Recorded in engine.json and actual-latency research. Additional XeLL settings guard remains built-only and is excluded from this deployed acceptance.

PCL telemetry: overlay ABI18 now carries actual keyboard/mouse/controller receipts and routed/failed/unmatched notification counts. Panel states SDK coalescing explicitly. Runtime/overlay Release builds and56 Rust tests passed; live UI verification pending. NVIDIA verification executable not found in searched local SDK and standard install directories; vendor/controller/lifecycle acceptance remains open.

NVIDIA PCL tools located in official Reflex revision c2522cee75be7a952ab29281c4cf6d978c5e2e0e. PrintPCL failed FvSDK_CreateSession -1; full validation requires FrameView SDK setup/system restart per consumed NVIDIA guide. No installer, setup batch, reboot or vendor-tool acceptance performed.

Session stopped at user request. User accepts the deployed SR/FG state as sufficient. PCL overlay ABI18 is built and CPU-tested; new overlay and XeLL settings guard were staged in trial235214, but gameplay/UI/settings verification was interrupted and remains incomplete. The stopped trial's21 manifest entries were restored and hash-verified by tools/restore-capture-trial.ps1 (resumed-restoration-20261116-235448-824.json). No FrameView installer, verification setup, reboot, full eng/verify.ps1 suite, release or readiness promotion performed. All current tracked changes and source diagnostics are checkpointed; licensed binaries, external references and captures remain ignored.


### Vulkan SR provider admission (V01)

Rust shares FidelityFX staging allocation with the Vulkan resource owner and
preserves cleanup-only owners after refused FidelityFX or XeSS rollback. Vulkan
SR captures the SDK output before RSF sharpening. The split FSR4 dispatcher
uses its admitted records to create the owner. The XeSS fixtures exercise the
production dispatcher, and a Rust fixture checks the Runtime DLL export
inventory. [V01 port evidence](research/vulkan-sr-provider-admission-port.md)
records the source checks, fixture changes and remaining C++ paths. Build,
test, lint and device validation are pending.

### Shared SR sessions and bridging (D03)

Rust owns shared SR frame admission, auxiliary binding restoration and
cross-API retirement. Checked close exposes a retained bridge for retry, and
an output-only size change retires and reallocates the prepared surfaces.
Provider switch logs distinguish a committed FidelityFX version from a
candidate still under review. See [D03 source evidence](research/rust-sr-sessions-bridge-port.md)
for the D01 startup dependency, pending Anomaly Auto admission and fixture
coverage. Build, test, lint and device execution remain pending.

### D04 D3D12 host source port

The shared hosts preserve fence failure codes, retry provider teardown before
replacement, and carry SR capture identity through evaluation. The C11 FG12 and
SR12 ABI fixtures preserve their DLL and embedded registrations. The
[D04 source record](research/d04-d3d12-host-port.md) lists the five remaining
C++ paths and the Rust review resolutions. Verification covers source syntax,
bindings and pinned formatting. Build, test and device execution remain pending.
