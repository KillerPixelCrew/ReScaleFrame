# FSR3 Vulkan SDK corrections

`luma-history-format.json` records the exact correction for pinned SDK commit
`c6efa6bf7f2027b3ec94f28578bb5965eabb9e55` and [upstream issue 161](https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK/issues/161).
Both luma history images are RGBA16F, while their GLSL storage declaration says
`rgba8`. Vulkan requires matching storage image and view formats. The correction
changes only that declaration to `rgba16f`; resource precision and the algorithm
remain unchanged. Original SDK copyright and license notices are preserved.

`swapchain-lifetime.json` preserves NV low-latency swapchain creation metadata,
removes duplicate semaphore allocation, indexes composition-present semaphores by
the acquired image instead of reusing semaphore zero, and releases cached UI
composition GPU objects after presentation drains. Failed image acquisition does
not signal a binary semaphore that no present will consume. The exact replacements apply only to the
pinned staged sources; the vendor submodule stays unchanged. Runtime receipts
and packaging verify both correction manifests.

Build from the ReScaleFrame repository root, passing the VulkanStory host checkout:

```powershell
& ./runtime/vulkan/fsr3/build-sdk.ps1 -HostRoot <host> -VulkanSdkRoot <VulkanSDK>
```

The script requires Windows x64, CMake, Visual Studio 2022/2026 x64 C++ tools,
Vulkan SDK headers/import library, and the pinned SDK's own shader compiler tools.
It copies tracked vendor sources into a fresh directory under `artifacts/`,
guards the original shader declaration and RGBA16F resource descriptions, and
rebuilds the SDK API target and its shader permutations. MSBuild concurrency is
limited to four jobs and each shader compiler to one worker by default. Only
staged build scheduling is adjusted; shader flags/permutations remain unchanged.
It does not edit the
vendor submodule or launch the game. A failed build produces no runtime receipt.
The host's ordinary Windows `scripts/build-runtime.ps1` invokes this build. A cached
runtime is reused only when its pinned source, correction, build recipe and binary
hashes match the receipt.

The default output is `artifacts/rescaleframe/fsr3-sdk/Release/win-x64/runtime`.
`scripts/prepare-native-bundle.ps1` requires that corrected runtime by default.
For another fresh build directory, pass its `runtime/` subdirectory explicitly
with `-Fsr3RuntimeDirectory`. Every selected runtime requires the build
receipt, pinned source identity, correction identity and matching runtime hash;
it cannot silently use the original prebuilt runtime. A missing corrected build
or stale receipt fails packaging rather than restoring known undefined behavior.
