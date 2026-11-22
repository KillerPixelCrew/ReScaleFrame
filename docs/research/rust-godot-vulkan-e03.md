# Godot Vulkan port E03

This source change follows the pinned Godot 4.7.2 integration at
`ed1daf0bf001b61586d9930840f2f1394092c079`. The engine records the SR and
final-scene passes and owns the Vulkan device, graph recording and WSI. The
Runtime owns provider negotiation, SR transactions and FG guide allocation.
The existing inputs and resource domains come from the
[engine integration](godot-engine-integration.md) and
[FSR4 recording handoff](godot-fsr4-graph-handoff-20261025.md).

## Rust findings

| PORT.md title | Source evidence |
| --- | --- |
| Put fixed-guide allocation and recreation policy on the Runtime Vulkan presenter. | `integrations/godot/native/vulkan.rs` drops the fixed-guide cache and the reconstruction-generation WSI veto. The adapter continues to recreate WSI for FG provider changes. Runtime receives the existing frame record and guide extents. V03 commit `6f7a8b95`, integrated with V02 by `75287dc8`, resizes the retired pair after the source fences and DX12 consumer timeline. Integrate V03 before this change. |
| Derive Godot Vulkan's Auto masks and explicit admission from the same capability facts. | `Capabilities` in `integrations/godot/native/vulkan.rs` builds the SR and FG masks from one guarded snapshot. XeSS SR, FSR4 SR and XeSS FG use the same bits for explicit admission. The explicit-only DLSS-G and FSR4 FG paths, capacity checks and FSR4 FG probe keep their separate gates. |
| Use one reconstruction generation key for Godot configuration changes. | Both `control.rs` and `vulkan.rs` compare `Settings::reconstruction_key` from `platform.rs`. The fixture varies backend, quality, preset and gameplay. |
| Keep FG admission and recording outcomes together for each Godot Vulkan pass. | `Vulkan::fg_pass` applies options and invokes the typed recording callback synchronously. The final-scene callback returns the current pass result, including missing presenter and device mismatch. Diagnostics receive that result separately. |
| Merge Vulkan extension names with one checked helper. | `runtime/common/src/vulkan_extensions.rs` supplies the shared Win32 pair and ordered merge to Godot negotiation and Unity Mono. The merge checks count overflow and allocation, preserves base order and compares non-null names by NUL-terminated content. |
| Move XeSS feature-chain compatibility and fallback into Runtime device negotiation. | `runtime/common/src/vulkan_negotiation.rs` owns the promoted-feature checks and the XeSS, Win32 interop and latency retry order. It takes a version-checked typed leaf table and the borrowed engine request, then returns the loader result and accepted feature receipt. The common implementation is compiled into the native adapter, which already links the Runtime. Godot's Vulkan leaves materialize and lend the actual structures. |
| Associate Vulkan feature readiness with the Godot device that acquired it. | `integrations/godot/native/vulkan/creation.rs` publishes readiness and adapter facts after a successful create, keyed by VkDevice. The primary swapchain reports its device under the graphics guard, SR samples the device in its pass, and destruction removes only the matching entry. The fixture covers failed and successful secondary attempts and destruction of the primary. |
| Drain the Godot Vulkan presenter once per shutdown attempt. | `vulkan.rs` disables and drains once before `vulkan/sr.rs` waits for device idle and closes the Runtime SR session. Reconfiguration still drains before retiring a stale live context. The stop fixture asserts one drain, idle and destroy in order. |
| Put Vulkan SR provider dispatch behind a single Runtime context. | `runtime/orchestrator/src/vulkan_sr_session.rs` uses the linked native and V01 split provider services through a typed Runtime table. It owns the provider handle, plan, preset, record, fallback and command retirement. Godot lends its packet, current recording, and optional graph handoff through one SR session. A cleanup-only owner stays reachable on refused open or close. |
| Keep diagnostic failures from skipping Godot's Vulkan spatial fallback. | Runtime returns the provider result, continuation and output result after its recording decision. Godot's dump and log delivery is best effort. The Godot fixture injects diagnostic failures and the Runtime fixture checks recovery on a split continuation and refusal when the continuation is null. |

## Native inventory

- `integrations/godot/native/vulkan_creation.h` was removed. Rust owns resolver dispatch.
- `integrations/godot/native/vulkan_control.h` contains declarations for the typed native
  callback table and Runtime entry points used by `runtime.cpp`.
- `integrations/godot/native/vulkan.cpp` was removed. `vulkan/native.rs` owns
  guarded loader startup, engine record translation and calls to Runtime presentation
  and SR. `vulkan/native/capture.rs` uses the Runtime Vulkan capture services and
  typed Vulkan dispatch. `runtime.cpp` supplies the build-dependent provider and capture
  function pointers from the existing NativeVulkan target.
- `integrations/godot/native/vulkan_creation.cpp` was removed in the follow-up.
  `vulkan/creation/typed.rs` owns the typed resolver entries and uses the size-checked
  table of linked Runtime services. `typed/chain.rs` owns the
  aligned, bounded feature-chain copies through each synchronous loader call.
- `tests/godot_vulkan_creation.cpp` was ported into Cargo fixtures in
  `vulkan/creation/typed/tests.rs`. They drive the production resolver with typed
  fake Vulkan entry points and Runtime services. They cover caller-owned feature
  links and values, allocator forwarding, aggregate and legacy feature roots,
  augmentation and retries, returned adapter facts and teardown order.
  The standalone native AMD Vulkan latency fixture still exercises its owner.
- `runtime/orchestrator/src/native_sr_vulkan_session.cpp` and its private helper header
  were removed. `vulkan_sr_session/linked.rs` supplies the same typed provider routes
  from the Godot archive, which already links NativeVulkan. The Runtime's session
  owner and injected test routes use the same declaration in `routes.rs`.
  Non-Vulkan Runtime targets retain their existing link dependencies.

## Source validation

The E01 handoff and E02 CPU, successful submission and exact Present receipt hooks
remain in the engine module and patch. The new SR wrapper calls the existing V01
split API with the existing caller-managed handoff ABI. A refused committed split
can recover on its returned continuation. A null continuation stops recording and
the provider result remains visible. The unchanged graph pass record carries source,
view and resource generation. The adapter's existing resource layouts, frame identity,
motion conventions and shader producers carry forward. Active GPU bindings, resize,
output recovery and native/provider-owned Present need device validation.

Source checks compared C/Rust callback fields, sizes and offsets and confirmed the engine
receipt files were unchanged. The changed Rust files passed formatting with rustfmt
1.8.0 from Rust 1.94.1. In the follow-up,
`tools/generate-godot-vulkan-bindings.py` regenerated the loader and Runtime records
with bindgen 0.72.1 from Vulkan header revision 335 in the pinned Godot checkout.
The generated offsets include the fields read and written in the feature-chain port.
The Windows window-class comparison now uses the GameSDK `ascii_wide` helper and
retains the six code units of `Engine`.

The final native cutover started at `82b39a63`. Source comparison matched all 17
Godot Vulkan exports and 20 private leaves. The engine module and patch carrying
successful-submit and exact Present receipts are unchanged. The native Present
entry still uses the same receipt-controlled capture gate, SDK bypass and call order.
Provider-owned Present still wraps the Runtime call with capture and returns the
actual result. Recycled command buffers reach capture retirement, then the same
Runtime SR retirement entry, under the graphics guard. The SR session implementation
and fixture bodies are unchanged. Bindgen 0.72.1 reproduced the extended private
bindings byte for byte from the pinned headers, and Cargo 1.94.1 resolved the locked
workspace offline.

Builds, fixtures, lint, device runs and visual acceptance remain user-owned.
