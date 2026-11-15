# Vulkan FG image enumeration

The first TotK FSR 3 presentation-owner run crashed in `vkCreateImageView` before
generation was enabled. The adapter supplied capacity for eight images. Source
review found that FidelityFX Vulkan, `sdk/src/backends/vk/FrameInterpolationSwapchain/FrameInterpolationSwapchainVK.cpp`,
`FrameInterpolationSwapChainVK::getSwapchainImages`, copies only `gameBufferCount`
handles but does not reduce an oversized caller count. The adapter subsequently
consumed uninitialized tail handles. The SDK reference checkout was not edited.

`native_fg_vulkan.cpp`, `rsf_ffxgvk_images` now queries the actual count, bounds
the fill call and returns only the initialized set. Short buffers retain
VK_INCOMPLETE semantics. The actual owner fixture simulates the SDK behavior and
checks oversized, short and count-only enumeration. It passes.

The real Streamline DLSS FG proxy exposed a separate behavior: filling an
oversized array returned INCOMPLETE, leaving the adapter with no usable image
set. `native_fg_dlss_vulkan.cpp`, `rsf_dlssgvk_images` now also queries first and
fills at the exact available count. Its fixture simulates that proxy response and
checks oversized and short capacities. It passes. Neither path passes a provider
chain to the native loader's WSI table.

TotK also bypassed Streamline for Win32 surface creation. The corrected interposer,
`source/core/sl.interposer/vulkan/wrapper.cpp`, `vkCreateWin32SurfaceKHR`, records
surface-to-HWND ownership needed by DLSS FG. The game now resolves that call
through the SDK-aware instance resolver and destroys the surface through the same
dispatch path. Image enumeration must complete with a nonzero usable set before
acquisition; an acquired index must name a valid image.

## Device evidence

RTX 4070 Laptop GPU, driver 617.42, TotK 1.4.3, validation with GPL off and
descriptor writes. Generation stayed explicitly disabled throughout these runs:

- `build/checks/rsf-fg3-wsi-fixed.log` in the TotK workspace: FSR 3 owner,
  1,588 presents, zero validation errors, shader failures and skipped draws.
- `build/checks/rsf-fg1-wsi-fixed.log`: DLSS owner, three valid proxy images,
  1,574 presents, zero validation errors, shader failures and skipped draws.
- `build/checks/rsf-fg3-guides-fixed.log`: FSR 3 owner and independent FG-only
  depth/motion production, 1,794 presents, zero validation errors, shader failures
  and skipped draws. The moving-object sample reduces median flow error from
  camera-only 6.36 to 1.58 pixels; this does not establish full motion coverage.

Upscaling was off. The DLSS run retained a saved performance quality selection
but still rendered the game's native 1600x896 scene: FG-only production applies
neither SR resolution nor jitter/LOD policy. All artifacts remain ignored.
These runs establish initial provider WSI and input production, not generated
frames. HUD-less output, submitted prepare/HUD-less leases, explicit source
selection at SDK Present, other providers and broader lifecycle/quality checks
remain integration work.

The full native gate runs 232 tests and retains the two existing Anomaly FG
device failures. Focused FFX and DLSS owner fixtures pass. No new Rust behavior is
introduced.
