# Rust DLSS Vulkan present capture owner

This cutover starts at `56338a5`. It moves the remaining lifecycle and submission owner of the
dedicated DLSS-G present capture from `native_fg_dlss_vulkan_capture.cpp` to
`dlss_vulkan_capture.rs`. The capture is a diagnostic readback of the real WSI image that
Streamline presents, enabled by `RSF_VULKAN_FRAME_CAPTURE_DIR`. It builds on the ledger moved in
`a3663dd` and the DLSS-G owner cutover in [rust-vulkan-dlss-fg-cutover.md](rust-vulkan-dlss-fg-cutover.md).
The callback contract is `runtime/vulkan/streamline/present-capture.h`, SHA-256
`5e6f502a08ced8c1527c552c00c9c0d30637d00e6fe599cd97f7dad8eee7a9f3`. It is unchanged, so the
corrected Streamline receipt hash for that file still applies.

No public export, export name, ordinal or public header changed. `rsf_core_dlss_capture_*` are
private symbols between the Rust static library and the native leaves.

## Ownership

| Boundary | Before | After |
| --- | --- | --- |
| Admission | C++ created the ledger, loaded the leaves and registered `State::callback`. | Rust publishes no owner while capture is disabled, registers its own callback last and closes the native leaves if registration refuses. It validates the packet size, ABI and every required pointer, then decides capture from the ledger, the boundary flag, frame selection and the unsubmitted state. |
| Command and fence lifetime | C++ map of pool, command buffer and fence per queue family. | Rust owns the per-family pool, buffer and fence handles. It waits a pending family's own fence before resetting its pool, and destroys fences and pools on close before the native leaves. |
| Submission | C++ ordered begin, record, end, submit, wait, flush and the waitless Present. | Rust keeps the same order and failure results. A submit failure returns the Vulkan result without a Present, and a fence wait failure returns it with a positive status mapped to `VK_ERROR_INITIALIZATION_FAILED`. Every capture failure before submission presents the original info once. An incomplete packet is refused without a Present, as before. |
| Retirement | C++ drained the ledger, flushed and cleared registration. | Rust drains the ledger under its lock and flushes only with completion proof. Detach clears the registration with the same device and user without taking the capture lock, and the owner stays alive until close. |
| Native leaves | | Typed Vulkan calls, the real image contract and enumeration, the copy submit behind the Present's original wait semaphores, the WSI lease, Streamline registration, `FrameCaptureVk` recording and flush, and the JSONL and error log formatting. |

The callback runs on the presenter thread inside the SDK's synchronous WSI path. The Rust owner
takes only its own lock there, as before. It never takes a session, PCL or host queue lock.

## Differences

- A failed or throwing real image enumeration now logs `real-images` and presents the original
  info. Before, an exception from the image vector reached the callback's catch and returned
  `VK_ERROR_INITIALIZATION_FAILED` without a Present.
- The wait-stage array for the copy submit is allocated after recording instead of before. If that
  allocation fails, the leaf returns `VK_ERROR_OUT_OF_HOST_MEMORY` and the owner treats it as a
  submit failure: it reports the result without a Present and stops recording if a readback was
  retained. Before, the same allocation failure returned `VK_ERROR_INITIALIZATION_FAILED` before
  recording. Neither path consumes the waits twice.
- Neither change was reproduced. They follow from moving allocation into leaves that must not
  throw across the Rust boundary. No reproducible defect was found in the existing owner, so
  none was fixed.

## Verification

MSVC 14.52 x64 with the Visual Studio 18 2026 generator and Windows SDK 10.0.26100.0 in the
worktree-local `build/present-capture`. Vendor include paths were read from the read-only
checkouts named in `D:/Coding/ReScaleFrame/build/rust-migration/CMakeCache.txt`. The private
`FETCHCONTENT_BASE_DIR` is in the worktree and MinHook uses the baseline source override. The
pinned yyjson files were copied into the worktree's ignored `vendor/` after their SHA-256 values
matched the lock file. The Vulkan `vulkan_core.h` SHA-256 is
`b8944f451db2231a07acbce90d1d6de2993fc6d656ab120cd833447ec9120710`.

No corrected Streamline receipt is available, so the Runtime DLL compiles the disabled capture
stubs and the enabled leaves are only built in `rsf_native_fg_dlss_vulkan_test`. The test now
drives the Rust owner through the real Vulkan, WSI and registration leaves with synthetic Vulkan
functions. Open, frame selection, readback recording and file output are synthetic. It checks the
registration and clearing identity, the transient pool for the packet queue family, the copy
submit with the original two waits at all-commands stages and its own fence, the 15 second fence
wait before flushing, and a native Present with the same chain, index and extension chain but no
waits. It also checks that an unmapped source and a chain without transfer-source usage present
the original info pointer once, and that an incomplete packet is refused without a Present.
Replacing the waitless Present with the original info made the test fail.

The Rust owner tests use a synthetic leaf table. They cover disabled admission, refused
registration, packet validation, the exact call order and handle reuse, a fence timeout that
blocks reuse and flushing until its own fence completes, an unsubmitted readback, a failed
recording, an unresolved real image, drained flush, refused and repeated detach, and ledger
collision reports.

- Built Debug and Release `rsf_orchestrator`, `rsf_proxy_ac7`, `rsf_proxy_dinput8`,
  `rsf_minecraft`, `rsf_native_fg_dlss_vulkan_test`, the three `rsf_native_fg_vulkan_dispatch*_test`
  targets, `rsf_streamline_vulkan_dispatch_test`, `rsf_native_dlss_vulkan_test`,
  `rsf_native_vulkan_test`, `rsf_native_xess_vulkan_test`, `rsf_native_vulkan_exports_test`,
  `rsf_fg_present_bridge_test` and `rsf_d3d11_present_bridge_test`. The bridge fixtures were built,
  not run.
- `ctest -C <config>` passed nine CPU-only cases in each configuration:
  `native_vulkan_dll_exports`, `native_fg_dlss_vulkan_contract`, `native_dlss_vulkan_contract`,
  `native_vulkan_contract`, `native_xess_vulkan_contract`, `streamline_vulkan_dispatch_contract`
  and the three `native_fg_vulkan_dispatch*` contracts.
- Compared `dumpbin /nologo /exports` Release name and ordinal pairs with a baseline built from
  `56338a5` in the same build tree. Runtime matched 381 pairs, AC7 256, Loader 273 and Minecraft
  267, with no differences.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and
  `cargo test --workspace --locked` passed. No `rustup` is installed, so the system Rust 1.100.0
  ran instead of the 1.94.1 pin in `rust-toolchain.toml`.

Actual Streamline execution, GPU completion, captured pixels and game
rendering were outside the worker validation.

## Integrated Streamline build

After the merge, the coordinator ran the repository's
`runtime/vulkan/streamline/build-sdk.ps1` recipe on source
`2122257e0fce486f91b385aa63b9a09b0a34b363`. The generated Production
receipt matched the checked Present capture header and the rebuilt
`sl.interposer.dll` and `sl.common.dll` hashes. The build log and
receipt are under `.local/streamline-cutover/`. The separate
`build/rust-migration-streamline` project enabled the production
Vulkan Streamline leaves with that receipt.

MSVC Release built the enabled Runtime DLL and its AC7, Loader and
Minecraft consumers at merge `1be5c74`. The Runtime DLL loaded in
`native_vulkan_dll_exports`. The synthetic
`streamline_vulkan_dispatch_contract` and
`native_fg_dlss_vulkan_contract` also passed. Export names and
ordinals matched all 381, 256, 273 and 267 recorded pairs, respectively.
The integrated branch passed 305 locked Rust tests, formatting and
Clippy. The ABI inventories and CTest transcript are under
`build/rust-migration-streamline/evidence/`. Actual Vulkan GPU
execution and captured image validation remain open.

## Godot capture preflight

`eng/build-godot.ps1 -StageOnly -StageRuntime -NativeBuildDirectory
build/rust-migration-streamline` stages the tested extension from the
same build used for the ABI checks. The staged DLL matched that build,
and all 37 runtime files matched the package manifest. The NVIDIA
archive matched the pinned SHA-512. The corrected Vulkan runtime
matched its separate build receipt.

The staged extension registered `ReScaleFrameEffect` in the pinned
Godot 4.7.2 engine in a headless script run. The earlier local editor
binary crashed during headless import with both the staged extension
and an earlier native DLL. Its no-addon control exited normally. The
exact fixture also reproduced the import crash without the optional
editor plugin and without a fixed iteration limit. A debugger run
exceeded the inspection interval without yielding a stack. The engine
and DLL identities, package and differential logs are under
`.local/godot/preflight/`. These checks cover staging and native class
registration. Real Vulkan WSI output still requires the windowed run.
