# Managed Vulkan integration

The provider contracts and Vulkan SDK adapters were migrated from VulkanStory-Rewrite
revision `0bbcf237ba583e3c02aa19824f3440df145bcaab`. Existing comments and attribution
are retained. Migrated sources retain their original license terms, preserved in
`notices/LICENSE-OPTIMUM-LEGACY-GPL-COMMONS`; moving them does not relicense them.

`ReScaleFrame.Contracts` retains the existing `VulkanStory.Contracts` type names so
the host can forward its public contracts without breaking existing assembly references.
`ReScaleFrame.Vulkan` contains game-independent provider ownership and execution.
The host supplies Vulkan resources and retirement through `IUpscalerDevice`.
