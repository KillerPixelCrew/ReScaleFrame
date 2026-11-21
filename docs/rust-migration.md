# Rust orchestrator and integration migration

Branch `rust/orchestrator-anomaly` starts at `2db07f4`. The first cutover replaces the orchestrator behind its existing C interfaces. The second replaces the Anomaly adapter. Each production path has one implementation during the move.

## Remaining integration ports

Carry the same Rust ownership and public C ABI through every in-tree game
and engine integration. Retain the engine-required managed entry points,
JNI and Godot binding surfaces, and the native calls that require SDK types
or fault guards. Adapter code continues to supply engine facts through
the existing GameSDK interfaces, with backend and pacing policy in the
Runtime. Preserve the supported paths, feature refusals and package names.

| Integration | Native ownership and entry points to trace |
| --- | --- |
| Project Wingman | The game DLL and its dedicated XInput startup owner are in Rust. Both XInput carriers share the native forwarding leaf. Current binary and live verification remain open. |
| RDR2 | The game DLL renderer, ASI and attach workers, input guard, probes, Social Club backbuffer owner, capture installation and admission, hook registration, GPU leases, draw and pipeline records, presentation and NGX metadata, pending shots and capture drain, artifact publication and diagnostic workers are in Rust. Verify native observation coverage and the interleaved submission and retirement paths. |
| Unity Mono and Unity package | Mono loader, plugin lifecycle, Runtime setup, package services, per-view D3D12 SR, presentation and input, D3D11 and Vulkan graphics, commands, shared surfaces and queued texture leases, D3D11 render-pass planning, Editor preview resources, Vulkan resolver state and checked package retirement are in Rust. Device, editor and player lifetime checks remain open. |
| Steam, Fallout 4 and Skyrim SE | The shared Steam chain and both games' cold creation, identity, session lifecycle, camera math, screen and CPU event ownership are in Rust. Skyrim also owns its renderer observations, pre-HDR producers, presentation, resolution, auxiliaries and Community Shaders handover in Rust. Fallout 4's producer resolution, World and Present admission, render-input selection, render hook lifecycle, reduced-render recovery, SR/HUD-less publication, render scopes, imagespace and TAA, and viewport, dynamic-resolution and prepass decisions are in Rust. Verify producer identity, recovery and presentation in game. |
| AC7 | The game DLL's GameSDK, native site admission, scene and velocity patches, renderer transaction, TrueSky, queued scopes, view buffer, UI, scene colour, contact-shadow rewrite, velocity shaders, variants, history, stream and commit planning, and per-device velocity renderer lifetime are in Rust. F9 scheduling, root registration, draw, dispatch, geometry and before-draw records and carrier startup admission are also in Rust. Native GPU reads and artifact I/O retain their existing boundaries. |
| Godot | The native control, D3D12 and Vulkan sessions and SR contexts, D3D12 submission ledger, window and input routing, Vulkan creation, Auto provider routing and compositor diagnostics are in Rust. The private engine object owns viewport mode and identity, reset and render admission, CPU event records, the CPU frame lifecycle and render frame identity. SCons linkage and GDExtension, editor and player checks remain open. |
| Minecraft | The native session, surface, message observer, frame packets, settings and status are in Rust. The Java mod, typed JNI marshaling and linked Runtime entry points retain their existing contracts. |

Merge coherent ownership changes to the migration branch and remove their
worker worktrees. The user runs builds and verification. Record each
candidate's revision and feature set, and record binary ABI, focused
fixture, device, and game or engine evidence separately. The earlier
orchestrator and Anomaly verification gaps remain open.

## What the current build and callers require

| Surface | Evidence in this tree | Migration consequence |
| --- | --- | --- |
| Runtime linkage | `runtime/orchestrator/CMakeLists.txt` builds a core object library, a DLL with additional D3D11 and overlay services, and a separately linked native Vulkan archive. `loader/proxy` and the Minecraft target also link core objects. | Check both linkage forms. A successful DLL import test covers only one. Preserve enabled optional exports for each build configuration. |
| Plugin lifetime | `plugin_session.rs` validates the plugin table, copies host services, counts readers and can return a live handle after failed preparation when retirement is busy. Anomaly bootstrap resolves prepare and start by name. | Preserve result and handle together, hold the module through the last callback, and retry retirement. |
| Input and output | The game SDK describes frame identity, camera, CPU events, passes and resource leases. The Anomaly SR driver supplies scene inputs and consumes an output texture. FG obtains presentation and tagging services separately. | Keep source frame, view, format, extent, ownership and retirement visible at each call. Preserve separate SR output and final presentation ownership. Add an ABI field only for a demonstrated missing fact. |
| Additional Anomaly imports | The SR and motion stages share `frame_dump.rs` to borrow the carrier-loaded Runtime and resolve its capture exports. The SR driver and live overlay use the colour-transport setter. The shader and overlay owners query patching, diagnostic and named texture services. | Check direct lookups as well as table entries. Keep optional-service behavior and the adapter's single Runtime instance. |
| GPU and vendor work | Sessions, provider selection, pacing, presentation, diagnostics and overlay live across core and linked libraries. Native D3D11, D3D12, GL and Vulkan have distinct submission and retirement paths. | Port policy once. Keep foreign SDK calls at their existing owner and retain API-specific synchronization. Anomaly exercises only part of this surface. |
| Game-owned work | Anomaly builds its engine hooks, input handling, shader interception, motion history, SR reinsertion and FG input preparation in the game DLL. Its motion patch pack is built separately. | Retain verified build profiles, live settings, shader pack layout, failure paths and packaging when changing implementation language. |

## Order of work and acceptance

1. Establish the native baseline from the branch revision. Record runtime exports and direct/embedded consumers for matching features. Run the existing focused CPU and C ABI tests. Fix discovered correctness defects at their owner and carry the regression coverage across the cutover.
2. Introduce one workspace crate for core code. Start with SR backend selection, whose two scalar C exports already have a native caller test. Link the same implementation into both forms, retire the corresponding C++ implementation and verify symbol names and behavior. Port plugin sessions and frame sequencing next. Exercise short structs, version refusal, busy shutdown and failed startup.
3. Move settings, diagnostics, overlay services, rendering sessions, latency, graphics and presentation ownership. Keep the published headers authoritative. Inventory each service's callers and failure paths before switching it. Compare exports against the baseline for each enabled backend and exercise existing consumers on their graphics APIs.
4. Run the current C++ Anomaly adapter with the new runtime. Check standard and AVX profiles, overlay changes, quality and backend switches, camera movement, moving geometry, UI, FG, resize, fallback and teardown against recorded acceptance. Capture the evaluation frame and resource identity for rendering comparisons. Recheck other linked and DLL consumers before retiring the old core.
5. Port Anomaly behind its existing game DLL entry and package it with the same runtime. Repeat the fixture, device and game checks on the Rust adapter. Record known pre-existing gaps separately from regressions. Then carry the same ownership and ABI rules through the remaining integrations.

The [Anomaly release record](releases/v0.1.2-anomaly.md) distinguishes the shipped payload from later shader-provider work. It supplies historical acceptance only. Record validation of this branch in the [implementation tracker](implementation.md).

## C ABI comparison

The GameSDK stays at ABI 24. The source audit at `6414a05` compared 88 headers across the SDK, Runtime and Anomaly against `2db07f4`. Four headers differed after removing comments and whitespace. `native_fg.h` adds three linked-runtime declarations, `d3d11_present_bridge.h` adds `rsf_d3d11_present_source_queue`, and `overlay_api.h` raises `RSF_OVERLAY_SERVICE_OPTION_LIMIT` from 24 to 32. `shader_compile.h` changes the compiler helper's native ownership. All existing C function signatures and public record layouts matched. At `2ceec77` the public include tree still matched that audit and the tracked carrier `.def` files still matched `2db07f4`.

The source check at `a6f4dedb` matched the tracked GameSDK and Runtime
public headers and all four carrier `.def` files to `2ceec77`. Each
of the 99 Runtime Rust export directives names a source definition in
the orchestrator crate. The Runtime DLL, combined loader and Minecraft
still consume the same `rsf_orchestrator_core` object target. Its public
link to `rsf_orchestrator_rust` carries the archive to both forms. The
DLL still retains the native Vulkan service archive with whole-archive
linkage.
Compared with `58fe95a`, five pre-existing C/C++ exports now use Rust
directives. They are the Runtime version, CPU event and dispatch, and SR
surface set and release entries. Anomaly still declares the same three
DLL exports as `58fe95a` and borrows the already-loaded Runtime by name
for session, presentation and FG lookups. Its CMake target borrows the
Runtime include paths. Matching-feature builds and PE export comparison
remain open for this candidate.

On November 21, 2026, both archived MSVC Release trees (`build/rust-migration` and `build/rust-migration-streamline`) matched the recorded case-sensitive export names and ordinals for Runtime (381), AC7 (256), Loader (273), Minecraft (267), and the staged C++ Anomaly adapter (3). Neither archived Anomaly DLL imports the Runtime DLL statically. Both archives predate `d4de507`.

The Runtime version entry point now returns the workspace version from
Rust with the existing C signature and static string lifetime. On the
pinned Rust toolchain, the rebuilt Streamline-enabled Runtime returned
`0.1.2` on two direct DLL calls, matching `VERSION`. All four Release
export inventories matched again. Five focused MSVC DLL and embedded
contracts, 333 locked Rust tests, formatting and Clippy passed. The
rebuilt Godot extension also matched all 398 export names and ordinals
and kept its embedded Runtime linkage.

Callers still need to account for documented behavior changes. Frame assembly requires the output record to advertise its capacity and refuses a zero or short capacity before writing. Overlay settings callbacks can read the committed values, and a second write returns `BUSY` while the first is active. Vulkan capture and SR recording require completion proof before readbacks can be published or reused. The [implementation tracker](implementation.md) records the fixture coverage for those changes.

## Shader patch and Anomaly configuration ownership

The Runtime DLL publishes the generic shader patch service through
`rsf_get_shader_patch_api`. Anomaly borrows that table from the
carrier-loaded Runtime. Rust owns the shared pack rules, claim registry,
proof of the original shader, transformed output, reflection policy,
cache and interception inventory. The adapter owns the admitted motion
manifest and its lookup. The merged Runtime reopened the old C++ cache,
and Standard compiled new replacements in a cold game cache.
[Integrated shader patch evidence](research/rust-shader-patch.md#integrated-runtime-and-cold-cache-standard-run)

The Rust SR stage owns the retained INI, vendor directories and live
settings. The native facade no longer keeps its second configuration
record. Standard verified the packaged DLSS and XeSS directory defaults
on the exact staged Release candidate.
[SR configuration evidence](research/rust-anomaly-sr-configuration.md#coordinator-integration-and-game-checks)

The shared presentation and overlay diagnostics now name the graphics
path. Native D3D12 Streamline registration follows the adapter's backend
and runtime-switching request. The DXGI fullscreen description preserves
the refresh rate supplied by the caller. The Runtime overlay and embedded
carrier use the same host owner and matching panel ABI check.

## Integrated linkage audit

CMake builds `rsf_orchestrator_rust` once for each selected target and
configuration. The Runtime DLL, the two combined DirectInput carriers
and Minecraft link it through `rsf_orchestrator_core`. The Runtime
shader patch library links that Rust owner and the existing native
compiler, reflection, JSON and hook leaves. The thin carrier loads the
Runtime DLL. Anomaly borrows the already-loaded module with
`GetModuleHandleW` and resolves the C tables by name. The public GameSDK,
runtime contract and shader patch headers match the baseline byte for byte.

The remaining shared D3D11 SR surface admission and release sequence now
run in Rust. The pipeline closes admission before releasing the surface,
and the public C entry points retain their signatures and exports in the
rebuilt DLL and embedded consumers.
[Surface and teardown checks](implementation.md#rust-orchestrator-and-first-anomaly-port)

## Source audit checkpoint

The integrated ABI audit examined `41aa6bd` and traced the later
Streamline host and FSR4 compatibility link paths. It matched 29
generated ABI files, the 18 frame-tap C entries and their native table,
and the SDK guards on the D3D12 provider calls and CMake targets. The
audit found one layout defect in the private D3D11 state snapshots.
The Windows SDK defines 253 class-instance entries, and both sides
now check the corrected layout. See commit `bc9256f`.

Source review traced Anomaly's by-name Runtime lookups and its linked
Graphics calls. Windows link verification still needs to cover binaries
that consume more than one Rust
archive. Anomaly links its adapter archive and the shared Graphics
owner. The Runtime and combined carriers also link the FSR4 compatibility
archive. With the corrected Streamline runtime present, the Runtime
also links the Streamline Vulkan lifecycle archive. The standalone
FidelityFX 4, Streamline and XeSS Vulkan bridges use the Windows GNU
target.

Build and fixture results below apply to their recorded candidates.
The Runtime and Anomaly binaries found under `build/` at `afe5eee`
predate the merged policy cutover.

| Check | Evidence still needed for the merged source |
| --- | --- |
| Build and ABI | Windows builds of the Runtime DLL, thin and combined carriers, Minecraft, Unity Native, Godot Native and Anomaly. Compare each PE export inventory against its matching SDK configuration, including ordinals and carrier forwarders. Check that Anomaly still resolves the already-loaded Runtime by name. The `native_vulkan_dll_exports` fixture checks a subset of named entry points through `GetProcAddress`. The standalone Windows GNU Vulkan bridges also need link verification. |
| Focused contracts | Run `input_receipts_contract`, `native_cpu_reservation_contract`, `native_fg_cpu_contract`, `d3d11_host_contract`, `sharpening_settings`, and the Rust input receipt and D3D11 capture budget cases. Check `plugin_session_lifetime` and `plugin_session_lifetime_embedded`, `callback_host_abi` and `callback_host_abi_embedded`, the Anomaly lifecycle and both profiles' input and SR hook contracts, and the enabled Vulkan provider contracts. Record the build commit and feature set with the results. |
| Device | Check provider switches, fullscreen and resize, GPU retirement, D3D11 state restoration, OpenGL sharing, capture completion and overlay drawing on the affected APIs. |
| Game | Use the same candidate on Standard and AVX. Record movement and held input, SR and FG switching, overlay capture and release, resize and fallback, and the observed shutdown sequence. Record an engine quit and explicit plugin retirement separately. |

## Native policy cutover

The source audit of the Runtime and combined carriers identified input
receipt routing and D3D11 capture budgets as native policy helpers
in the production core. The rights holder confirmed both helpers
were written for ReScaleFrame and authorized MIT. Commit `b2b1c5a` records
that authorization. The D3D11 host owns the capture budgets in Rust.
The Rust CPU dispatcher owns receipt replay, and FG frame routing uses
the Rust unique-owner selector. Their former native implementations and
private callbacks are removed. The CPU event and dispatch C signatures
are unchanged. Build and export verification of this cutover remain open.
The archived Release inventories in `build/rust-migration/baseline-exports`
give the following expected ordinals for builds with matching options.

| Binary | `rsf_native_cpu_dispatch` | `rsf_native_cpu_event` |
| --- | ---: | ---: |
| Runtime DLL | 227 | 228 |
| AC7 carrier | 123 | 124 |
| Loader carrier | 140 | 141 |
| Minecraft DLL | 134 | 135 |

The standalone Rust contract crate owns capability negotiation behind
`rsf_negotiate`. The existing C shim and fixture use the same request,
capability and choice layouts. The entry checks record prefixes before
reading the full request and keeps the original FG-before-SR policy.
The standalone fixture remains unrun. The Runtime uses its production
backend and session owners.

Scene promotion's Rust owner now admits the source texture, selects a
compatible view format and owns the partial texture, RTV and SRV
allocations until the replacement is complete. D3D11 calls, the common
SRV format lookup and the allocation receipt's native pointer formatting
remain in the native leaves. Source comparison against `301ad289`
matched all 21 format family entries using Windows SDK 10.0.26100.0,
the descriptor values and allocation order, and the 11 private table
entries. The public C declarations and existing `scene_promote`
device fixture are unchanged. Pinned format, locked Windows metadata
and whitespace checks passed. Link and device verification remain open.

The Rust fullscreen pass and present blit now decide draw admission and
derive the target extent from the texture and view facts. The D3D11
context type and device queries run in the original order. Sizing still
uses both explicit dimensions when supplied, refuses a zero base extent
or an out-of-range mip, and clamps smaller mip dimensions to one.
Source comparison against `a5867f2f` matched the six private callbacks,
the 12-byte fact record, the HLSL, command order and public entry points.
Pinned formatting, locked Windows Cargo metadata and whitespace passed.
The existing `fullscreen_pass` device fixture remains unrun.

The auxiliary SR owner now decides texture admission and the three
packed-colour format capability requirements using native descriptor
facts. The original device, array, mip, sample and usage checks run
in the same order. Source comparison against `119892c2` matched the
21 private slots and 40-byte facts record, and targeted bindgen
reproduced the bindings. The public host declarations and native
`auxiliary_sr_device` fixture remain unchanged. Pinned formatting,
locked Windows Cargo metadata and whitespace checks passed.

The retained standalone FSR3 Vulkan bridge now owns SDK binding, SR
contexts, FG contexts and swapchain lifetime in Rust. The same
`VulkanStoryFsr3*` entry points forward through typed FidelityFX and
Vulkan leaves. Source comparison matched all 21 export signatures.
Windows GNU linkage, binary exports and external host presentation
remain unverified.
[FSR3 ownership and export comparison](research/rust-vulkan-fsr3-legacy-sr.md)

The standalone VulkanStory NGX shim loads and publishes its SDK
symbols in Rust. The C shim keeps the native NGX call frames and the
same managed ABI. Windows GNU and Linux builds and binary inventories
remain unverified. [NGX loader](research/rust-vulkanstory-ngx-loader.md)

The Runtime's Vulkan-to-D3D12 FG owner now plans copy versus blit,
placeholder clear, image layout transitions and queue ownership in
Rust on the XeSS and FSR4 routes. The Vulkan SDK descriptors and command
calls stay native. [Transfer plan](research/rust-vulkan-dx12-copy-plan.md)

FidelityFX SR and FG share one Rust table for the four promoted Vulkan
device-function aliases. The loaders try the requested name before
the core alias. FG's miss on a null name now bypasses the alias lookup.
The typed calls and WSI borrow stay native. Link and device checks remain open.

FidelityFX Vulkan SR's staging allocation now follows the existing
create, requirements, memory choice, allocate and bind sequence in
Rust. The shared typed Vulkan leaves keep the same descriptors.
[Image allocation source comparison](research/rust-vulkan-ffx-sr-images.md)

The FidelityFX Vulkan SR owner also records first-use mask and output
initialization and output copy-back in Rust. It preserves the narrower
shader and transfer access masks, the exact allocation extent and the
failed-dispatch history behavior. [FFX image recording](research/rust-vulkan-ffx-sr-images.md#first-use-initialization-and-output-publication)

The integrated and standalone XeSS Vulkan bridge use the same Rust
Present input checks and resource transition and tag sequence. The
native leaves retain typed D3D12 and SDK calls. The source comparison
includes both HUD-less modes, distinct tags for aliased inputs and
the original failure cleanup. [XeSS input ownership](research/rust-vulkan-xess-present-inputs.md)

The D3D11 host now adopts the native creation facade in Rust, and
its presentation owner holds the three D3D12 command allocator and
list pairs through checked retirement. Native FidelityFX FG also
plans the Vulkan HUD-less copy and its barriers in Rust. The
device and game paths remain open. [D3D11 adoption](research/rust-d3d11-native-chain-adoption.md) ·
[Command-slot ownership](research/rust-d3d11-present-slots.md) ·
[FidelityFX FG copy sequence](research/rust-vulkan-ffx-fg-copy-plan.md)

The Vulkan-to-D3D12 interop owner now chooses all 13 required
instance and device functions in Rust. It preserves lookup order,
including later lookups after a missing address, and leaves
the native capsule unpublished on refusal. Typed lookups still
report exceptions through the original status. Source comparison
against `aa66153c` matched the names, scopes and slots and left
the public ABI, image and semaphore calls and device fixture
unchanged. Pinned formatting and locked Windows metadata passed.

The D3D12 SR owner now creates and owns the constant SDR exposure
texture and upload row through separate native allocation and write
leaves. Source comparison against `f446c181` matched the selection
gate, both resource descriptors, the 256-byte pitch, the 1.0 value,
the 53 private callback slots and the public C entry points. A
successful Map with a null address now unmaps and refuses the
allocation. Pinned formatting, locked Windows metadata and
whitespace checks passed. The linked and DLL SR12 fixtures remain
unrun.

The D3D11 host now owns bridge setup and installation ordering through
the existing presentation C ABI. Anomaly's FG input owner holds all
six staging resources and releases partial allocations in reverse
order. The FG private binding regenerated identically. [D3D11 bridge installation](research/rust-d3d11-host-bridge-install.md) ·
[Anomaly FG resources](research/rust-anomaly-fg-stage.md)

The native D3D12 overlay owns its three allocator, command list and
descriptor slots in Rust, preserving fence admission and partial
teardown order. Anomaly's overlay owner also orders and admits the
DirectInput and DXGI probes in Rust. DLSS and FidelityFX Vulkan FG
share a Rust staging sequence with their original source and output
extents. Build, device and game checks remain open.
[Overlay slots](research/rust-d3d12-overlay-slots.md) ·
[Anomaly probes](research/rust-anomaly-overlay-probes.md) ·
[Vulkan FG staging](research/rust-vulkan-dlss-fg-cutover.md#guide-and-hud-less-staging-commands)

XeSS Vulkan SR also plans guide staging, private output preparation and
copy-back in Rust. Its guide copies use the shared shader-read sequence.
Its output retains the XeSS-specific GENERAL layout and copy extent.
Source checks matched the private table and disabled routes. GPU and
game verification remain open. [XeSS SR transfers](research/rust-xess-vulkan-sr-cutover.md#transfer-recording-in-the-rust-owner)

The XeSS Vulkan SR owner also resolves all ten required SDK exports in
Rust. It retains first-miss refusal, cleanup and module pinning. The
native leaf binds the same typed table. Enabled and disabled builds
and binary and device checks remain open.
[XeSS SR resolver](research/rust-xess-vulkan-sr-cutover.md#required-sdk-exports-in-the-rust-owner)

The legacy D3D11 Streamline SR owner now selects the required
interposer and DLSS feature lookups in Rust. It shares the seven
common lookups and adds the five owner-only lookups for a session
it initializes. The typed SDK calls and borrowed-host behavior
stay at their existing boundaries. [Streamline resolver source comparison](research/rust-d3d11-streamline-resolution.md)

Native D3D12 DLSS now selects its ten interposer addresses and two
DLSS feature functions in Rust. Routes that own Present still require
the two common Present callbacks. Source comparison against `bef5a2d2`
matched lookup order, support and initialization calls, public
declarations and disabled stubs. Typed pointers now bind after all
selected feature lookups succeed. Pinned formatting, locked Windows
Cargo metadata and whitespace checks passed. Build and runtime
verification remain open.

Anomaly's Stage 3 burst owner now admits the source and device, builds
the staging descriptor and guards the readback map in Rust. The
existing D3D11 leaves still perform the single-subresource copy and
the predication and map effects. Source comparison and targeted
binding regeneration passed. [Anomaly SR staging](research/rust-anomaly-sr-stage.md)

The D3D12 Streamline host now chooses startup admission and SDK
preferences, stages graphics objects and rolls back initialization
in Rust. Source comparison matched the lookup order, signature
gate, enabled and disabled wrappers and linked consumers. Build,
binary export and device checks remain open.
[Streamline startup](research/rust-d3d12-streamline-host-startup.md)

The plain D3D12 physical swapchain plan in the shared presentation
bridge now comes from Rust. Compared with `d1f40c73`, the plan keeps
flip discard, applies only the supported tearing flag, and selects
the same direct or native queue. Native D3D12 retains the application
buffer count and usage. The D3D11 facade still uses three physical
buffers with render-target usage. The typed DXGI create, interface
query, error receipt and borrowed queue handoff match the previous
source. The private 24-byte record, pinned formatting, locked
Windows metadata and whitespace checks passed. Device fixtures
remain unrun.

The same presentation owner now acquires and retires the interop
queue, completion fence, completion event and optional application
ready event. Source comparison matched all 73 private platform slots
and 33 public facade signatures. Binary linkage, partial-initialization
fixtures and D3D11 and D3D12 device retirement remain open.
[Synchronization ownership](research/rust-d3d11-present-sync.md)

The presentation owner also owns the lazy provider retirement fence
and its receipt counter. The current private table has 72 callbacks.
The public retirement entry and both linked callers keep the borrowed
fence contract. The extended reentry fixture and device checks remain
unrun. [Provider retirement](research/rust-d3d11-present-sync.md#provider-retirement-receipt)

The Vulkan FSR4 FG bridge plans DXGI adapter selection, D3D12
command setup, and HWND swapchain creation in Rust. Its native capsule
retains the typed SDK descriptors, including DXGI fields preserved on
a retry. Source comparison matched the 36 private operations and
the shipped declarations. Enabled and disabled linkage, the bridge
contract, and device and game checks remain open.
[FSR4 FG graphics setup](research/rust-vulkan-fsr4-fg-cutover.md#dxgi-and-d3d12-setup-ownership)

The DLSS-G Present capture owner now admits the real image and
enumerates it under the same WSI lease. Rust owns the image list and
preserves Present's original waits on refusal. The native leaf
keeps the Vulkan calls. The extended fixture and live capture
remain unrun.
[Real Present image selection](research/rust-vulkan-dlss-present-capture.md#real-present-image-selection)

The shared overlay now routes its window messages in Rust, including
keyboard press and release pairing, touch gesture and handle ownership,
and visible raw-input forwarding to the default procedure. The typed
Win32 leaves and public input declarations match the existing caller
paths. Live input and native linkage remain open.
[Window message routing](research/rust-overlay-host.md#window-message-routing)

The OpenGL shared-surface owner now resolves WGL entries, admits the
two Win32 sharing extensions and acquires the device for the active
context's adapter in Rust. Native callbacks still perform the typed
GL and COM calls. The expanded fixture and device checks remain open.
[Context setup](research/rust-gl-shared-surface-cutover.md#context-capability-and-adapter-setup)

The source comparison after `038f0650` still matches the public
Runtime and GameSDK headers and the carrier export definitions at
`2ceec77`. All 99 Runtime Rust export directives have corresponding
function definitions in the orchestrator crate. The current DLL and
linked binaries still need their export and linkage checks.

The shared XeSS and FSR4 Vulkan FG owner now resolves its Vulkan
dispatch and checks the blit features before publishing the graphics
capsule in Rust. Native callbacks retain the typed Vulkan queries.
Build and live graphics verification remain open.
[Graphics admission](research/rust-vulkan-dx12-copy-plan.md#graphics-admission-and-dispatch-setup)

The shared XeSS and FSR4 Present path now admits the borrowed shape
and plans binary and timeline waits in Rust. The native Vulkan arrays,
result write and graphics queue call retain their typed boundaries.
Build and live graphics verification remain open.
[Present admission and waits](research/rust-vulkan-dx12-copy-plan.md#present-admission-and-waits)

Anomaly's Rust SR stage now admits render-target identity, view and
MSAA facts, and loading state. The native leaf reads the same
Standard and AVX profile data. Build, fixture and game verification
remain open.
[Frame and loading admission](research/rust-anomaly-sr-stage.md#frame-and-loading-admission)

## Verification coverage

| Requirement | Recorded evidence | Still open |
| --- | --- | --- |
| Core ownership | Rust owns plugin sessions, frame assembly, capture, provider admission, latency and pacing, presentation buffers, readbacks, paired physical DXGI swapchain and queue, and shared D3D11 panel state. The OpenGL and FidelityFX Vulkan WSI routes also use Rust ownership. The merged Standard clone exercised native rendering, DLSS SR and a DLSS-G to FSR3 and back provider transition. [Implementation tracker](implementation.md) · [Shared overlay host evidence](research/rust-overlay-host.md#integrated-runtime-and-standard-game-run) | The latest fullscreen, D3D11-on-12 overlay wrapping and provider cutovers need device verification. |
| Stable C ABI and linked consumers | The merged shader patch and configuration build kept all eight checked Release PE inventories and Anomaly's by-name Runtime lookup. The enabled Debug and Vulkan Streamline-disabled Release builds matched the four core export inventories and Anomaly's three exports. Both built and ran the public shader patch cache-reopen probe. [Build variants](research/rust-shader-patch.md#avx-cold-cache-and-build-variants) | Recheck affected artifacts after further changes. |
| C++ Anomaly baseline | Standard and AVX ran the existing C++ adapter with the Rust Runtime. Standard exercised the overlay, held-key gameplay, SR switching and DLSS-G. Both shut down through the engine. [C++ adapter baseline](research/rust-anomaly-cpp-baseline.md) | The baseline covers the exact recorded executable and payload hashes. |
| Rust Anomaly game acceptance | Both profiles reloaded the Rust manifest in the foreground and drew the merged overlay through resize. Standard switched native and DLSS SR and changed FG providers. AVX switched SR providers with DLSS-G active and returned gameplay mouse input after closing the panel. The combined build compiled byte-identical cold shader caches in both profiles and passed the native AVX opt-out run with a gameplay backbuffer. Earlier Standard evidence covers refused-SR spatial recovery. [Cold-cache results](research/rust-shader-patch.md#avx-cold-cache-and-build-variants) · [AVX opt-out](research/rust-anomaly-sr-configuration.md#merged-avx-opt-out-retest) | Verify held-key gameplay on the Rust adapter. The pointer-bound Forward attempt left the player indoors and the capture position stationary. [Movement attempt](research/rust-overlay-host.md#pointer-bound-movement-attempt) |
| Device and shader evidence | Offscreen SR and sharpening runs, exact Vulkan capture readbacks and live Anomaly results are recorded separately from synthetic contracts. Standard and AVX verified raw SR inputs and outputs. Matched AVX evaluations cover camera cuts, pans, small window-region motion, animated first-person geometry and zero fill with missing generated motion. The pending OpenGL, D3D12 readback, overlay and presentation fixtures link on the merged MSVC Release build. [SR device results](research/rust-anomaly-sr-pass.md#offscreen-device-acceptance) · [First-person zero fill](research/rust-overlay-host.md#first-person-zero-fill-with-missing-motion) · [Merged fixture builds](research/rust-overlay-host.md#merged-device-fixture-builds) | Moving world characters, OpenGL sharing, D3D12 readback, D3D11-on-12 overlay, fullscreen and windowed Vulkan DLSS-G remain open. |
