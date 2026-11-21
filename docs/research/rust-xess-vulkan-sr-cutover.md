# XeSS Vulkan super-resolution Rust cutover

The baseline is `7dfa446`. This source port moves the XeSS Vulkan SR owner from `native_xess_vulkan.cpp` into the Rust runtime as `vulkan_sr/xess.rs`. It preserves the four public `rsf_xessvk_*` requirement exports, SRVK ABI 2 and the seven `rsf_srvk_*` exports. The question was whether the C++ owner's policy could move without changing public routes, linkage, SDK calls or Vulkan recording. The approach follows the [Vulkan SR cutover](rust-vulkan-sr-cutover.md): Rust holds the handle and decisions, and a private native table performs Windows loader, XeSS SDK and Vulkan calls.

## Ownership and execution

| Boundary | Owner |
| --- | --- |
| Public entry points | `native_xess_vulkan.cpp` keeps the four `RSF_RUNTIME_API` requirement exports and forwards them to Rust with the private leaf table, or with a null table when XeSS Vulkan headers are absent. `rsf_srvk_*` reaches XeSS through the existing shared owner. Its platform table now carries a pointer to the XeSS table instead of a C++ legacy provider, so the private table shrinks from 320 to 280 bytes. Both sides assert the layout. |
| SDK session | Rust admits the setup, refuses a second load, holds the loaded SDK in an `Arc` and refuses shutdown while any context holds a reference. Requirement calls run under the session lock. Rust maps XeSS results: unsupported device, driver, feature or unimplemented calls return `NOT_SUPPORTED`. Warnings and other failures return `INIT_FAILED`, as before. |
| Context lifetime and sizing | Rust creates the context, maps quality 0 to 5 to the SDK enum values, sets automatic exposure, inverted-depth and jittered-motion flags, and checks the recommended render size against the SDK's nonzero inclusive range. It binds the device leaf, records the packed SDK version for capture metadata and allocates three input caches at the SDK maximum and the private output at output size. Refused creation runs the cleanup leaves for any partial context and image objects. |
| Frame execution | Shared admission (`frame_admit`) runs first. Rust then logs the input range once and checks the actual render size against the retained range, jitter within half a pixel, a finite reciprocal pre-exposure, R32F depth, transfer-source guides and distinct images. It sets velocity scale, stages the leased guides into the cache and initializes the private output once. History resets on a reset flag, frame gap, session, view, generation or active-extent change. After accepted execution it records captures in the previous order, runs sharpening, copies the private output to the caller's image and commits history. |
| Teardown | Destruction flushes captures and destroys the SDK context. A refused SDK destruction keeps the owner for retry. Successful destruction clears sharpening, releases views, images and memory, deletes the native state and then drops the SDK reference. |
| Native work | The leaves load `libxess.dll` from the absolute runtime directory and pin it, resolve SDK and Vulkan entry points, call the SDK, allocate device-local images and views, and record the same barriers and copies as before. The existing Vulkan sharpening pass and its SPIR-V stay behind the leaf. |

## Deliberate differences

- A refused `xessVKExecute` now clears the whole history record. The old code cleared only the frame ID, so a following frame numbered 1 in the same session, view and generation missed its reset. The shared FFX owner already cleared the whole record.
- Captures now use the command-retirement completion that the FFX owner adopted. Each readback is tagged with its command buffer and a `VkEvent` recorded after its copy. `rsf_srvk_retire_commands` releases only that buffer's readbacks and maps only executed copies. Godot and the device fixture already retire after their fence wait and reset. The previous shared-submission check used a device-wide readiness test.
- The sharpening failure message is logged once per context by Rust instead of inside the C++ pass. The text and the fallback to `OutputDebugStringA` are unchanged.
- The private XeSS core entry points (`rsf_core_xessvk_*`) reuse the shared SRVK setup and frame admission. They are not DLL exports.

## Render inputs and downstream consumers

Source review used the [Godot XeSS Vulkan research](godot-xess-vulkan-20261025.md), the [input range note](vulkan-xess-input-range-20261115.md) and the pinned XeSS headers (`vendor/xess/inc`, package `v3.0.2`, SR 2.0.2.0). No engine, shader or producer code changed.

| Input or pass | Producer and existing binding | Cutover |
| --- | --- | --- |
| Scene colour | Godot Forward+ internal RGBA16F colour in shader-read layout, render size inside the SDK range. | Same transfer copy into the maximum-size RGBA16F cache and the same SDK subrectangle. |
| Depth | Godot `resolve_motion` R32F normalized depth. | Same R32F requirement, copy and inverted-depth flag. Shared admission still validates near, far and FOV. |
| Motion | Dense RG16F motion. The caller supplies motion scale. | Same velocity-scale call before staging. Jitter is in input pixels and limited to half a pixel. |
| Exposure | XeSS automatic exposure. The caller supplies pre-exposure. | Same flag and reciprocal exposure scale. |
| Reactive, composition and bias | Optional R8 masks from the SRVK frame. | Still not consumed by XeSS. Shared admission validates them when present. |
| Sharpening | The existing Vulkan compute pass with optional motion, depth and exposure. | Same pass, descriptor and scratch retirement through `rsf_srvk_retire_commands`. |
| Reinsertion and later rendering | Private RGBA16F output in GENERAL, copied into the caller's transfer-destination image after accepted execution. A refused execution leaves the caller's spatial recovery. Later passes, FG and Present use the completed image through their existing owners. | Same barriers, copy extent and output ownership. |

The earlier recorded transparent, particle, skinned and occlusion-heavy coverage gaps and render-path limits of the Godot integration still apply.

## Verification

- The worktree build used `Visual Studio 18 2026` x64 with the staged headers from the main checkout (`vendor/xess/inc`, `vendor/fidelityfx`, `references/godot`). Its hash-matched yyjson files were copied in, as in the earlier cutover. Debug built the Runtime DLL, the Vulkan archive and the SRVK, split and XeSS fixtures. Release also built the AC7 carrier, the shared loader, Minecraft, `rsf_input_dump_vulkan` and the Godot native DLL with `references/godot/godot-cpp`.
- Debug and Release each passed seven CPU cases: `native_vulkan_contract`, `native_vulkan_dll_exports`, `native_sr_split_vulkan_contract`, `native_sr_split_vulkan_disabled`, `native_sr_split_vulkan_stub`, `native_xess_vulkan_contract` and `native_xess_input_range_contract`. The rewritten input range contract drives the real native leaves and the Rust owner with inert SDK and Vulkan entry points. It covers session refusal while a context is live, cache sizing and usage, active-extent and frame-gap resets, all range and image refusals, and image, view, memory and context teardown.
- A Release build with empty `RSF_XESS_VK_INCLUDE` and `RSF_FFX_VK_INCLUDE` passed `native_vulkan_contract`, `native_vulkan_dll_exports` and `native_xess_vulkan_contract`. The input range contract is not defined without XeSS headers. That Runtime keeps the four `rsf_xessvk_*` and seven `rsf_srvk_*` exports and has 354 names. The 27 absent names are the optional `RsfXessBridge*` and `VulkanStoryXessFg*` frame-generation exports, which the unchanged CMake gates on the same XeSS header path.
- Release `dumpbin /exports` matched every name and ordinal in `build/rust-migration/baseline-exports` for Runtime (381), AC7 (256), Loader (273) and Minecraft (267). Godot has 398 names, identical to the earlier Vulkan SR inventory, including the four XeSS requirement exports and the seven SRVK exports. `dumpbin /dependents` confirmed static linkage for Godot and the SRVK and XeSS fixtures, and the Runtime import on the split DLL fixture.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` passed. The orchestrator has 133 passing tests, four of them new. They cover session admission and result mapping, cache sizing and history, refusal unwinding and routing through the shared SRVK owner.
- Device fixtures were built only: `rsf_native_xess_vulkan_test`, `rsf_native_vulkan_test` and `rsf_input_dump_vulkan`. No device test, game or visible window ran.

Validation covers source, builds, ABI and CPU fixtures. GPU validation and current game image acceptance remain open.

The coordinator merge routes XeSS image allocation and partial cleanup
through the same Rust helper as DLSS SR and Vulkan FG. The real-leaf input
range fixture still verifies the XeSS-specific image usage and sizes. The
synthetic partial-allocation test verifies release of the unpublished image
and memory before context and earlier image retirement.

## Transfer recording in the Rust owner

Compared `transition`, `copy_region`, `stage`, `prepare_output` and `copy_output`
in `native_xess_vulkan.cpp` at `8f5d0b4944ff9573eb5f5af6bf2eb0837409c` with
`vulkan_sr/xess.rs` and the shared `vulkan_fg_copy` Rust and native recorders.
The guide transfers match the shared shader-read copy sequence used by
[Vulkan FG](rust-vulkan-dlss-fg-cutover.md#guide-and-hud-less-staging-commands).
The XeSS owner uses the same records for its output, where the layouts and
barrier order differ from guide staging.

| Position in evaluation | Recording and state |
| --- | --- |
| After velocity scale succeeds | For each guide in colour, depth, motion order, transition the caller's image from `SHADER_READ_ONLY_OPTIMAL` to `TRANSFER_SRC_OPTIMAL`. Transition the cache image from `UNDEFINED` on first use or `SHADER_READ_ONLY_OPTIMAL` on reuse to `TRANSFER_DST_OPTIMAL`. Copy the leased guide's width and height, restore the source to `SHADER_READ_ONLY_OPTIMAL`, then restore the cache image to `SHADER_READ_ONLY_OPTIMAL`. Mark that cache image initialized after its restore. The SDK still sees the maximum-size cache views with the active input extent passed separately. |
| Before the typed XeSS execute call | Transition the private output from `UNDEFINED` to `GENERAL` on first use and record `GENERAL` to `GENERAL` on reuse. Mark the allocation initialized after this barrier, including when the subsequent SDK call refuses. A refused execute clears history and returns before capture, sharpening or copy-back. The host's existing submit-in-order or retire contract still applies to that recording. |
| After accepted execute | Record the SR parameters and leased colour, depth and motion captures in their existing order. Run sharpening and record the private output capture in `GENERAL`. Transition only the private output from `GENERAL` to `TRANSFER_SRC_OPTIMAL`, copy the record's output width and height to the caller's existing `TRANSFER_DST_OPTIMAL` image, then restore the private output to `GENERAL`. Commit active extent and history after that restore. |

Every transfer barrier still uses `ALL_COMMANDS` for both stages and
`MEMORY_READ | MEMORY_WRITE` for both access masks, with source access zero
from `UNDEFINED`. Both queue families remain `VK_QUEUE_FAMILY_IGNORED` and
each command stays on the supplied command buffer, preserving graphics queue
ownership without a queue submission. The typed leaves still emit one colour
image barrier with zero dependency flags and no other barriers in that call.
Barriers and copies use mip 0 and layer 0 with one level and one layer. Each
copy has zero offsets and depth 1. FFX SR's narrower access masks and the
Vulkan/D3D12 external queue transfers use their own recorders.

The private `Ops` table replaces `stage` and `prepare_output` with typed
`barrier` and `copy` leaves and removes `copy_output`. It shrinks from 248 to
240 bytes on x64. Both declarations assert the size and the offsets for
`barrier` (152), `copy` (160), `execute` (168) and `sharpen` (176). The shared
barrier and copy records remain 40 and 32 bytes. The public requirement ABI 1
and SRVK ABI 2 structures and exports keep their signatures. The initial
`libxess.dll` resolver, module pin and typed SDK calls follow the existing code
and package pin.

Source review traced the disabled configurations through the unchanged CMake
guards and `native_xess_vulkan.h`. Without XeSS headers, the four requirement
exports still pass a null XeSS table. Initialize and the requirement queries
retain argument validation and `NOT_COMPILED`, and shutdown still succeeds.
SRVK with FFX Vulkan headers uses its shared table with a null XeSS member.
Without FFX Vulkan headers it passes a null shared table. Both create paths
clear the output handle and validate setup before returning `NOT_COMPILED` for
XeSS. Neither disabled path reaches Vulkan recording or the SDK loader.

The existing Rust owner fixture now records primitive barriers and copies
instead of the former compound leaves. The native range fixture uses the
production table and mocks typed Vulkan calls. Its barrier mock discards the
fields and its copy mock checks extents and count, so masks, queue families,
subresources and layout parity were reviewed in source. Rust 1.94.1 formatting,
locked offline Cargo metadata and Git whitespace checks passed. Enabled and
disabled compilation and linking, binary export checks, Clippy, fixture
execution, GPU execution and capture, and game acceptance remain unverified
for this change. The producer and shader coverage limits above still apply.

## Required SDK exports in the Rust owner

The transfer port at `af5e2b51` left the required export list in
`native_xess_vulkan.cpp`. The initialization owner in `vulkan_sr/xess.rs` now
checks that list through the existing `SdkModule::symbol` lookup on its
`libxess.dll` handle. The native leaf still allocates the capsule before the
first lookup and returns `INIT_FAILED` if that allocation fails. It uses XeSS
header types to bind the complete address array after Rust accepts it.

Source comparison matched all ten lookups, in order, against the original
`RSF_XESS_LOAD` calls and their corresponding capsule members.

1. `xessVKGetRequiredInstanceExtensions`
2. `xessVKGetRequiredDeviceExtensions`
3. `xessVKGetRequiredDeviceFeatures`
4. `xessVKCreateContext`
5. `xessVKInit`
6. `xessVKExecute`
7. `xessDestroyContext`
8. `xessGetOptimalInputResolution`
9. `xessSetVelocityScale`
10. `xessGetVersion`

The first absent export yields `MISSING_ENTRY_POINT` and drops the capsule
before releasing the module reference. A complete table reaches the existing
module pin, whose refusal yields `LOAD_FAILED` through the same cleanup. Only
a successful pin publishes the session. The session lock and context-held
references still govern requirements, live-context shutdown refusal and final
cleanup. The private `Ops` table grows from 240 to 256 bytes on x64 for its
typed binder and fixture lookup override. Both declarations assert the address
array size and the binder, lookup and rendering leaf offsets.

The source comparison also checked the public requirement ABI 1 and SRVK ABI 2
paths, including setup validation and `NOT_COMPILED` with the disabled XeSS
table. The context flags, SDK calls, frame and image handling, transfer
recording and SDK package pin match the transfer port. The existing native
range fixture now drives the typed binder with inert exports and checks
refusal at the fourth symbol before pinning, then retries all ten in order.
The Rust owner fixture supplies its existing fake handles through the private
lookup slot. Rust 1.94.1 formatting, locked offline Windows Cargo metadata and
Git whitespace checks passed. Compile and link, Clippy, fixtures, device and
game verification remain pending for this port.
