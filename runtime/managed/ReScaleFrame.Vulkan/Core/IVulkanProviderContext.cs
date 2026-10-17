using Silk.NET.Vulkan;

namespace ReScaleFrame.Vulkan;

/// <summary>Borrowed device and dispatch selected by the renderer; no allocation or queue ownership crosses here.</summary>
internal interface IVulkanProviderContext
{
    Vk Api { get; }
    Device Device { get; }
    PhysicalDevice PhysicalDevice { get; }
    Queue GraphicsQueue { get; }
    uint GraphicsQueueFamily { get; }
    Queue Fsr3AsyncQueue { get; }
    Queue Fsr3PresentQueue { get; }
    Queue Fsr3AcquireQueue { get; }
    bool Fsr3SwapchainQueuesAvailable { get; }
    string Fsr3QueueTopology { get; }
    StreamlineRuntime? Streamline { get; }
    Result WaitDeviceIdle();
    void CheckResult(Result result, string operation);
}
