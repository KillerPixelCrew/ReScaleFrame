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
