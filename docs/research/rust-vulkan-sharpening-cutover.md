# Vulkan sharpening Rust cutover

The baseline is `56338a5`. The first-party Vulkan sharpening pass in
`runtime/orchestrator/src/sharpening_vulkan.h` was a header-only C++ class that
owned its Vulkan device binding, pipelines, per-recording slots, admission and
failure logging. This cutover moves that ownership into
`runtime/orchestrator/src/vulkan_sharpening.rs`. The header keeps the
`rsf::sharpening::Vulkan` facade with the same `run`, `execute`, `retire`,
`clear` and `dispatches` members, so no consumer changed.

## Consumers

| Consumer | Calls | Owner of retirement |
| --- | --- | --- |
| `native_dlss_vulkan.cpp` (Streamline DLSS SR, `vulkan_dlss_sr.rs`) | `run`, `clear` on release, `retire` with the capture retirement | host through `rsf_srvk_retire_commands` |
| `native_sr_vulkan.cpp` (FFX SR, `vulkan_sr.rs`) | `execute`; the Rust owner logs the first failure | same |
| `native_xess_vulkan.cpp` (XeSS SR, `vulkan_sr/xess.rs`) | `execute` on the private output; the Rust owner logs | same |
| `native_sr_fsr4_vulkan.cpp` (FSR4 split continuation, `vulkan_fsr4_sr.rs`) | `run` in the continuation buffer, `clear` after Vulkan idle | host through `rsf_fsr4vk_retire_commands` |
| `tests/sharpening_vulkan.cpp` | device fixture, built only | fixture |

Every consumer clears only after its own drain (device idle or the ABI's drain
precondition) and retires a command buffer only after GPU completion and reset.
The cutover keeps both contracts.

## Ownership

| Concern | Owner |
| --- | --- |
| Admission | Rust. Off, Vendor, unknown modes and zero strength return success without device work. Then it requires commands, output and a frame record; motion, depth and exposure must be on the setup device, in `SHADER_READ_ONLY_OPTIMAL` and nonzero; exposure must be 1x1 R32F or R16F. These are the baseline tolerances. Sanitizing and the active test call the existing `options.h` helpers through a leaf, so D3D11, D3D12 and Vulkan keep one sanitize policy. |
| Effective filter | Rust. Depth RCAS and depth DAS fall back to RCAS without depth. Absent inputs bind the output view, as before. |
| Device and pipelines | Rust binds one device on first use and latches a failed or foreign bind until `clear`. It creates the two layouts and the RCAS, depth RCAS and depth DAS pipelines in that order and releases them in the baseline order. |
| Slots | Rust keeps at most 64 in-flight recordings, each with a descriptor pool, set, host-coherent uniform block and RGBA16F scratch image. A slot is reused only after `retire` names its command buffer. |
| Scratch images | The shared `ensure_image` and `rsf_vkfg_platform` leaves (`vulkan_fg_resources.rs`), with storage and transfer-source usage and the first device-local memory type, matching the baseline choice. |
| Failure | Rust reports `Vulkan sharpening unavailable; retaining vendor output` once per owner for `run`; `execute` callers keep their own report. Every refusal happens before any command is recorded. |
| Native leaves | `sharpening_vulkan.h` resolves the same 31 Vulkan functions, creates layouts, shader modules from the exact `rcas_spirv`, `da_rcas_spirv` and `da_das_spirv` arrays with entry `CSMain`, descriptor pools, uniform blocks and views (D32 depth uses the depth aspect), packs the 128-byte constants with `options.h`, and records the descriptor update, six barriers, dispatch and copy unchanged. |

`sharpening_vulkan_core.h` is the private C contract between the two sides.
`vulkan_sharpening_abi.rs` mirrors it by hand; both sides assert sizes and
offsets. The `rsf_core_sharpenvk_*` functions are not DLL exports.

The constant mapping, MAS ramp and shaders remain GPL-3.0 code in
`runtime/graphics/src/sharpening` and stay on the native side. The Rust owner
is first-party MIT code and does not port them.

## Defects

- **Zero output extent.** The baseline accepted a frame record with a zero
  output width or height, created a zero-extent scratch image and recorded a
  zero-size dispatch and copy. Vulkan forbids a zero image extent
  ([VUID-VkImageCreateInfo-extent-00944](https://docs.vulkan.org/spec/latest/chapters/resources.html#VUID-VkImageCreateInfo-extent-00944)).
  The CPU fixture compiled against the baseline header reproduced it: the call
  returned success after `vkCreateImage` received 33x0 and the dispatch was
  3x0. Rust now refuses the frame before binding the device.
- **Missing dispatch functions.** The baseline called `instance_proc` and
  `device_proc` without checking them. The leaf now refuses a null setup
  device or dispatch function. This came from source review; the fixture case
  is excluded from the baseline build because the baseline would crash.

## Deliberate differences

- A refusal after a free slot was chosen (a view failure) releases the views
  at once and leaves the slot free. The baseline kept the slot reserved until
  the host retired that command buffer. No command was recorded in either case.
- A resized scratch image is created before the old one is released, through
  the shared image leaf. The baseline released first. Only free slots resize.
- Allocation failure of the Rust owner or slot list refuses the pass instead of
  relying on a caught `std::bad_alloc`. If the owner itself cannot be
  allocated, even Off returns failure.

## Verification

Windows x64, Visual Studio 18 2026, MSVC 19.52 and Rust 1.100.0 (the repository
pins 1.94.1; no rustup pin was applied). The worktree-local build used the main
checkout's vendor and reference headers read only, the pinned MinHook source,
and the three pinned yyjson files copied from the main checkout. The Streamline
receipt at `.local/streamline-cutover/artifacts/streamline-2122257-current/runtime`
enabled the DLSS consumer in the Runtime.

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked
  -- -D warnings` and `cargo test --workspace --locked` (301 tests) passed.
  Eight new unit tests drive the owner through fake leaves: disabled modes,
  admission, effective filter and constants facts, slot reuse, the 64-slot
  bound, one failure report, latched binding, refused preparation and views,
  and clear ordering with a leak check.
- New `sharpening_vulkan_cpu` links the production facade, leaves and Rust
  owner against fake Vulkan entry points. It checks the exact SPIR-V pointers
  and sizes in mode order, `CSMain`, memory types, scratch format and usage,
  view formats and aspects, all six descriptor writes, the barrier layout
  sequence, dispatch groups and pipeline, copy extent, packed constants,
  depth fallback, in-flight slots, retirement, the slot bound, logging,
  failure latching and release, and zero leaked handles after `clear`.
  Compiled against the baseline header, every case except the zero-extent
  case passed, which shows parity for the rest.
- Release and Debug passed `sharpening_vulkan_cpu`, `native_vulkan_dll_exports`,
  `native_vulkan_contract`, `native_xess_vulkan_contract`,
  `native_xess_input_range_contract`, `native_sr_fsr4_vulkan_contract`,
  `native_sr_fsr4_state_contract`, `native_dlss_vulkan_contract`,
  `native_ffx_vulkan_reset_contract`, `native_ffx_vulkan_submission_contract`,
  `native_sr_split_vulkan_contract`, `native_sr_split_vulkan_disabled`,
  `native_sr_split_vulkan_stub` and `sharpening_settings`. Release also passed
  `sharpening` (D3D11 WARP). The `sharpening_vulkan` device fixture was built
  in both configurations and not run.
- Release `dumpbin /exports` matched every name and ordinal in
  `build/rust-migration/baseline-exports` for Runtime (381), AC7 (256), Loader
  (273) and Minecraft (267). No `rsf_core_sharpenvk_*` name is exported.
- A stub Release configuration without Vulkan, FFX Vulkan or XeSS Vulkan
  headers built the Runtime and passed `native_vulkan_dll_exports`,
  `native_vulkan_contract`, `native_xess_vulkan_contract` and the three split
  contracts. Its Runtime has 354 names: the baseline set minus the 27 XeSS
  frame-generation Vulkan bridge exports, which CMake builds only with those
  headers.

## Remaining limits

Physical GPU synchronization, image quality and moving-game behaviour
of the Rust owner remain unverified. The Godot extension links the native
Vulkan services statically. The worker
configuration lacked the pinned Godot C++ bindings. The full
`eng/verify.ps1` suite was outside this validation.

## Integrated checks

The Streamline-enabled Release build at `41b6b12d35ea` passed eight
focused Vulkan CPU contracts, including sharpening admission, DLSS
present capture and both linkage forms of the shared readback test.
The Runtime, AC7, Loader and Minecraft DLLs matched all 381, 256, 273
and 267 recorded export names and ordinals. All 330 locked Rust
tests, formatting and Clippy passed. The logs and inventories are in
`build/rust-migration-streamline/evidence/41b6b12d35ea/`.

The same build linked the Godot native extension with the pinned
godot-cpp revision `8070599885b6df675fcad033dd032ce44be2b850`.
Its 398 export names and ordinals matched the archived XeSS SR
inventory, and its import table showed static Runtime linkage.
The Godot graph handoff and Vulkan creation CPU contracts passed.

## Offscreen device acceptance

The Release `sharpening_vulkan` fixture passed at `9367f1306570`
on an RTX 4070 Laptop GPU. It ran 30 dispatches with explicit
command-buffer retirement, covered three concurrent uniform and
descriptor slots, and checked the RCAS, Depth RCAS and Depth DAS
output. It also verified that the depth modes kept the sharpening
response off the opposite side of a depth edge. The CTest log and
hashes are in `build/rust-migration-streamline/evidence/9367f1306570/`.
