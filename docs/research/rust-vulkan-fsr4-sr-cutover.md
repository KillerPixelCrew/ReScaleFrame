# Vulkan FSR4 super-resolution Rust cutover

The baseline is `7dfa446`. The migration moves the split Vulkan provider owner
into `vulkan_fsr4_sr.rs`, behind the existing linked service and seven public C
exports. The shared split facade still admits both handoff versions and provides
the disabled implementation. The linked Godot adapter uses the same provider
with legacy ABI 1. The SDK bridge continues to run the existing native provider.

[The original interop record](godot-fsr4-interop-20261025.md),
[Godot graph handoff](godot-fsr4-graph-handoff-20261025.md), and
[tracked submission contract](vulkan-fsr4-sr-leases-20261115.md) describe the
baseline decisions and earlier device evidence. This cutover uses the same
producers, command recording and SDK dispatch.

## Admission, frame inputs and retirement

| Concern | Owner and evidence |
| --- | --- |
| Provider creation | Rust checks the setup and handoff through the shared split validator, converts the borrowed UTF-8 directory to UTF-16, and checks Vulkan dispatch and physical-device LUID before it creates the DX12 bridge. The unchanged bridge loads the signed provider, verifies the selected FSR4 version and owns the existing INT8 compatibility lease. The public info query still returns facts reported by that bridge. |
| Shared memory and fence | Rust reuses the shared DX12/Vulkan importer for the permanent D3D12 fence semaphore, dedicated image memory, device-local memory-type intersection, NT handles and COM references. All four shared images retain transfer source and destination usage. The output also retains Vulkan sampled and storage usage and the DX12 UAV flag. Partially imported resources stay with the creation transaction if checked cleanup refuses. |
| Source inputs and passes | The engine provides scene-linear RGBA16F color, normalized R32F depth and RG16F motion at the render rectangle. The service checks copy capability, Vulkan device and layouts, exact camera and frame rectangles, session, view, increasing frame ID and producer generation. It copies the input images and restores their shader-read layouts. The bridge receives jitter in render pixels, the supplied pixel motion scales, frame time in milliseconds, pre-exposure and the existing depth conventions. A new generation, explicit reset, first frame or prior SDK refusal resets temporal history. |
| Successful handoff | Rust records the native transfer and release commands into the actual producer buffer, then requests a distinct continuation. The inspected Godot source at `ed1daf0bf001b61586d9930840f2f1394092c079` with the RSF driver patch and module uses the engine's command-pool family and submits the chained sections on `main_queue`. The driver retains the existing binary waits and adds external timeline waits at `ALL_COMMANDS`. The producer waits for the preceding `done` and signals `ready`. The DX12 queue waits for `ready`, executes the SDK command list and signals `done`, and its private completion fence governs the three allocator slots. The host continuation waits for `done`, acquires the shared output before sharpening, copies it to the engine transfer destination and releases ownership. |
| SDK refusal and terminal split | The native bridge still queues the ordered `ready` to `done` handoff on SDK refusal. Rust returns the resumed buffer for the host's spatial fallback and skips copying a refused provider output. Failed or aliased continuations leave the provider terminal. ABI 2 accepts successful-submit notifications only for the exact producer then continuation. ABI 1 relies on its existing caller guarantee that each frame is submitted in order on the same host queue. |
| Checked retirement and captures | Rust refuses destruction with a pending tracked split. It waits up to 10 seconds for the latest DX12 `done`, idles Vulkan to cover the continuation, and calls the bridge's checked DX12 and SDK retirement. The shared capture helper still records producer color/depth/motion and uses the owned RGBA16F copy source for the exact output capture. Final flush precedes sharpening and import release. Once SDK teardown starts, the owner refuses plan/evaluate even when capture or SDK cleanup requires a retry. Checked bridge destruction follows import release and frees the remaining provider and queue state. |

The provider consumes the existing staged render inputs. The bridge's FidelityFX
dispatch and the shared Vulkan sharpening implementation still own their GPU
work. The cited integration evidence sets the scope for active shader
permutations, object or bone history, translucent and forward geometry,
particles, water, volumes and downstream composition. Device synchronization and moving-game
quality for the Rust owner remain open.

## Verification

The isolated Windows build used Visual Studio 18 2026, MSVC 19.52 and the
FidelityFX headers at `7539ae8b6568ad29ed78a753404351daf3b5cca7`. Godot linked with
godot-cpp `8070599885b6df675fcad033dd032ce44be2b850`. The optional corrected
Streamline package was absent. The fresh child worktree's yyjson download failed
the existing hash check. Copying the three pinned, hash-matching files from the
coordinator checkout into the ignored vendor directory unblocked configuration.

- `cargo test --workspace --locked` passed 204 tests. `cargo fmt --all -- --check`
  and `cargo clippy --workspace --all-targets --locked -- -D warnings` passed.
- Debug and Release each passed nine focused CPU contracts and compiled the C
  split header and Vulkan device fixture. The tests are `native_vulkan_dll_exports`,
  `godot_graph_handoff_contract`, `native_sr_fsr4_vulkan_contract`,
  `native_sr_fsr4_state_contract`, `native_vulkan_contract`,
  `native_sr_split_vulkan_contract`, `native_sr_split_vulkan_disabled`,
  `native_sr_split_vulkan_stub` and `native_dx12_vulkan_interop_contract`.
  The FSR4 state case links the Rust owner with CPU doubles. It checks image
  formats and usage, exact command buffers and timeline pairs, the fallback
  continuation, generation changes, checked destruction and partial construction.
  The device-capable executables ran only their no-argument CPU cases.
- With `RSF_FFX_INCLUDE=OFF`, the Release runtime and three focused split/export
  contracts built and passed.
- Release `dumpbin /exports` matched every name and ordinal in
  `build/rust-migration/baseline-exports` for Runtime (381), AC7 (256), Loader
  (273) and Minecraft (267). The FSR4-disabled Runtime also matched all 381.
  Godot has 398 named exports, including all seven linked split exports. The DLL
  fixture imports Runtime, while Godot and the FSR4 device fixture link the
  provider directly.
