# Rust DLSS Vulkan frame-generation owner

This cutover starts at `5accc0e`. It ports the existing Vulkan DLSS-G owner behind
`rsf_dlssgvk_*` and keeps the public FGVK and DLSSGVK ABI at 1. The reference is
the frame path and queue ordering already documented in
[Godot Vulkan DLSS FG](godot-vulkan-dlss-fg.md),
[explicit Vulkan FG sources](vulkan-fg-present-source-20261115.md), and
[the later capture evidence](vulkan-dlss-focus-capture-20261115.md). The public
SDK header contract for `eBlockNoClientQueues` requires a per-Present input
completion fence and value obtained on the presenting thread. The reference
source checkout is `2122257e0fce486f91b385aa63b9a09b0a34b363`.

## Ownership and completion

| Boundary | Cutover |
| --- | --- |
| Admission and proxy lifetime | Rust claims the single DLSS-G viewport, checks the SDK support sample and dimensions, and only publishes an owner after the native proxy, image enumeration and optional capture registration succeed. Native checks the Vulkan descriptor, looks up the Streamline feature functions and Vulkan entry points, and holds the checked device, allocator and swapchain handle. It adds WSI transfer-destination usage and preserves game-facing transfer-source layout. |
| Guides and HUD-less source | Rust validates the camera, record, dimensions, image identity, format and motion scale. It uses the shared six-slot frame ledger, private images and two fences per slot. Native encodes constants and typed Streamline tags. Depth is R32F and motion is RG16F at render size. The HUD-less image is RGBA8 at output size. The native constants convert the supplied motion scale from pixels using the render dimensions. The original worker callback resolves the source's session and frame token. A failed tag after a copy retains the recorded work and its resources. |
| Exact presentation and queue provenance | Rust decides admission using both recording submissions and the frame ledger's monotonic local sequence. Explicit Present uses the session, frame, view and resource generation from the accepted identity prefix. Incomplete, duplicate and expired sources publish Off before ordinary WSI. The proxy Present uses `queues[0]` without an outer broker lock. The native resource leaf submits completion fences using the host's exact `graphics_submit` and `queue_user`. The dedicated present capture retains the packet's actual queue, queue family, submit and WSI functions. |
| Resource reuse | Rust requires the shared source/consumer fence proof and the per-slot Streamline timeline lease before reusing private images. A zero timeline value is valid. A missing query, missing semaphore or failed On Present leaves the affected slot unavailable. |
| Retirement | Rust refuses early if a list still contains unsubmitted copies. It publishes Off with SDK resources retained, checks device idle under the host queue broker and verifies the Streamline worker timeout epoch. It retires borrowed SDK timeline leases before flushing captures, detaches the capture callback before freeing the SDK feature, and destroys the chain, private images and fences after the checks succeed. |

### Corrections

- The previous owner only retained the SDK lease after a successful WSI result and could continue
  to report generation enabled when Present itself had failed. A proxy error can follow work on
  the SDK/presenter side. The dedicated capture callback, for example, can return a fence wait
  error after submission. A later sample alone cannot attach a completion value to the failed
  input. The owner now reports generation inactive and refuses reuse and teardown when that
  completion is unprovable. It can also report an accepted Present as inactive when the query or
  following consumer fence provides no proof. The Vulkan return value is preserved.
- The previous input capture called the legacy flush after idle with no link from each readback
  to its recording. The shared frame ledger can discard an abandoned recording when the host
  submits a newer recording at the same command-buffer address. Idle cannot prove that the
  abandoned copy executed. The owner now groups guide and HUD-less readbacks by local recording
  sequence and pass. It discards the corresponding group when the frame ledger discards it and
  only completes submitted groups after the owner has observed their fence or drained the
  exact host queue. The existing dedicated presenter ledger still uses its own fence.
- Previously the SDK feature could be freed before a refusing capture unregister. The new order
  preserves the SDK feature, swapchain and private inputs if unregister refuses. A successful
  detach keeps the capture state until owner destruction. If SDK release then refuses, the
  capture callback stays detached and checked teardown can be retried. Native cleanup also
  distinguishes a successful but unpublished proxy from an output handle written on create
  failure.

The existing host ABI reports successful submission by command-buffer handle. The shared ledger
resolves reuse from the newest observed recording. Vulkan list resets or submissions outside
those callbacks cannot establish source identity through this ABI. This migration does not change
the host submission contract or the accepted input formats and shader producers.

## Verification

MSVC 19.52 x64, Visual Studio 18 2026 and Windows SDK 10.0.26100.0. The build uses the
vendor include and feature options from `D:/Coding/ReScaleFrame/build/rust-migration/CMakeCache.txt`
with a private `FETCHCONTENT_BASE_DIR` and the baseline MinHook source override.
The local Streamline 2.14.1 `sl_dlss_g.h` SHA-256 is
`1fc18cbe004e280df1f787276d08a1b28b8a8c4c65856fbaa659f56dff6a915d`.
The Vulkan `vulkan_core.h` SHA-256 is
`b8944f451db2231a07acbce90d1d6de2993fc6d656ab120cd833447ec9120710`.

- Built Debug and Release with
  `cmake --build build/dlss-fg-cutover --config <config> --target <targets> --parallel 8`.
  Targets were `rsf_orchestrator`, `rsf_proxy_ac7`, `rsf_proxy_dinput8`, `rsf_minecraft`,
  `rsf_native_fg_dlss_vulkan_test`, the three `rsf_native_fg_vulkan_dispatch*_test` targets,
  `rsf_streamline_vulkan_dispatch_test`, `rsf_native_dlss_vulkan_test`,
  `rsf_native_vulkan_test`, `rsf_native_xess_vulkan_test`, `rsf_native_vulkan_exports_test`,
  `rsf_d3d11_present_bridge_test` and `rsf_fg_present_bridge_test`. Device variants were built.
- The corrected Streamline binary receipt was unavailable. The enabled DLSS-G fixture compiles
  the Streamline and Vulkan leaves against the local headers and supplies synthetic SDK and
  Vulkan callbacks. It tests the original token, typed tags, image copies, exact callback queue,
  duplicate and partial submission, expired token, refused options and teardown, and retry with
  the captured semaphore and value. `ctest --test-dir build/dlss-fg-cutover -C <config> -R
  <names> --output-on-failure` passed nine CPU-only cases in each configuration. The cases
  were `native_vulkan_dll_exports`, `native_fg_dlss_vulkan_contract`,
  `native_dlss_vulkan_contract`, `native_vulkan_contract`, `native_xess_vulkan_contract`,
  `streamline_vulkan_dispatch_contract` and the three `native_fg_vulkan_dispatch*` contracts.
- Loaded both optional stub Runtime DLLs and called the public create and teardown entry points.
  Valid setup returned `RSF_BACKEND_ERROR_NOT_COMPILED` and cleared both outputs. Bad ABI and
  short size returned their documented errors, and null drain and destroy were successful.
- Compared `dumpbin /nologo /exports` Release name and ordinal pairs against
  `D:/Coding/ReScaleFrame/build/rust-migration/baseline-exports`. Runtime matched 381 pairs,
  AC7 matched 256, Loader matched 273 and Minecraft matched 267.
- `cargo test -p rsf-orchestrator --locked vulkan_dlss_fg -- --nocapture` passed the synthetic
  owner test. It checks abandoned recording capture groups, exact submitted groups, SDK
  release and detach refusal, cleanup on retry, and quarantine after a failed On Present.
  `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`
  and `cargo test --workspace --locked` passed.

Logs and the stub/export comparison script are in the worker's ignored
`build/dlss-fg-cutover` directory. Runtime loading with a corrected receipt, actual
NVIDIA SDK execution, GPU completion, captured pixels and game rendering remain
unverified for this build. No device, visible-window, game or full verification
suite was run.

## Proxy creation ownership

The first Rust port still left the Vulkan proxy handle, image count,
transfer-usage selection and creation sequence in the native wrapper.
The Rust owner now validates the marshalled swapchain description, holds
the only proxy handle and gives its enumerated image count to Streamline.
It preserves caller usage, requires transfer destination support and
requires transfer source support when capture is enabled. It also adds
transfer source usage when supported with capture off. The typed native
calls still use the host's device, allocator, queue and Streamline dispatch.
Proxy create and destroy stay outside the host's WSI broker because
Streamline takes its own swapchain lock there. Frame tagging, callback
leases and retirement checks follow the existing path above.

The synthetic fixture reproduced a zero-image proxy on the previous
implementation. When enumeration returned success with a zero count,
the public creation call accepted the chain. The Rust owner now refuses
that count and destroys the unpublished proxy once. The enabled fixture
also verified a foreign Vulkan struct tag, transfer capability refusal,
capture registration failure and rollback, and successful creation and
teardown on retry.

The MSVC Release tree built the Runtime with the corrected Streamline
receipt for `2122257e0fce486f91b385aa63b9a09b0a34b363` and passed nine
Vulkan CPU and ABI contracts. Runtime, AC7, Loader and Minecraft matched
all baseline export names and ordinals. The Streamline-disabled Release
DLL passed the export contract and the direct stub probe, which verified
that create clears both outputs and invokes no host callbacks.
All 413 locked Rust tests, formatting and Clippy passed on Rust 1.94.1.
The enabled fixture uses synthetic Vulkan and SDK entry points. Hardware
activation, real-scene tagging, visual quality and pacing remain open
for this change.

## Guide and HUD-less staging commands

Baseline `f446c181f675173ddfa820ecd2568d7024625bee`. This port moves the command
choices from `native_fg_dlss_vulkan.cpp`'s `stage` and `transition` into the
Rust recording path. DLSS records one transfer for each accepted source, using
the source image handle and its width and height. Prepare records R32F depth and
then RG16F motion at render size. HUD-less records RGBA8 at output size. Each
uses its own retained slot image and that image's existing initialization flag.

| Order | Vulkan command | Access masks |
| --- | --- | --- |
| 1 | Source `SHADER_READ_ONLY_OPTIMAL` → `TRANSFER_SRC_OPTIMAL` | Both `MEMORY_READ \| MEMORY_WRITE` |
| 2 | Retained image `UNDEFINED` on first use or `SHADER_READ_ONLY_OPTIMAL` on reuse → `TRANSFER_DST_OPTIMAL` | Source zero from `UNDEFINED`, otherwise `MEMORY_READ \| MEMORY_WRITE`. Destination `MEMORY_READ \| MEMORY_WRITE`. |
| 3 | Copy from source to retained image in their transfer layouts | One color region at zero offsets, extent `{source.width, source.height, 1}` |
| 4 | Source `TRANSFER_SRC_OPTIMAL` → `SHADER_READ_ONLY_OPTIMAL` | Both `MEMORY_READ \| MEMORY_WRITE` |
| 5 | Retained image `TRANSFER_DST_OPTIMAL` → `SHADER_READ_ONLY_OPTIMAL` | Both `MEMORY_READ \| MEMORY_WRITE` |

Each barrier uses `ALL_COMMANDS` (`0x10000`) for both stages, ignored queue families,
zero dependency flags and one color image barrier. Barriers and copies cover mip 0
and layer 0, with one level and one layer. Layouts 0, 5, 6 and 7, memory access
bits `0x8000` and `0x10000`, and ignored queue family `0xffffffff` match the local
Vulkan SDK 1.4.357.0 header and are asserted by the typed native recorder.

Native FidelityFX FG uses the same layouts, access, stages and command order for
its HUD-less staging copy, so the two owners share the Rust command sequence and
native descriptor marshalling. FidelityFX still supplies its swapchain extent and
captures the retained image. DLSS supplies the leased producer extent and captures
the producer. The Vulkan-to-D3D12 path keeps its own copy, blit and clear plan. Its
shared images acquire and release external queue ownership, use `GENERAL` between
uses and need separate source and target extents. See the prior source reviews for
[FidelityFX](rust-vulkan-ffx-fg-copy-plan.md) and [D3D12 interop](rust-vulkan-dx12-copy-plan.md).

DLSS still records inside the synchronous Streamline callback after token admission
and, for prepare, after constants succeed. Each image is marked initialized after
its restore barriers. Typed tags use the retained images and the original token.
Successful tags trigger producer capture in the existing recording group. A tag
failure still leaves copies marked recorded for submission or discard. Discard
clears initialization and cancels the capture group. The same source and consumer
fences and SDK timeline value govern reuse and retirement, and Present still uses the
accepted frame identity and existing queue path.

The DLSS private platform table replaces `stage` with `barrier` and `copy` after
`release`, growing from 224 to 232 bytes on x64. The shared barrier and copy records
are 40 and 32 bytes. FidelityFX keeps its 27-pointer table with the same slot order
and record layout. Public ABI versions and entry points keep their existing signatures.

Source review checked the old Vulkan command sequence against the Rust recorder
and typed leaves, and compared the private ABI fields, table initializers and Rust
fixture callbacks. The DLSS native fixture copies the production table and exercises
the original token, typed tags, copy handles, reuse gates and failed-tag retirement.
Its barrier mock discards parameters and its copy mock checks handles, so the exact
layout, access, stages and extent were checked by source comparison. The FidelityFX
native fixture also keeps the production recording leaves. Both disabled create
branches still pass a null table to their Rust validation path before reporting
`NOT_COMPILED` on valid inputs. Rust 1.94.1 workspace formatting, locked offline
Cargo metadata and Git whitespace checks passed. Enabled and disabled builds,
Clippy, fixture execution, Vulkan device and game verification, deployment and push
remain user-owned.
