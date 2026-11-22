// SPDX-License-Identifier: MIT
#[cfg(all(
    windows,
    any(
        feature = "metadata",
        feature = "d3d12",
        feature = "d3d11",
        feature = "vulkan",
        feature = "vulkan-cpu",
        feature = "texture"
    )
))]
mod windows;
