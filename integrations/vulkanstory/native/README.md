# VulkanStory provider ABI compatibility

These aliases belong to the VulkanStory standalone package. The current managed
Vulkan module and the integrated native Runtime call the neutral provider
symbols directly. Existing VulkanStory consumers can keep the original
filenames and symbol names.

| Package | Neutral prefix | Compatibility path |
| --- | --- | --- |
| `VulkanStoryStreamline.dll` | `rsf_vk_sl_` | Same-DLL aliases from `sl.def` |
| `VulkanStoryFsr4.dll` | `rsf_vk_fsr4_` | Same-DLL aliases from `fsr4.def` |
| `VulkanStoryXessFg.dll` | `rsf_vk_xess_fg_` | Same-DLL aliases from `xess_fg.def` |
| `VulkanStoryFsr3.dll` | `rsf_vk_fsr3_` | Same-DLL aliases from `fsr3.def` |
| `VulkanStoryNgx.dll`, `libVulkanStoryNgx.so` | `rsf_vk_ngx_` | Rust forwarding exports selected by `vulkanstory-compat` |

The standalone build scripts include the matching alias table. Each alias
shares the same loaded provider and session, and public structure layouts
and ABI versions stay the same. The Windows aliases are resolved by the
linker. The Rust shims use the same typed calls on Windows and Linux, with
the driver-facing call site still in the NGX package module.

The external VulkanStory host package manifests at
`e215d0143df4c0baf0438eb2c309df6085f69ea3` list only the original
filenames. In-place aliases keep those manifests valid and preserve old
consumers without a second provider DLL.
