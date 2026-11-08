# Reproducible FidelityFX Vulkan line-width correction, 8 November 2026

The approved vendor commit `d5902250e8c52e797475ffe629f9c2bbec80efbf` changes
exactly two static rasterization widths from zero to one in the SDK UI-composition
and debug-pacing pipeline constructors. UI composition creates normal and
premultiplied pipelines, so these two defects affect three pipeline creations.
The SDK uses dynamic viewport/scissor, not dynamic line width. The vendor
worker's before/after fixture observed three `00749` errors before the correction
and none after it with `wideLines` disabled. Its evidence remains in the vendor
worktree's ignored `build/linewidth-evidence/REPORT.md` and `manifest.json`.
This is source/API investigation and actual pipeline execution, with no binary
reverse engineering or new engine/game policy.
The ReScaleFrame fixture/source baseline is `32eaee5fcb3ba4c9cb9dd342ce523f1ea4c01e52`.

The RSF build recipe still requires pristine source commit
`c6efa6bf7f2027b3ec94f28578bb5965eabb9e55`. It does not advance the original
detached reference checkout. The new `line-width.json` manifest records both
vendor revisions and exact before/after SHA-256 values. The patch helper checks
both entire normalized-LF files and their single expected anchors before writing
either exported file. Existing WSI dispatch, luma/storage formats, descriptor
pool, swapchain lifetime, CPU-drain and optional trace corrections remain in the
same build flow. Final UI source identity includes the existing lifetime edits.

Schema 2 receipts add source/correction commits, manifest/helper hashes, reviewed
before/after hashes and actual staged hashes. The builder checks staged files on
cache reuse and before publishing a receipt. The runtime hash and all existing
correction hashes remain required. Godot build keys include the new manifest and
helper; runtime staging calls the shared receipt validator. Missing, stale or
mutated identities fail instead of selecting an uncorrected DLL. External
schema-1-only packaging consumers need the corresponding schema-2 validation.

## Executed evidence

- Windows VS2026 Debug `eng/verify.ps1 -VS2026` passed: 103 discovered, 92
  executed, 11 optional skips, fmt/Clippy. The new exact-source/receipt test
  executed against the pinned reference. It covers changed second-file refusal
  without partial writes, wrong revision, duplicate application, stale/missing
  receipt fields, changed manifest/helper/source identities and mutated exports.
- The initial full SDK build failed opening an existing generated shader include
  at a 263-character path. `build/ffx-linewidth-sdk-build.log` and its complete
  failed export under `.local/ffx-linewidth/host/artifacts/full-runtime` remain.
  A shorter fresh output inside this worktree corrected that build-path problem;
  no shader, build target or validation rule was reduced.
- The complete Release `amd_fidelityfx_vk` API build passed, including FSR2,
  FSR3 upscaler, frame interpolation, optical flow, Vulkan backend and shaders.
  Output/receipt: `.local/f/artifacts/a/runtime/`. Log:
  `build/ffx-linewidth-sdk-short-build.log`. Verified cache reuse passed;
  a separate copied cache with a changed UI source was refused by the actual
  builder. Logs: `build/ffx-linewidth-cache-validation.log` and
  `build/ffx-linewidth-mutated-cache-refusal.log`.
- The vendor worker's headless pipeline fixture was copied into the task-owned
  ignored `build/ffx-linewidth-pipeline/`, then relinked against this complete
  build's backend library. Intel UHD Graphics 770 created normal UI,
  premultiplied UI and debug-pacing pipelines with all optional core features,
  including `wideLines`, disabled: zero line-width and zero total validation
  errors. The three existing GalaxyOverlay layer-name warnings were retained.
  Log: `build/ffx-linewidth-pipeline-validation.log`.
- `rsf_native_vulkan_test --device` used this exact runtime on RTX 4070 Laptop
  GPU. FSR2 and FSR3 each completed 12 frames, pixel readback, reset and image-owner
  checks; spatial fallback gradient/borders/alpha/layout checks also passed.
  Khronos core/synchronization validation was enabled with zero VUID, validation
  error or synchronization hazard. Log:
  `build/ffx-linewidth-runtime-validation.log`.

Runtime SHA-256:
`0910650858C02F94B54AFC5215CED36E780D7B446E298AF60BCEEDA7DCD624C1`.
Native SR fixture SHA-256:
`2BA98C2746F967F62583E549BC9211E3ECD21149D9FA47928AF5741253EF9BB3`.
Manifest SHA-256:
`0654267BF28EEE8082A64663BDD2DAE237D9171C127078B177BCBC89BFB235CD`.
The runtime receipt contains the helper, recipe, shader compiler and source-file
identities. MSVC is 19.51.36260.0 / toolset 14.51.36231, CMake 4.4.4 and Vulkan
SDK 1.4.357.0. The reference checkout remains clean at the original pinned commit.

This is built and synthetic/device-tested SDK pipeline and SR behavior. The
pipeline fixture allocates no UI descriptor sets and is not full presenter
lifecycle coverage; the vendor worker separately recorded an existing descriptor
pool/free-set cleanup defect. No real RSF guarded WSI/FG presentation, game,
stereo, headset, visual-quality or overlay compatibility acceptance is claimed.
No game installation, UI, clipboard, original vendor source or global setting
was changed. Parent integration owns distribution of the task-owned runtime.
