# Vulkan XeSS frame generation Rust cutover

The baseline is `7dfa446`. The Vulkan presenter and private interop wrapper now
use the shared Rust Vulkan/DX12 FG owner. XeSS and FSR4 share WSI admission,
image and command-buffer lifetime, the frame recording ledger, Vulkan
submission, the bidirectional timeline, capture retirement and teardown.
Their existing DX12 bridges still execute the SDK and graphics calls. The
[original XeSS interop record](godot-xess-fg-interop-20261025.md) covers the
historical device results and failures. The
[FSR4 migration](rust-vulkan-fsr4-fg-cutover.md) records the shared mechanism.

## Producer and queue evidence

Source review used the pinned Godot checkout
`ed1daf0bf001b61586d9930840f2f1394092c079` with the RSF driver modifications.
The local XeSS headers and developer guide were at
`8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0`.

| Input or boundary | Producer and runtime use |
| --- | --- |
| Depth and motion | At the Godot SR graph callback, `resolve_motion` supplies render-size R32F depth and RG16F motion. The adapter supplies signed texture-motion-to-render-pixel scale, unjittered row-major camera matrices, render-pixel jitter and inverted depth. Rust checks the source device, size, format, layout and copy-source lease before staging copies. |
| HUD-less color | The adapter reports output-size RGBA8 after postprocessing and before canvas. The shared ledger requires the same session, view, frame, resource generation and render extent as the guides. The copy is converted to BGRA8 for the DX12 bridge. |
| Final color and queue | The Vulkan presenter reads the completed game-facing RGBA8 or BGRA8 image in shader-read layout, including canvas, and stages its BGRA8 shared color. The patched driver admits the primary chain with graphics queue index 0. It reports submitted engine command buffers after successful `vkQueueSubmit` and after releasing the submit mutex. The native submit callback uses that same queue. Rust uses queue fences for source reuse and capture completion, and discards superseded command-buffer recordings. |
| Vendor handoff | The present submission preserves the engine's semaphore waits and an ordered prior DX12 done value. Native barriers release the shared Vulkan images to EXTERNAL ownership and reacquire them for reuse. The bridge waits for Vulkan ready and signals done after its DX12 work. The Rust owner waits on the real timeline and checks both queue drains before retirement. Bridge consumers retire before shared images and the importer. |
| Exact and fallback presentation | Explicit Present matches the published source identity prefix, including older work. The legacy XeSS path uses an active render/Present identity from the shared latency ledger when available. Missing or unsubmitted guides disable interpolation and keep the real image path available. The bridge only sees private fallback guides with FG disabled. Capture metadata follows a matching record, and the generated-output counter comes from observed SDK callbacks. |
| XeLL | Rust reserves increasing frame IDs before the SDK call and publishes a mapping after successful sleep. The sleep gate is separate from the marker and Present state. A real Present with a published CPU identity can emit markers even without FG guides. The fallback ID sent to XeSS FG never creates a XeLL mapping. Changed options are serialized with sleep and applied after the bridge confirms DX12 completion. |

The migration keeps the existing shader producers, formats and GPU bindings.
The source review follows the rendered depth and motion resolve, completed
HUD-less scene, canvas-composited image and vendor consumers. Active material
permutations, independent object and deformation motion, particles, translucency
and volumes retain the limits of the earlier
[engine integration evidence](godot-engine-integration.md). This cutover has
no fresh device capture or moving-frame acceptance.

## Verification

- Windows MSVC 19.52, VS2026 x64. The Release Rust runtime DLL, AC7 carrier,
  shared loader, Minecraft DLL and directly linked XeSS Vulkan FG device
  fixture built. The device fixture provides import and presentation modes.
  PE inspection found the XeSS bridge in that executable and no Runtime DLL
  import. The Runtime DLL passed the loader export contract.
- All Release names and ordinals matched the coordinator's
  `build/rust-migration/baseline-exports` by PE inspection. Runtime had 381
  exports, AC7 256, Loader 273 and Minecraft 267. The build with Vulkan and
  vendor headers disabled also linked and passed the DLL export contract.
  Its 354 Runtime exports matched the enabled list after removing the 27
  optional Intel bridge exports and recalculating the ordinals.
- Seven CPU contracts passed in both Debug and Release. These were
  `native_fg_vulkan_dispatch_contract`, `native_fg_vulkan_dispatch_xess_contract`,
  `native_fg_vulkan_dispatch_fsr4_contract`, `native_latency_vulkan_contract`,
  `native_dx12_vulkan_interop_contract`, `native_fsr4_fg_state_contract` and
  `xell_sleep_concurrency_contract`. Release also passed `native_vulkan_dll_exports`,
  `native_ffx_vulkan_submission_contract`, `native_fsr4_fg_contract` and
  `native_fsr4_fg_bridge_contract`.
- `cargo test --workspace --locked` passed 205 tests, including 130
  orchestrator tests. `cargo fmt --all --check` and
  `cargo clippy --workspace --all-targets --locked -- -D warnings` passed.
  The new XeLL test blocks a newer sleep on an older real-frame Present,
  exercises pass-through without guides, rejects duplicate sleep and early
  markers, and checks that a failed sleep preserves the earlier mapping.
- An extra attempted FFX reset fixture build hit stale private C++ field names
  such as `rsf_ffxgvk::latest` and `Slot::consumer_completion`. The fixture
  and its included implementation are identical to `7dfa446`. The first
  isolated configure also received a yyjson payload outside the lock hash.
  The hash-matched local pinned files resolved configuration.
- Device execution, visible windows, game launches and the full verification
  suite were outside this cutover.
