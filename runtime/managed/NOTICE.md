# Managed Vulkan integration

The provider contracts and Vulkan SDK adapters are MIT, like the rest of ReScaleFrame.

`ReScaleFrame.Contracts` retains the existing `VulkanStory.Contracts` type names so
the host can forward its public contracts without breaking existing assembly references.
`ReScaleFrame.Vulkan` contains game-independent provider ownership and execution.
The host supplies Vulkan resources and retirement through `IUpscalerDevice`.
