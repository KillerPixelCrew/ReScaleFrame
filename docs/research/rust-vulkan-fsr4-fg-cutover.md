# Vulkan FSR4 frame generation Rust cutover

The baseline is `a339246`. This ports the Vulkan broker that drives the existing
exact-family-4 FidelityFX DX12 bridge. The [original interop record](godot-fsr4-fg-interop-20261025.md)
covers SDK admission, queue ownership, producer data and previous device limits.
This cutover kept the same shader inputs, vendor calls and external-resource
protocol. Its evidence is source inspection, Windows builds and CPU contracts.

## Ownership and frame flow

| Concern | Rust owner and native boundary |
| --- | --- |
| Admission and resource lifetime | Rust validates the private WSI policy, obtains the Vulkan adapter LUID, opens the FidelityFX bridge, verifies the returned LUID and count-one capability, then imports the bridge's shared timeline and images. It starts the presenter only after these steps succeed. The unchanged SDK bridge enumerates family 4 by its paired version string and verifies the opaque ID on the created context. The existing importer owns each Vulkan import and its DX12 COM reference together. Rust owns the checked lifetime of both bridge and importer, the command pool, the three Vulkan backbuffers, the six source slots and their fence handles. |
| Guides and HUD-less staging | The Godot producer passes render-sized copyable R32F normalized depth and RG16F motion at the SR pass, then output-sized RGBA8 HUD-less color after postprocessing and before the canvas. Rust checks the device, extent, format and shader-read layout. The shared recording ledger checks session, frame, view and resource generation before joining HUD-less with the guides. Independent object and deformation motion remain the engine's inputs. Motion-to-pixel scale, jitter in render pixels, the camera convention and frame time in milliseconds reach the existing SDK PrepareV2 call. |
| Exact submission and reuse | The notification covers successful submissions of the tracked graphics command buffers to the queue represented by `setup.queues[0]` and `graphics_submit`. The inspected Godot checkout at `ed1daf0bf001b61586d9930840f2f1394092c079` plus the RSF driver patch creates/resizes the primary swapchain through `main_queue` and executes frame and split render-graph command buffers on that queue. The module binds its graphics callback to queue index 0 from that family. The notification follows successful `vkQueueSubmit` after unlocking the queue. Shared recording order discards superseded command-buffer recordings. One Vulkan fence marks a joint successful notification. Separate fences mark split notifications; their waits occur on busy slot reuse. Layout state advances only after the matching submission is marked. |
| Present and retirement | Explicit Present reads only the published identity prefix and selects that recording even when newer work has been staged. Missing, partly submitted, mismatched or forbidden inputs keep FG Off and present the completed real image. A matched recording can generate once. Rust assigns `ready` and `done` in pairs, preserves the original Present waits and chains the previous DX12 `done` before the next Vulkan `ready`. The DX12 bridge still signals only after its SDK work and HUD-less ownership transfer are complete. Rust requires both API drains before release, retains owners on unknown completion, and retries partially completed teardown without repeating releases. |
| Diagnostics | Existing Vulkan readback and the shared Rust frame-capture owner still reserve and publish captures. The FSR4 capture callback attaches recordings to the prepare or HUD-less submission group. A slot fence or the drained Vulkan device proves completion of those groups. Superseded groups cannot publish and are released after the device drain. Provider capture metadata identifies FG backend 4. Generated-callback telemetry still comes from the SDK output-resource callback. |

The color copy, format conversion and ownership barriers remain Vulkan leaves.
The Off path retains its existing placeholder allocation and never passes those
images to an enabled SDK PrepareV2 dispatch. The legacy Present call uses the
latest accepted recording for the engine's serial graphics path. Both copies
still require that recording's submission evidence. The source review covers
the three staged source inputs and the final presentation image. Active shader permutations,
object and bone history, transparencies, particles, volumes, HDR, resize and
moving-frame image quality were outside hardware validation for this port.

## Verification

The isolated Windows MSVC build used Visual Studio 18 2026 and the coordinator's
reference and vendor paths. The installed FidelityFX header checkout used for
this build was `7539ae8b6568ad29ed78a753404351daf3b5cca7`. No SDK binary was used
by the CPU tests. The corrected optional Streamline Vulkan package was absent
from this configuration.

- `cargo test --workspace --locked` passed all 161 tests. Formatting and
  `cargo clippy --workspace --all-targets --locked -- -D warnings` passed.
- Eight CPU contracts passed in Debug and Release: `native_fsr4_fg_contract`,
  `native_fsr4_fg_state_contract`, `native_fsr4_fg_bridge_contract`,
  `native_fg_vulkan_dispatch_fsr4_contract`, `native_fg_vulkan_dispatch_contract`,
  `native_dx12_vulkan_interop_contract`, `native_ffx_vulkan_submission_contract`
  and `native_vulkan_dll_exports`. The Release build with `RSF_FFX_INCLUDE=OFF`
  passed `native_vulkan_contract`, `native_fg_vulkan_dispatch_contract` and
  `native_vulkan_dll_exports`.
- The FSR4 state contract uses the real Rust state owner with CPU doubles for
  the Vulkan and SDK calls. It checks cross-producer refusal, split notifications,
  superseded recording groups, older exact Present with a short identity,
  one-time admission, Off fallback, ordered timeline values, completion before
  slot reuse, same-adapter admission, terminal retention and cleanup retry. The
  existing SDK bridge tests still check family-4 selection, PrepareV2 constants
  and error/terminal behavior.
- The shared FFX submission fixture initially failed to build because it still
  named `self.latest` at the baseline. Its older-Present assertion now observes
  the shared recording timeline's latest accepted frame.
- Release export names and ordinals matched the coordinator's
  `build/rust-migration/baseline-exports` for Runtime (381), AC7 carrier (256),
  shared loader (273) and Minecraft (267).
- Built the Vulkan SR/DLSS and XeSS fixtures, the XeSS Vulkan presentation
  fixture, the Vulkan readback fixture and the FSR4 bridge fixture. Ran only
  CPU entry points. Device, visible-window, game, and full verification runs
  were outside this cutover.

## FidelityFX DX12 bridge ownership

Starting at `b01e43d`, the lower bridge uses a separate Rust owner behind the
existing `RsfFsr4Fg*` C entry points. Source review followed both the Vulkan
broker's bridge calls and its callbacks into the shared Vulkan/DX12 importer.
The optional CMake source block supplies the native operations and links the
Rust archive for linked consumers and the Runtime DLL. The Rust module needs
no SDK headers or link symbols when the block is disabled.

Rust owns the LUID request, provider path and exact family-4 selection, the
created-provider ID check, HWND admission, resource and camera validation,
sequence and reset state, callback telemetry, and checked retirement. C++
queries the SDK and marshals its typed descriptors. Its stable allocation holds
the retained context descriptors, COM objects and SDK callback. The callback
uses a separate Rust atomic allocation that lives through native release.

The source review against the previous bridge and FidelityFX headers at
`7539ae8b6568ad29ed78a753404351daf3b5cca7` followed the output-sized BGRA8
real and HUD-less inputs and render-sized R32F depth and RG16F motion. Configure
precedes PrepareV2 for each enabled frame. The DX12 queue waits for Vulkan
`ready`, executes the frame and presents. The SDK present wait and local fence
prove completion before HUD-less returns to COMMON and the queue signals
`done`. An Off frame still configures the SDK and clears an earlier callback
error only after configuration succeeds. A retry after a local-fence timeout
checks the fence value after waking because an earlier wait can still signal
the shared event. A partial command setup without an event has submitted no
work, so its cleanup skips the local signal and wait. Teardown still releases
the generation context and DXGI chain before the swapchain context, and keeps
the remaining owners available on failure. A prepared bridge cannot enqueue
another handoff or drain signal before final release.

The CPU fixture now drives the actual Rust bridge through native doubles for
ordering, resets, error propagation, terminal retention and partial retirement.
This change was reviewed at source level and checked with pinned Rust
formatting and Git whitespace checks. Compiled contracts, export checks,
disabled-build linkage, device and moving-frame behavior await verification.

## DXGI and D3D12 setup ownership

The graphics setup comparison used RSF `59783098`, Windows SDK `10.0.26100.0`
and FidelityFX headers at `7539ae8b6568ad29ed78a753404351daf3b5cca7`. The
Rust bridge now orders the setup steps and supplies their scalar settings.
The native capsule keeps the COM references, Win32 handle and typed SDK
descriptors at their existing addresses. The frame lease tracker covers
submitted resources with queue and vendor retirement points. This setup has
partial owners before either point exists, so it keeps the capsule's checked
cleanup path.

| Path | Source comparison |
| --- | --- |
| Probe and create | Create the DXGI factory with flags zero, enumerate by the exact 64-bit LUID, read `VendorId` from `GetDesc1`, and create a D3D12 feature-level 12.0 device on that adapter. The native leaf copies the LUID bits into `LUID` and reports the same failing HRESULT. Provider path construction, DLL loading and version enumeration follow device setup. Probe publishes the LUID, vendor and selected version only on success and releases its temporary capsule. |
| Command setup | Create and verify the selected FG context before creating the normal-priority direct queue, direct allocator and node-zero direct list. Close the fresh list through the same leaf used during frame recording, create a non-shared fence at value zero, then create the unnamed auto-reset event in the unsignaled state. The event completes the native local-fence predicate. A failed stage skips all later stages. |
| Partial create | The original error is returned. Checked cleanup can return the bridge for retry if it cannot destroy the generation context or otherwise retire the owner. Failed command setup without an event leaves the local-fence predicate false, so cleanup skips signaling and waiting on the unsubmitted queue. Native final release still disposes of partial COM owners in the same order. |
| HWND start | Use the admitted output extent in pixels, BGRA8 UNORM, one sample, three render-target-output buffers and flip discard. The initial quality, stereo and flags are zero, with stretch scaling and unspecified alpha. Rust supplies borrowed setup values, which native copies into the retained `DXGI_SWAP_CHAIN_DESC1`. The retained typed FidelityFX version and HWND descriptors still point to that storage and the same factory, queue and chain output. |
| Failed HWND start | The pinned FidelityFX header exposes a mutable DXGI description. An admissible retry refreshes the seven fields that the baseline assigned on every attempt, preserving the other five fields in native storage. The raw SDK result is returned, with the existing `E_UNEXPECTED` when it reports success without a chain. A returned chain or swapchain context prevents another start and remains available for checked teardown. |

The source check compared the private 36-slot operations table, both setup
records and the existing fixture initializer. It traced failure returns through
probe, create, start and teardown. The exported bridge types and entry points,
optional-build wiring, provider and shader paths stay at the baseline. Pinned
Rust formatting, locked offline metadata and Git whitespace checks cover this
change. Compile and link, Clippy, the existing contracts, export verification,
device and game behavior remain open.
